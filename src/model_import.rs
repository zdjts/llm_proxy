//! One-shot import of [models.dev](https://models.dev/api.json) catalog metadata
//! into `model_registry`.
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::AppError;
use crate::model_catalog::canonical_thinking_level;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportCounts {
    pub new: usize,
    pub skipped: usize,
    pub conflicts: usize,
    pub failed: usize,
}

#[derive(Debug, Clone)]
pub struct ImportReport {
    pub counts: ImportCounts,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    provider: String,
    value: Value,
}

/// Community-maintained model catalog used for registry updates.
pub const DEFAULT_SOURCE: &str = "https://models.dev/api.json";

const FETCH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn import_file(
    db: &SqlitePool,
    path: impl AsRef<Path>,
    overwrite: bool,
) -> Result<ImportReport, AppError> {
    let text = tokio::fs::read_to_string(path.as_ref())
        .await
        .map_err(|e| AppError::Config(format!("cannot read model metadata source: {e}")))?;
    import_json(db, &text, overwrite).await
}

pub async fn import_default(db: &SqlitePool, overwrite: bool) -> Result<ImportReport, AppError> {
    import_source(db, DEFAULT_SOURCE, overwrite).await
}

pub async fn import_source(
    db: &SqlitePool,
    source: &str,
    overwrite: bool,
) -> Result<ImportReport, AppError> {
    if is_remote_source(source) {
        let text = fetch_catalog(source).await?;
        import_json(db, &text, overwrite).await
    } else {
        import_file(db, source, overwrite).await
    }
}

pub async fn import_json(
    db: &SqlitePool,
    text: &str,
    overwrite: bool,
) -> Result<ImportReport, AppError> {
    let root: Value = serde_json::from_str(text)
        .map_err(|e| AppError::Config(format!("invalid model metadata JSON: {e}")))?;
    let providers = root
        .as_object()
        .ok_or_else(|| AppError::Config("model metadata root must be an object".into()))?;
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for (provider, group) in providers {
        let Some(models_value) = group.get("models") else {
            continue;
        };
        let models = models_value.as_object().ok_or_else(|| {
            AppError::Config(format!("provider '{provider}' models must be an object"))
        })?;
        for (model_key, model) in models {
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(model_key);
            if id.trim().is_empty() {
                return Err(AppError::Config(
                    "model id must be a non-empty string".into(),
                ));
            }
            if !seen.insert((provider.clone(), id.to_owned())) {
                return Err(AppError::Config(format!(
                    "duplicate source model id '{id}' for provider '{provider}'"
                )));
            }
            candidates.push(Candidate {
                id: id.to_owned(),
                provider: provider.clone(),
                value: model.clone(),
            });
        }
    }
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT logical_model, pool_id, upstream_model FROM routing_config WHERE enabled = 1",
    )
    .fetch_all(db)
    .await
    .map_err(|e| AppError::Internal(format!("load routing config: {e}")))?;
    let mut report = ImportReport {
        counts: ImportCounts::default(),
        reasons: Vec::new(),
    };
    let mut tx = db
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("begin model import: {e}")))?;
    for (destination, pool_id, upstream_model) in rows {
        let catalog_id = upstream_model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(destination.as_str());
        let pool: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM key_pool WHERE id=?1 AND enabled=1")
                .bind(&pool_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?;
        if pool.is_none() {
            report.counts.skipped += 1;
            report
                .reasons
                .push(format!("{destination}: pool unavailable"));
            continue;
        }
        let provider_rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, kind FROM provider_config WHERE pool_id=?1 AND enabled=1")
                .bind(&pool_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(db_err)?;
        if provider_rows.is_empty() {
            report.counts.skipped += 1;
            report
                .reasons
                .push(format!("{destination}: provider unavailable"));
            continue;
        }
        let Some(source) = pick_catalog_match(&candidates, catalog_id, &provider_rows) else {
            report.counts.skipped += 1;
            report
                .reasons
                .push(format!("{destination}: no associated metadata"));
            continue;
        };
        let Some((provider_id, provider_kind)) =
            pick_local_provider(&provider_rows, &source.provider)
        else {
            report.counts.conflicts += 1;
            report.reasons.push(format!(
                "{destination}: provider association is not unambiguous"
            ));
            continue;
        };
        let mapped = map_model(&destination, provider_kind, provider_id, &source.value)
            .map_err(|e| AppError::Config(format!("{destination}: {e}")))?;
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM model_registry WHERE id=?1")
            .bind(&destination)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
        if exists.is_some() && !overwrite {
            report.counts.conflicts += 1;
            report
                .reasons
                .push(format!("{destination}: protected existing record"));
            continue;
        }
        if exists.is_some() {
            sqlx::query("UPDATE model_registry SET display_name=?1,provider_kind=?2,provider_config_id=?3,supports_vision=?4,supports_tool_calling=?5,supports_json_mode=?6,max_context_tokens=?7,max_output_tokens=?8,input_price_per_1m=?9,output_price_per_1m=?10,capabilities_json=?11,updated_at=unixepoch('subsec')*1000 WHERE id=?12")
                .bind(&mapped.0).bind(provider_kind).bind(provider_id).bind(mapped.1 as i32).bind(mapped.2 as i32).bind(mapped.3 as i32).bind(mapped.4).bind(mapped.5).bind(mapped.6).bind(mapped.7).bind(mapped.8).bind(&destination).execute(&mut *tx).await.map_err(db_err)?;
        } else {
            sqlx::query("INSERT INTO model_registry (id,display_name,provider_kind,provider_config_id,supports_vision,supports_tool_calling,supports_json_mode,max_context_tokens,max_output_tokens,input_price_per_1m,output_price_per_1m,capabilities_json,enabled) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,1)")
                .bind(&destination).bind(&mapped.0).bind(provider_kind).bind(provider_id).bind(mapped.1 as i32).bind(mapped.2 as i32).bind(mapped.3 as i32).bind(mapped.4).bind(mapped.5).bind(mapped.6).bind(mapped.7).bind(mapped.8).execute(&mut *tx).await.map_err(db_err)?;
        }
        report.counts.new += 1;
    }
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("commit model import: {e}")))?;
    Ok(report)
}

fn is_remote_source(source: &str) -> bool {
    let lower = source.trim();
    lower.starts_with("https://") || lower.starts_with("http://")
}

async fn fetch_catalog(url: &str) -> Result<String, AppError> {
    let client = reqwest::Client::builder()
        .connect_timeout(FETCH_CONNECT_TIMEOUT)
        .timeout(FETCH_TIMEOUT)
        .user_agent("llm_proxy/2.0")
        .build()
        .map_err(|e| AppError::Internal(format!("model catalog HTTP client: {e}")))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Config(format!("cannot fetch model catalog: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Config(format!(
            "cannot fetch model catalog: HTTP {status}"
        )));
    }
    response
        .text()
        .await
        .map_err(|e| AppError::Config(format!("cannot read model catalog body: {e}")))
}

fn db_err(e: sqlx::Error) -> AppError {
    AppError::Internal(format!("model import database error: {e}"))
}
fn norm(s: &str) -> String {
    s.to_ascii_lowercase().replace(['-', '_', ' '], "")
}
fn model_id_matches(destination: &str, catalog_id: &str) -> bool {
    destination == catalog_id
        || destination.ends_with(&format!("/{catalog_id}"))
        || catalog_id.ends_with(&format!("/{destination}"))
        || destination.contains(catalog_id)
}
fn catalog_score(
    destination: &str,
    candidate: &Candidate,
    provider_rows: &[(String, String)],
) -> i32 {
    let mut score = 0;
    if candidate.id == destination {
        score += 100;
    } else if destination.ends_with(&format!("/{}", candidate.id))
        || candidate.id.ends_with(&format!("/{destination}"))
    {
        score += 80;
    } else if destination.contains(&candidate.id) {
        score += 40 + candidate.id.len().min(20) as i32;
    }
    if provider_rows
        .iter()
        .any(|(id, kind)| compatible_provider(&candidate.provider, id, kind))
    {
        score += 20;
    }
    score += canonical_catalog_rank(&candidate.provider);
    score
}
fn canonical_catalog_rank(provider: &str) -> i32 {
    match norm(provider).as_str() {
        "openai" | "anthropic" | "google" | "xai" | "deepseek" | "mistral" => 15,
        "zai" => 14,
        "zhipuai" => 13,
        "zhipu" => 12,
        "groq" => 11,
        _ => 0,
    }
}
fn pick_catalog_match<'a>(
    candidates: &'a [Candidate],
    destination: &str,
    provider_rows: &[(String, String)],
) -> Option<&'a Candidate> {
    let mut best: Option<(&Candidate, i32)> = None;
    for candidate in candidates {
        if !model_id_matches(destination, &candidate.id) {
            continue;
        }
        let score = catalog_score(destination, candidate, provider_rows);
        best = match best {
            None => Some((candidate, score)),
            Some((best_candidate, best_score)) => match score.cmp(&best_score) {
                Ordering::Greater => Some((candidate, score)),
                Ordering::Less => best,
                Ordering::Equal => match tie_break(candidate, best_candidate) {
                    Ordering::Less => Some((candidate, score)),
                    Ordering::Greater | Ordering::Equal => best,
                },
            },
        };
    }
    best.map(|(candidate, _)| candidate)
}

/// Deterministic tie-break for equally scored catalog candidates.
/// Candidates with cost metadata are preferred (imports populate pricing
/// columns); remaining ties resolve to the lexicographically smallest
/// (provider, id) so repeated imports are idempotent.
fn tie_break(a: &Candidate, b: &Candidate) -> Ordering {
    let a_cost = a.value.get("cost").is_some();
    let b_cost = b.value.get("cost").is_some();
    b_cost.cmp(&a_cost).then_with(|| {
        (a.provider.as_str(), a.id.as_str()).cmp(&(b.provider.as_str(), b.id.as_str()))
    })
}
fn pick_local_provider<'a>(
    provider_rows: &'a [(String, String)],
    catalog_provider: &str,
) -> Option<(&'a String, &'a String)> {
    if provider_rows.len() == 1 {
        return provider_rows.first().map(|(id, kind)| (id, kind));
    }
    let matching: Vec<_> = provider_rows
        .iter()
        .filter(|(id, kind)| compatible_provider(catalog_provider, id, kind))
        .collect();
    if matching.len() == 1 {
        return matching.first().map(|(id, kind)| (id, kind));
    }
    None
}
fn compatible_provider(source: &str, configured_id: &str, configured_kind: &str) -> bool {
    let s = norm(source);
    let id = norm(configured_id);
    let kind = norm(configured_kind);
    if s == id || s == kind {
        return true;
    }
    matches!(
        (s.as_str(), kind.as_str()),
        ("xai", "openai")
            | ("xai", "azure")
            | ("deepseek", "openai")
            | ("google", "gemini")
            | ("openai", "azure")
            | ("azureopenai", "azure")
            | ("azureopenai", "openai")
            | ("zai", "openai")
            | ("zhipuai", "openai")
            | ("zhipu", "openai")
    )
}

type MappedModel = (
    String,
    bool,
    bool,
    bool,
    i64,
    i64,
    Option<f64>,
    Option<f64>,
    String,
);

fn map_model(
    destination: &str,
    kind: &str,
    provider_id: &str,
    model: &Value,
) -> Result<MappedModel, String> {
    let name = model
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("model name must be non-empty")?
        .to_owned();
    let limit = model
        .get("limit")
        .and_then(Value::as_object)
        .ok_or("limit must be an object")?;
    let context = positive_i64(limit.get("context"), "limit.context")?;
    let output = positive_i64(limit.get("output"), "limit.output")?;
    let vision = model
        .get("modalities")
        .and_then(|m| m.get("input"))
        .and_then(Value::as_array)
        .map(|input| input.iter().any(|v| v.as_str() == Some("image")))
        .unwrap_or(false);
    let tools = model
        .get("tool_call")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let json_mode = model
        .get("structured_output")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (ip, op) = if let Some(cost) = model.get("cost") {
        (Some(price(cost, "input")?), Some(price(cost, "output")?))
    } else {
        (None, None)
    };
    let mut safe = sanitize(model);
    if let Value::Object(ref mut o) = safe {
        o.remove("id");
        o.remove("name");
        if !o.contains_key("thinkingLevelMap")
            && let Some(map) = thinking_level_map(model)
        {
            o.insert("thinkingLevelMap".into(), map);
        }
    }
    let caps = serde_json::to_string(&serde_json::json!({
        "source_provider": provider_id,
        "reasoning": model.get("reasoning").cloned().unwrap_or(Value::Null),
        "metadata": safe
    }))
    .map_err(|_| "invalid capabilities JSON")?;
    let _ = (destination, kind);
    Ok((
        name, vision, tools, json_mode, context, output, ip, op, caps,
    ))
}
fn positive_i64(v: Option<&Value>, field: &str) -> Result<i64, String> {
    let n = v
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{field} must be an integer"))?;
    if n <= 0 {
        Err(format!("{field} must be positive"))
    } else {
        Ok(n)
    }
}
fn price(cost: &Value, field: &str) -> Result<f64, String> {
    let n = cost
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("cost.{field} must be a number"))?;
    if !n.is_finite() || n < 0.0 {
        Err(format!("cost.{field} invalid"))
    } else {
        Ok(n)
    }
}
fn sensitive(k: &str) -> bool {
    let n = norm(k);
    [
        "key",
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
    ]
    .iter()
    .any(|x| n.contains(x))
}
/// Build the per-model `canonical level → upstream wire value` map from a
/// models.dev `reasoning_options` array.
///
/// Keys are canonicalised to pi/gateway vocabulary: models.dev spells the
/// "thinking disabled" effort value `none` for some catalogs (xAI, hy4),
/// which canonicalises to `off`.  Other canonical levels pass through; any
/// value outside the canonical set is dropped rather than stored under a key
/// no consumer can name.  Values are preserved verbatim — they are the wire
/// values the upstream API expects in `reasoning_effort`.
fn thinking_level_map(model: &Value) -> Option<Value> {
    let options = model.get("reasoning_options")?.as_array()?;
    let effort = options.iter().find(|option| {
        option
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "effort")
    })?;
    let values = effort.get("values")?.as_array()?;
    let mut map = serde_json::Map::new();
    for value in values {
        let Some(level) = value.as_str().filter(|s| !s.trim().is_empty()) else {
            continue;
        };
        let Some(canonical) = canonical_thinking_level(level) else {
            continue;
        };
        map.insert(canonical.to_owned(), Value::String(level.to_owned()));
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}
fn sanitize(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !sensitive(k))
                .map(|(k, v)| (k.clone(), sanitize(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(sanitize).collect()),
        _ => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contiguous_match_is_used() {
        assert!("grok-4.5_oa".contains("grok-4.5"));
    }
    #[test]
    fn sanitizer_removes_secrets() {
        let v = sanitize(&serde_json::json!({"apiKey":"SECRET","tool_call":true}));
        assert!(!v.to_string().contains("SECRET"));
    }
    #[test]
    fn remote_source_detects_http_urls() {
        assert!(is_remote_source("https://models.dev/api.json"));
        assert!(is_remote_source("http://127.0.0.1:9/api.json"));
        assert!(!is_remote_source("models-store.json"));
        assert!(!is_remote_source("./api.json"));
    }
    #[test]
    fn thinking_map_is_built_from_models_dev_effort_values() {
        let map = thinking_level_map(&serde_json::json!({
            "reasoning_options": [{"type": "effort", "values": ["low", "medium", "high"]}]
        }))
        .unwrap();
        assert_eq!(map["low"], "low");
        assert_eq!(map["high"], "high");
        assert!(map.get("off").is_none());
    }
}
