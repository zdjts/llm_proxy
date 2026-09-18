//! Protocol-agnostic response normalization.
//!
//! Downstream clients such as pi fold chain-of-thought from
//! `reasoning_content` (also `reasoning` / `reasoning_text`).  They do
//! **not** parse `<think>` tags inside `content`.
//!
//! This module is a pure post-processing pass, provider-agnostic:
//!
//! 1. Fold alias reasoning fields into OpenAI `reasoning_content`.
//! 2. As a fallback, pull inlined `<think>…</think>` /
//!    `<thinking>…</thinking>` blocks out of `content`.
//! 3. If neither is present, leave the payload alone.
//!
//! Tag pairs are **not** operator-configurable.  An empty `close` string
//! is still accepted by [`ThinkTagAccumulator`] for tests, but the
//! production path always uses [`builtin_think_pairs`].

use crate::types::{ChatCompletionResponse, ResponseMessage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThinkEvent {
    /// Bytes belonging to the model's hidden reasoning chain.  Should
    /// be appended to the response's `reasoning_content` field.
    Reasoning(String),
    /// Bytes belonging to the user-visible answer.  Should be forwarded
    /// as the response's `content` field.
    Text(String),
    /// The accumulator is holding the tail of `pending` because it might
    /// still be the prefix of a known tag.  No further action needed —
    /// the next `feed` call will resolve it.
    Pending,
}

/// Built-in wrappers recognised on the production path.
pub fn builtin_think_pairs() -> Vec<(String, String)> {
    vec![
        ("<think>".into(), "</think>".into()),
        ("<thinking>".into(), "</thinking>".into()),
    ]
}

fn take_nonempty(slot: &mut Option<String>) -> Option<String> {
    slot.take().filter(|s| !s.is_empty())
}

fn merge_nonempty(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (None, None) => None,
        (Some(a), None) | (None, Some(a)) => Some(a),
        (Some(a), Some(b)) => {
            if a.is_empty() {
                Some(b)
            } else if b.is_empty() {
                Some(a)
            } else {
                Some(format!("{a}\n{b}"))
            }
        }
    }
}

/// Fold `reasoning` / `reasoning_text` / string `thinking` into
/// `reasoning_content` so clients only need to look at one field.
pub fn fold_message_reasoning(msg: &mut ResponseMessage) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(s) = take_nonempty(&mut msg.reasoning_content) {
        parts.push(s);
    }
    if let Some(s) = take_nonempty(&mut msg.reasoning) {
        parts.push(s);
    }
    if let Some(s) = take_nonempty(&mut msg.reasoning_text) {
        parts.push(s);
    }
    if let Some(serde_json::Value::String(s)) = msg.thinking.as_ref()
        && !s.is_empty()
    {
        parts.push(s.clone());
        msg.thinking = None;
    }
    msg.reasoning_content = parts
        .into_iter()
        .fold(None, |acc, next| merge_nonempty(acc, Some(next)));
}

/// Non-streaming entry point used by the handler.
pub fn normalize_chat_response(resp: &mut ChatCompletionResponse) {
    let pairs = builtin_think_pairs();
    for choice in &mut resp.choices {
        fold_message_reasoning(&mut choice.message);
        let Some(content) = choice.message.content.as_mut() else {
            continue;
        };
        if content.is_empty() {
            continue;
        }
        let (extracted, cleaned) = extract_think_tags(content, &pairs);
        *content = cleaned;
        if let Some(extracted) = extracted.filter(|s| !s.is_empty()) {
            choice.message.reasoning_content =
                merge_nonempty(choice.message.reasoning_content.take(), Some(extracted));
        }
    }
}

/// One-shot extraction over a fully-buffered `content` string.
///
/// Returns `(reasoning, cleaned_content)` where:
///
/// - `reasoning = None` if no opening tag was ever matched.
/// - `reasoning = Some(s)` if at least one think block was found.  When
///   multiple blocks exist they are joined with `\n`.
/// - `cleaned_content` is `content` with every recognised
///   `<open>…<close>` span removed.  An unclosed think block at EOF is
///   folded into `reasoning`; a leftover tag *prefix* in text mode is
///   returned as `cleaned_content` so we do not invent reasoning.
pub fn extract_think_tags(content: &str, pairs: &[(String, String)]) -> (Option<String>, String) {
    let mut acc = ThinkTagAccumulator::new(pairs.to_vec());
    let events = acc.feed(content);
    let mut reasoning = String::new();
    let mut text = String::new();
    let mut saw_reasoning = false;
    for ev in events {
        match ev {
            ThinkEvent::Reasoning(s) => {
                saw_reasoning = true;
                if !reasoning.is_empty() && !s.is_empty() {
                    reasoning.push('\n');
                }
                reasoning.push_str(&s);
            }
            ThinkEvent::Text(s) => text.push_str(&s),
            ThinkEvent::Pending => {}
        }
    }
    let flushed = acc.flush();
    if !flushed.is_empty() {
        if acc.in_reasoning() {
            saw_reasoning = true;
            if !reasoning.is_empty() {
                reasoning.push('\n');
            }
            reasoning.push_str(&flushed);
        } else {
            text.push_str(&flushed);
        }
    }
    let reasoning = if saw_reasoning || acc.tag_count() > 0 {
        Some(reasoning)
    } else {
        None
    };
    (reasoning, text)
}

/// Streaming state machine.
///
/// Holds the pending byte buffer (which may end mid-tag), the configured
/// tag pairs, and the current in-reasoning vs in-text state.  Calls to
/// [`ThinkTagAccumulator::feed`] return a `Vec<ThinkEvent>` describing
/// everything that could be resolved from this chunk alone.
#[derive(Debug)]
pub struct ThinkTagAccumulator {
    pairs: Vec<(String, String)>,
    state: State,
    /// Bytes received from the upstream that we have not yet committed
    /// to either side because the tail might still be a tag prefix.
    pending: String,
    /// Cached maximum of all tag lengths (open and close).  Rebuild
    /// when the pair list changes.
    longest_tag: usize,
    /// Count of `<open>` / `<close>` matches processed during the
    /// lifetime of this accumulator.  Used by [`extract_think_tags`] to
    /// distinguish "matched an empty think block" (open + close back to
    /// back, reasoning content was empty) from "no think blocks at all".
    tag_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Currently in user-visible text.  An open tag transitions to
    /// `InReasoning`.
    InText,
    /// Currently in a reasoning block.  A close tag transitions to
    /// `InText`.
    InReasoning,
}

impl ThinkTagAccumulator {
    pub fn new(pairs: Vec<(String, String)>) -> Self {
        let longest_tag = pairs
            .iter()
            .map(|(o, c)| o.len().max(c.len()))
            .max()
            .unwrap_or(0);
        Self {
            pairs,
            state: State::InText,
            pending: String::new(),
            longest_tag,
            tag_count: 0,
        }
    }

    /// True iff the accumulator is currently inside a reasoning block.
    pub fn in_reasoning(&self) -> bool {
        matches!(self.state, State::InReasoning)
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Number of `<open>` / `<close>` tags the state machine has
    /// successfully matched so far.  Use this to tell "matched an
    /// empty think block" apart from "no think blocks at all".
    pub fn tag_count(&self) -> usize {
        self.tag_count
    }

    /// Feed a new chunk of `content` into the state machine.  Returns
    /// the events that can be emitted right now.  A trailing
    /// `ThinkEvent::Pending` is yielded when the accumulator is waiting
    /// for the next chunk to disambiguate a possible tag prefix.
    pub fn feed(&mut self, delta: &str) -> Vec<ThinkEvent> {
        self.pending.push_str(delta);
        self.drain_resolved()
    }

    /// Drain the pending buffer, emitting everything that resolves to a
    /// concrete side.  Returns `Pending` if a tail byte sequence must be
    /// held back because it could still be the prefix of a tag.
    fn drain_resolved(&mut self) -> Vec<ThinkEvent> {
        let mut events = Vec::new();
        loop {
            let Some((pos, tag, kind)) = self.find_next_tag() else {
                // No more complete tags in `pending`.  See whether any
                // tail bytes need to be held back as a potential prefix.
                let flushed = self.hold_partial_tail();
                if !flushed.is_empty() {
                    match self.state {
                        State::InText => events.push(ThinkEvent::Text(flushed)),
                        State::InReasoning => events.push(ThinkEvent::Reasoning(flushed)),
                    }
                }
                if !self.pending.is_empty() {
                    events.push(ThinkEvent::Pending);
                }
                break;
            };

            let tag_len = tag.len();
            let before = self.pending[..pos].to_string();
            self.pending.drain(..pos);
            self.pending.drain(..tag_len);
            self.tag_count += 1;

            if !before.is_empty() {
                match self.state {
                    State::InText => events.push(ThinkEvent::Text(before)),
                    State::InReasoning => events.push(ThinkEvent::Reasoning(before)),
                }
            }
            match kind {
                TagKind::Open => self.state = State::InReasoning,
                TagKind::Close => self.state = State::InText,
            }
        }
        events
    }

    /// Look for the earliest matching tag in `self.pending`.  Returns
    /// `(byte_offset, tag_str, kind)`.
    ///
    /// Close tags are only considered when the state machine is
    /// currently inside a reasoning block — an orphan close in text
    /// would otherwise be stripped from legitimate text like
    /// `</thinking about it>`.  Pairs whose `open` is empty are skipped
    /// entirely because they cannot represent a coherent think block.
    fn find_next_tag(&self) -> Option<(usize, &str, TagKind)> {
        let mut best: Option<(usize, &str, TagKind)> = None;
        for (open, close) in &self.pairs {
            if open.is_empty() {
                continue;
            }
            for (needle, kind) in [
                (open.as_str(), TagKind::Open),
                (close.as_str(), TagKind::Close),
            ] {
                if needle.is_empty() {
                    continue;
                }
                if kind == TagKind::Close && matches!(self.state, State::InText) {
                    continue;
                }
                if let Some(idx) = self.pending.find(needle) {
                    let replace = best.is_none_or(|(b, best_tag, _)| {
                        idx < b || (idx == b && needle.len() > best_tag.len())
                    });
                    if replace {
                        best = Some((idx, needle, kind));
                    }
                }
            }
        }
        best
    }

    /// Find the latest position in `pending` where a non-empty prefix
    /// of some configured tag starts, drain everything before it into
    /// the return value, and leave the prefix (and any non-matching
    /// bytes that follow it) in `pending` for the next `feed` call.
    ///
    /// Returns the bytes that should be emitted right now.  An empty
    /// return value means the entire `pending` is a potential tag
    /// prefix and must be held.
    fn hold_partial_tail(&mut self) -> String {
        if self.pending.is_empty() || self.longest_tag == 0 {
            return std::mem::take(&mut self.pending);
        }
        // The longest possible held tail is `longest_tag - 1` bytes —
        // any longer than that and we already have enough characters to
        // match the longest known tag in full.
        let max_keep = self.longest_tag - 1;
        let min_pos = self.pending.len().saturating_sub(max_keep);

        // Only walk UTF-8 character starts.  Byte offsets inside a
        // multibyte rune (e.g. 户) panic on `&str` slicing and would
        // reset the SSE socket.
        let mut best_pos: Option<usize> = None;
        'pos_loop: for (pos, _) in self.pending.char_indices().rev() {
            if pos < min_pos {
                break;
            }
            let tail = &self.pending[pos..];
            let mut end = 0;
            for ch in tail.chars() {
                end += ch.len_utf8();
                let substr = &tail[..end];
                let mut matches = false;
                for (open, close) in &self.pairs {
                    if open.is_empty() {
                        continue;
                    }
                    for needle in [open.as_str(), close.as_str()] {
                        if !needle.is_empty() && needle.starts_with(substr) {
                            matches = true;
                            break;
                        }
                    }
                    if matches {
                        break;
                    }
                }
                if matches {
                    best_pos = Some(pos);
                    break 'pos_loop;
                }
            }
        }

        match best_pos {
            None => std::mem::take(&mut self.pending),
            Some(0) => String::new(),
            Some(pos) => {
                let drained = self.pending[..pos].to_string();
                self.pending.drain(..pos);
                drained
            }
        }
    }

    /// Flush any remaining pending bytes.  Conservative fallback: emit
    /// as `Reasoning` so we never silently leak a half-stripped think
    /// block into user-visible text.
    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Open,
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paired() -> Vec<(String, String)> {
        vec![("<think>".into(), "</think>".into())]
    }

    fn empty_close() -> Vec<(String, String)> {
        vec![("<think>".into(), String::new())]
    }

    #[test]
    fn no_tag_passes_through_unchanged() {
        let (r, c) = extract_think_tags("hello world", &paired());
        assert_eq!(r, None);
        assert_eq!(c, "hello world");
    }

    #[test]
    fn single_closed_block() {
        let (r, c) = extract_think_tags("<think>chain of thought</think>answer", &paired());
        assert_eq!(r.as_deref(), Some("chain of thought"));
        assert_eq!(c, "answer");
    }

    #[test]
    fn multiple_blocks_joined_with_newline() {
        let (r, c) = extract_think_tags(
            "<think>first</think>mid<think>second</think>tail",
            &paired(),
        );
        assert_eq!(r.as_deref(), Some("first\nsecond"));
        assert_eq!(c, "midtail");
    }

    #[test]
    fn block_split_across_chunks() {
        // Tag spans the SSE chunk boundary: "<th" at the end of chunk
        // N and "ink>..." at the start of chunk N+1.
        let mut acc = ThinkTagAccumulator::new(paired());
        let mut r = String::new();
        let mut t = String::new();
        let mut had_pending = false;
        for chunk in ["<th", "ink>reasoning</think>res"] {
            for ev in acc.feed(chunk) {
                match ev {
                    ThinkEvent::Reasoning(s) => r.push_str(&s),
                    ThinkEvent::Text(s) => t.push_str(&s),
                    ThinkEvent::Pending => had_pending = true,
                }
            }
        }
        assert!(had_pending, "expected at least one Pending event");
        assert_eq!(r, "reasoning");
        assert_eq!(t, "res");
    }

    #[test]
    fn unclosed_block_with_close_tag_falls_back_to_text() {
        let (r, c) = extract_think_tags("<think>half-formed reasoning", &paired());
        let r = r.expect("reasoning should be set");
        assert!(r.contains("half-formed reasoning"), "got {r:?}");
        assert_eq!(c, "");
    }

    #[test]
    fn block_embedded_in_text() {
        let (r, c) = extract_think_tags("before<think>hidden</think>after", &paired());
        assert_eq!(r.as_deref(), Some("hidden"));
        assert_eq!(c, "beforeafter");
    }

    #[test]
    fn empty_content_yields_empty_outputs() {
        let (r, c) = extract_think_tags("", &paired());
        assert_eq!(r, None);
        assert_eq!(c, "");
    }

    #[test]
    fn block_at_start_and_end() {
        let (r, c) = extract_think_tags("<think>a</think>middle<think>b</think>", &paired());
        assert_eq!(r.as_deref(), Some("a\nb"));
        assert_eq!(c, "middle");
    }

    #[test]
    fn multiple_tag_pairs_recognised() {
        let pairs = vec![
            ("<think>".into(), "</think>".into()),
            ("<thinking>".into(), "</thinking>".into()),
        ];
        let (r, c) = extract_think_tags("<think>A</think>mid<thinking>B</think>done", &pairs);
        assert_eq!(r.as_deref(), Some("A\nB"));
        assert_eq!(c, "middone");
    }

    #[test]
    fn case_sensitive_no_normalization() {
        // `<Think>` is NOT recognised — operators must list each casing.
        let (r, c) = extract_think_tags("<Think>not me</think>text", &paired());
        assert_eq!(r, None);
        assert_eq!(c, "<Think>not me</think>text");
    }

    #[test]
    fn empty_close_tag_folds_rest_into_reasoning() {
        let (r, c) = extract_think_tags("<think>hidden</think>rest", &empty_close());
        let r = r.expect("reasoning should be set");
        assert!(
            r.contains("hidden"),
            "reasoning should contain 'hidden', got {r:?}"
        );
        assert_eq!(c, "");
    }

    #[test]
    fn feeds_resolve_pending_when_next_chunk_disambiguates() {
        let mut acc = ThinkTagAccumulator::new(paired());
        let ev1 = acc.feed("<thin");
        assert!(matches!(ev1.last(), Some(ThinkEvent::Pending)));
        let ev2 = acc.feed("k>secret</think>visible");
        let mut r = String::new();
        let mut t = String::new();
        for ev in ev2 {
            match ev {
                ThinkEvent::Reasoning(s) => r.push_str(&s),
                ThinkEvent::Text(s) => t.push_str(&s),
                ThinkEvent::Pending => {}
            }
        }
        assert_eq!(r, "secret");
        assert_eq!(t, "visible");
    }

    #[test]
    fn feeds_emit_text_immediately_when_no_tag_is_involved() {
        let mut acc = ThinkTagAccumulator::new(paired());
        let events = acc.feed("just text");
        assert_eq!(events, vec![ThinkEvent::Text("just text".into())]);
    }

    #[test]
    fn text_split_with_no_lag_has_no_pending_at_end() {
        let mut acc = ThinkTagAccumulator::new(paired());
        let events = acc.feed("hello world");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ThinkEvent::Text(s) => assert_eq!(s, "hello world"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn empty_open_tag_in_pair_is_skipped() {
        // Defensive: an empty open tag would match every position via
        // `str::find`, which would be ambiguous.  We treat empty tags as
        // "no match" so operators cannot accidentally configure an
        // infinite loop / no-op.
        let pairs = vec![(String::new(), "</think>".into())];
        let (r, c) = extract_think_tags("hello</think>world", &pairs);
        assert_eq!(r, None);
        assert_eq!(c, "hello</think>world");
    }

    #[test]
    fn feeds_split_open_tag_across_three_chunks() {
        // <thi | n> | k>rest</think>answer
        let mut acc = ThinkTagAccumulator::new(paired());
        let mut all_events: Vec<ThinkEvent> = Vec::new();
        for chunk in ["<thi", "n", "k>rest</think>answer"] {
            all_events.extend(acc.feed(chunk));
        }
        let mut r = String::new();
        let mut t = String::new();
        for ev in all_events {
            match ev {
                ThinkEvent::Reasoning(s) => r.push_str(&s),
                ThinkEvent::Text(s) => t.push_str(&s),
                ThinkEvent::Pending => {}
            }
        }
        assert_eq!(r, "rest");
        assert_eq!(t, "answer");
    }

    #[test]
    fn embedded_close_tag_closes_first_block() {
        let (r, c) = extract_think_tags("<think>I said </think> in my head</think>real", &paired());
        assert_eq!(r.as_deref(), Some("I said "));
        assert_eq!(c, " in my head</think>real");
    }

    #[test]
    fn close_tag_at_very_start_emits_empty_then_text() {
        let (r, c) = extract_think_tags("<think></think>visible", &paired());
        assert_eq!(r.as_deref(), Some(""));
        assert_eq!(c, "visible");
    }

    #[test]
    fn adjacent_blocks_back_to_back() {
        let (r, c) = extract_think_tags("<think>A</think><think>B</think>visible", &paired());
        assert_eq!(r.as_deref(), Some("A\nB"));
        assert_eq!(c, "visible");
    }

    #[test]
    fn trailing_partial_tag_stays_in_text() {
        let (r, c) = extract_think_tags("hello <th", &paired());
        assert_eq!(r, None);
        assert_eq!(c, "hello <th");
    }

    #[test]
    fn chinese_bytes_do_not_panic_and_pass_through() {
        let (r, c) = extract_think_tags("用户户户户户户", &paired());
        assert_eq!(r, None);
        assert_eq!(c, "用户户户户户户");
    }

    #[test]
    fn chinese_around_think_tags() {
        let (r, c) = extract_think_tags("用户<think>想</think>好", &paired());
        assert_eq!(r.as_deref(), Some("想"));
        assert_eq!(c, "用户好");
    }

    #[test]
    fn thinking_is_not_split_as_think_prefix() {
        let (r, c) =
            extract_think_tags("<thinking>hidden</thinking>visible", &builtin_think_pairs());
        assert_eq!(r.as_deref(), Some("hidden"));
        assert_eq!(c, "visible");
    }

    fn sample_message(
        content: Option<&str>,
        reasoning: Option<&str>,
        reasoning_content: Option<&str>,
    ) -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: "r".into(),
            object: "chat.completion".into(),
            created: 1,
            model: "m".into(),
            choices: vec![crate::types::Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: content.map(str::to_string),
                    tool_calls: None,
                    reasoning_content: reasoning_content.map(str::to_string),
                    reasoning: reasoning.map(str::to_string),
                    reasoning_text: None,
                    thinking: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            raw_usage_json: None,
        }
    }

    #[test]
    fn normalize_folds_reasoning_alias() {
        let mut resp = sample_message(Some("visible"), Some("hidden"), None);
        normalize_chat_response(&mut resp);
        let msg = &resp.choices[0].message;
        assert_eq!(msg.content.as_deref(), Some("visible"));
        assert_eq!(msg.reasoning_content.as_deref(), Some("hidden"));
        assert!(msg.reasoning.is_none());
    }

    #[test]
    fn normalize_merges_alias_and_think_tags() {
        let mut resp = sample_message(Some("before<think>tag</think>after"), Some("alias"), None);
        normalize_chat_response(&mut resp);
        let msg = &resp.choices[0].message;
        assert_eq!(msg.content.as_deref(), Some("beforeafter"));
        assert_eq!(msg.reasoning_content.as_deref(), Some("alias\ntag"));
        assert!(msg.reasoning.is_none());
    }
}
