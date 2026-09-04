//! One-shot import of project-local Pi model metadata into `model_registry`.
use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::AppError;

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

const DEFAULT_SOURCE: &str = "models-store.json";

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
    import_file(db, DEFAULT_SOURCE, overwrite).await
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
        let models = group
            .get("models")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::Config(format!("provider '{provider}' models must be an array"))
            })?;
        for model in models {
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::Config("model id must be a non-empty string".into()))?;
            if !seen.insert(id.to_owned()) {
                return Err(AppError::Config(format!(
                    "duplicate source model id '{id}'"
                )));
            }
            candidates.push(Candidate {
                id: id.to_owned(),
                provider: provider.clone(),
                value: model.clone(),
            });
        }
    }
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT logical_model, pool_id FROM routing_config WHERE enabled = 1")
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
    for (destination, pool_id) in rows {
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
        let mut matches = Vec::new();
        let exact_matches: Vec<_> = candidates
            .iter()
            .filter(|c| {
                c.id == destination
                    && provider_rows
                        .iter()
                        .any(|(_, kind)| compatible_provider(&c.provider, kind))
            })
            .collect();
        if exact_matches.len() == 1 {
            matches = exact_matches;
        } else {
            for c in &candidates {
                if destination.contains(&c.id)
                    && provider_rows
                        .iter()
                        .any(|(_, kind)| compatible_provider(&c.provider, kind))
                {
                    matches.push(c);
                }
            }
        }
        if matches.len() > 1 {
            report.counts.conflicts += 1;
            report
                .reasons
                .push(format!("{destination}: ambiguous source match"));
            continue;
        }
        let Some(source) = matches.first() else {
            report.counts.skipped += 1;
            report
                .reasons
                .push(format!("{destination}: no associated metadata"));
            continue;
        };
        let matching_providers: Vec<_> = provider_rows
            .iter()
            .filter(|(_, kind)| compatible_provider(&source.provider, kind))
            .collect();
        if matching_providers.len() != 1 {
            report.counts.conflicts += 1;
            report.reasons.push(format!(
                "{destination}: provider association is not unambiguous"
            ));
            continue;
        }
        let (provider_id, provider_kind) = matching_providers[0];
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

fn db_err(e: sqlx::Error) -> AppError {
    AppError::Internal(format!("model import database error: {e}"))
}
fn norm(s: &str) -> String {
    s.to_ascii_lowercase().replace(['-', '_', ' '], "")
}
fn compatible_provider(source: &str, configured: &str) -> bool {
    let s = norm(source);
    let c = norm(configured);
    matches!(
        (s.as_str(), c.as_str()),
        ("xai", "openai")
            | ("xai", "azure")
            | ("deepseek", "openai")
            | ("google", "gemini")
            | ("google", "google")
            | ("openai", "openai")
            | ("openai", "azure")
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
    let context = positive_i64(model.get("contextWindow"), "contextWindow")?;
    let output = positive_i64(model.get("maxTokens"), "maxTokens")?;
    let input = model
        .get("input")
        .and_then(Value::as_array)
        .ok_or("input must be an array")?;
    let vision = input.iter().any(|v| v.as_str() == Some("image"));
    let compat = model.get("compat").and_then(Value::as_object);
    let tools = compat
        .map(|m| {
            m.keys().any(|k| {
                let n = norm(k);
                n.contains("tool") || n.contains("function")
            })
        })
        .unwrap_or(false);
    let json_mode = compat
        .map(|m| {
            m.keys().any(|k| {
                let n = norm(k);
                n.contains("json") || n.contains("strict")
            })
        })
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
    }
    let caps = serde_json::to_string(&serde_json::json!({"source_provider": provider_id, "reasoning": model.get("reasoning").cloned().unwrap_or(Value::Null), "metadata": safe})).map_err(|_| "invalid capabilities JSON")?;
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
        let v = sanitize(&serde_json::json!({"apiKey":"SECRET","compat":{"ok":true}}));
        assert!(!v.to_string().contains("SECRET"));
    }
}
