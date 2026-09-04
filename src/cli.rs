//! CLI management tool for llm_proxy (v2.0).
//!
//! Extended with client-key CRUD, quota management, and live stream command.

#[cfg(feature = "cli")]
mod cli_impl {
    use clap::{Parser, Subcommand};
    use serde::Deserialize;

    #[derive(Parser)]
    #[command(
        name = "llm_proxy_cli",
        about = "LLM Proxy Gateway Management CLI v2.0"
    )]
    struct Cli {
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        base_url: String,

        #[arg(short, long, default_value = "")]
        admin_key: String,

        #[command(subcommand)]
        command: Commands,
    }

    #[derive(Subcommand)]
    enum Commands {
        Status,
        Keys,
        Metrics,
        Reload,
        Alerts,
        #[command(name = "client-keys")]
        ClientKeys {
            #[command(subcommand)]
            action: ClientKeyAction,
        },
        Quotas,
        Live,
        Export {
            #[arg(short, long, default_value = "jsonl")]
            format: String,
            #[arg(short = 'H', long, default_value = "24")]
            hours: u32,
        },
        ImportModels {
            #[arg(long, default_value = "models-store.json")]
            source: String,
            #[arg(long)]
            db: String,
            #[arg(long, default_value_t = false)]
            overwrite: bool,
        },
    }

    #[derive(Subcommand)]
    enum ClientKeyAction {
        List,
        Add {
            key: String,
            #[arg(short, long, default_value = "default")]
            tenant: String,
            #[arg(short, long, default_value = "")]
            label: String,
        },
        Update {
            key_hash: String,
            #[arg(short = 'e', long)]
            enabled: Option<bool>,
            #[arg(short, long)]
            tenant: Option<String>,
            #[arg(short, long)]
            label: Option<String>,
        },
        Rotate {
            key_hash: String,
            #[arg(short, long)]
            new_key: String,
        },
        Delete {
            key_hash: String,
        },
    }

    #[derive(Deserialize)]
    struct StatusResp {
        uptime_secs: u64,
        requests_total: u64,
        requests_failed: u64,
        stream_requests: u64,
        cache_hits: u64,
        active_connections: u64,
        prompt_tokens: u64,
        completion_tokens: u64,
        retries: u64,
        key_demotions: u64,
        upstream_5xx: u64,
        upstream_4xx: u64,
        pools: Vec<PoolResp>,
        alert_count: usize,
    }

    #[derive(Deserialize)]
    struct PoolResp {
        pool_id: String,
        total_keys: usize,
        healthy_keys: usize,
        bad_keys: usize,
    }

    #[derive(Deserialize)]
    struct KeysResp {
        pools: Vec<PoolKeysResp>,
    }

    #[derive(Deserialize)]
    struct PoolKeysResp {
        pool_id: String,
        keys: Vec<KeyResp>,
    }

    #[derive(Deserialize)]
    struct KeyResp {
        key_hash: String,
        weight: u32,
        healthy: bool,
        success_rate: String,
    }

    #[derive(Deserialize)]
    struct ClientKeyRecord {
        key_hash: String,
        tenant_id: String,
        #[allow(dead_code)]
        created_at: i64,
        enabled: bool,
        label: String,
    }

    #[derive(Deserialize)]
    struct ClientKeyList {
        keys: Vec<ClientKeyRecord>,
        total: usize,
    }

    #[derive(Deserialize)]
    struct QuotaSnapshot {
        tenant_id: String,
        daily_tokens_used: u64,
        daily_tokens_limit: Option<u64>,
        monthly_requests_used: u64,
        monthly_requests_limit: Option<u64>,
    }

    fn redact_url(url: &str) -> String {
        let Ok(mut parsed) = reqwest::Url::parse(url) else {
            return "[invalid URL]".to_owned();
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            return "[invalid URL]".to_owned();
        }
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_query(None);
        parsed.set_fragment(None);
        parsed.to_string()
    }

    fn request_error(url: &str, error: reqwest::Error) -> String {
        let target = redact_url(url);
        format!(
            "request to {target} failed: {}. Ensure the gateway is running at that address or specify --base-url <gateway-url>.",
            error.without_url()
        )
    }

    fn do_get(url: &str) -> Result<String, String> {
        let client = reqwest::blocking::Client::new();
        let resp = client.get(url).send().map_err(|e| request_error(url, e))?;
        if resp.status().is_success() {
            resp.text().map_err(|e| e.to_string())
        } else {
            Err(format!("HTTP {}", resp.status()))
        }
    }

    fn do_post(url: &str, body: &str) -> Result<String, String> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.to_owned())
            .send()
            .map_err(|e| request_error(url, e))?;
        if resp.status().is_success() {
            resp.text().map_err(|e| e.to_string())
        } else {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            Err(format!("HTTP {status}: {text}"))
        }
    }

    fn do_patch(url: &str, body: &str) -> Result<String, String> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .patch(url)
            .header("Content-Type", "application/json")
            .body(body.to_owned())
            .send()
            .map_err(|e| request_error(url, e))?;
        if resp.status().is_success() {
            resp.text().map_err(|e| e.to_string())
        } else {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            Err(format!("HTTP {status}: {text}"))
        }
    }

    fn do_delete(url: &str) -> Result<String, String> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .delete(url)
            .send()
            .map_err(|e| request_error(url, e))?;
        if resp.status().is_success() {
            resp.text().map_err(|e| e.to_string())
        } else {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            Err(format!("HTTP {status}: {text}"))
        }
    }

    pub fn run() -> Result<(), String> {
        let cli = Cli::parse();

        match cli.command {
            Commands::Status => {
                let url = format!("{}/admin/api/status", cli.base_url);
                let body = do_get(&url)?;
                let s: StatusResp =
                    serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
                let hrs = s.uptime_secs / 3600;
                let min = (s.uptime_secs % 3600) / 60;
                println!("LLM Proxy v2.0 Status — uptime {hrs}h {min}m");
                println!("────────────────────────────────────");
                println!("Requests total:      {}", format_num(s.requests_total));
                println!("Failed:              {}", format_num(s.requests_failed));
                println!("Stream:              {}", format_num(s.stream_requests));
                println!("Cache hits:          {}", format_num(s.cache_hits));
                println!("Active connections:  {}", s.active_connections);
                println!("Prompt tokens:       {}", format_num(s.prompt_tokens));
                println!("Completion tokens:   {}", format_num(s.completion_tokens));
                println!("Retries:             {}", format_num(s.retries));
                println!("Key demotions:       {}", format_num(s.key_demotions));
                println!("Upstream 5xx:        {}", format_num(s.upstream_5xx));
                println!("Upstream 4xx:        {}", format_num(s.upstream_4xx));
                println!("Alert events:        {}", s.alert_count);
                println!();
                println!("Key Pools:");
                for p in &s.pools {
                    println!(
                        "  {:<20}  total={}  healthy={}  bad={}",
                        p.pool_id, p.total_keys, p.healthy_keys, p.bad_keys
                    );
                }
            }

            Commands::Keys => {
                let url = format!("{}/admin/api/keys", cli.base_url);
                let body = do_get(&url)?;
                let r: KeysResp = serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
                println!("Key Health");
                println!("──────────");
                for p in &r.pools {
                    println!("Pool: {}", p.pool_id);
                    for k in &p.keys {
                        let status = if k.healthy { "OK" } else { "BAD" };
                        println!(
                            "  {:>12}  w={:<4}  {:<5}  success_rate={}",
                            k.key_hash, k.weight, status, k.success_rate
                        );
                    }
                    println!();
                }
            }

            Commands::Metrics => {
                let url = format!("{}/metrics", cli.base_url);
                let body = do_get(&url)?;
                println!("{body}");
            }

            Commands::Reload => {
                let url = format!("{}/admin/api/config/refresh", cli.base_url);
                let body = do_post(&url, "{}")?;
                println!("{body}");
            }

            Commands::Alerts => {
                let url = format!("{}/admin/alerts?format=json", cli.base_url);
                let body = do_get(&url)?;
                let v: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
                let events = v
                    .get("events")
                    .and_then(|e| e.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                println!("Recent alert events: {events}");
                if let Some(arr) = v.get("events").and_then(|e| e.as_array()) {
                    for ev in arr.iter().take(20) {
                        let event_type =
                            ev.get("event_type").and_then(|e| e.as_str()).unwrap_or("?");
                        let msg = ev.get("msg").and_then(|m| m.as_str()).unwrap_or("-");
                        println!("  [{event_type}] {msg}");
                    }
                }
            }

            Commands::ClientKeys { action } => match action {
                ClientKeyAction::List => {
                    let url = format!("{}/admin/api/client-keys", cli.base_url);
                    let body = do_get(&url)?;
                    let r: ClientKeyList =
                        serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
                    println!("Client Keys ({})", r.total);
                    println!("─────────────────────────");
                    for k in &r.keys {
                        let status = if k.enabled { "enabled" } else { "disabled" };
                        println!(
                            "  {:>12}  tenant={:<10}  {}  label={:?}",
                            k.key_hash, k.tenant_id, status, k.label
                        );
                    }
                }
                ClientKeyAction::Add { key, tenant, label } => {
                    let url = format!("{}/admin/api/client-keys", cli.base_url);
                    let body = serde_json::json!({
                        "key": key,
                        "tenant_id": tenant,
                        "label": label,
                    })
                    .to_string();
                    let resp = do_post(&url, &body)?;
                    let k: ClientKeyRecord =
                        serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
                    println!("Added key: hash={}", k.key_hash);
                }
                ClientKeyAction::Update {
                    key_hash,
                    enabled,
                    tenant,
                    label,
                } => {
                    let url = format!("{}/admin/api/client-keys/{key_hash}", cli.base_url);
                    let mut obj = serde_json::json!({});
                    if let Some(e) = enabled {
                        obj["enabled"] = serde_json::json!(e);
                    }
                    if let Some(t) = tenant {
                        obj["tenant_id"] = serde_json::json!(t);
                    }
                    if let Some(l) = label {
                        obj["label"] = serde_json::json!(l);
                    }
                    let resp = do_patch(&url, &obj.to_string())?;
                    let k: ClientKeyRecord =
                        serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
                    println!("Updated key: hash={}  enabled={}", k.key_hash, k.enabled);
                }
                ClientKeyAction::Rotate { key_hash, new_key } => {
                    let url = format!("{}/admin/api/client-keys/{key_hash}", cli.base_url);
                    let body = serde_json::json!({"new_key": new_key}).to_string();
                    let resp = do_post(&url, &body)?;
                    let k: ClientKeyRecord =
                        serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
                    println!("Rotated key: hash={}", k.key_hash);
                }
                ClientKeyAction::Delete { key_hash } => {
                    let url = format!("{}/admin/api/client-keys/{key_hash}", cli.base_url);
                    let resp = do_delete(&url)?;
                    let k: ClientKeyRecord =
                        serde_json::from_str(&resp).map_err(|e| format!("parse: {e}"))?;
                    println!("Deleted key: hash={}", k.key_hash);
                }
            },

            Commands::Quotas => {
                let url = format!("{}/admin/api/quotas", cli.base_url);
                let body = do_get(&url)?;
                let quotas: Vec<QuotaSnapshot> =
                    serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
                println!("Tenant Quotas");
                println!("─────────────");
                for q in &quotas {
                    println!("Tenant: {}", q.tenant_id);
                    println!(
                        "  Daily tokens:   {} / {:?}",
                        format_num(q.daily_tokens_used),
                        q.daily_tokens_limit.map(format_num),
                    );
                    println!(
                        "  Monthly reqs:   {} / {:?}",
                        format_num(q.monthly_requests_used),
                        q.monthly_requests_limit.map(format_num),
                    );
                    println!();
                }
            }

            Commands::Live => {
                println!("Connect to:  {}/admin/live", cli.base_url);
                println!("WebSocket:   ws://127.0.0.1:8080/admin/live");
                println!("Open in browser for the live waterfall UI.");
            }

            Commands::Export { format, hours } => {
                let url = format!(
                    "{}/admin/export?format={format}&hours={hours}",
                    cli.base_url
                );
                println!("{}", do_get(&url)?);
            }

            Commands::ImportModels {
                source,
                db,
                overwrite,
            } => {
                let runtime =
                    tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {e}"))?;
                let report = runtime.block_on(async {
                    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{db}"))
                        .await
                        .map_err(|e| format!("database: {e}"))?;
                    llm_proxy::model_import::import_file(&pool, source, overwrite)
                        .await
                        .map_err(|e| e.to_string())
                })?;
                println!(
                    "new={} skipped={} conflicts={} failed={}",
                    report.counts.new,
                    report.counts.skipped,
                    report.counts.conflicts,
                    report.counts.failed
                );
                for reason in report.reasons {
                    println!("{reason}");
                }
            }
        }
        Ok(())
    }

    fn format_num(n: u64) -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        } else if n >= 1_000 {
            format!("{:.1}K", n as f64 / 1_000.0)
        } else {
            n.to_string()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // ── format_num unit tests ──────────────────────────────────────

        #[test]
        fn format_num_returns_string_for_small_numbers() {
            assert_eq!(format_num(0), "0");
            assert_eq!(format_num(1), "1");
            assert_eq!(format_num(999), "999");
        }

        #[test]
        fn format_num_returns_k_for_thousands() {
            assert_eq!(format_num(1_000), "1.0K");
            assert_eq!(format_num(1_500), "1.5K");
            assert_eq!(format_num(5_000), "5.0K");
            assert_eq!(format_num(999_999), "1000.0K");
        }

        #[test]
        fn format_num_returns_m_for_millions() {
            assert_eq!(format_num(1_000_000), "1.0M");
            assert_eq!(format_num(1_500_000), "1.5M");
            assert_eq!(format_num(25_000_000), "25.0M");
        }

        // ── Deserialization tests ───────────────────────────────────────

        #[test]
        fn it_deserializes_status_response() {
            let json = r#"{
                "uptime_secs": 3600,
                "requests_total": 1000,
                "requests_failed": 5,
                "stream_requests": 300,
                "cache_hits": 200,
                "active_connections": 4,
                "prompt_tokens": 50000,
                "completion_tokens": 25000,
                "retries": 3,
                "key_demotions": 1,
                "upstream_5xx": 2,
                "upstream_4xx": 10,
                "pools": [
                    {"pool_id": "main", "total_keys": 3, "healthy_keys": 2, "bad_keys": 1}
                ],
                "alert_count": 0
            }"#;
            let s: StatusResp = serde_json::from_str(json).unwrap();
            assert_eq!(s.uptime_secs, 3600);
            assert_eq!(s.requests_total, 1000);
            assert_eq!(s.cache_hits, 200);
            assert_eq!(s.pools.len(), 1);
            assert_eq!(s.pools[0].pool_id, "main");
            assert_eq!(s.pools[0].healthy_keys, 2);
            assert_eq!(s.pools[0].bad_keys, 1);
        }

        #[test]
        fn it_deserializes_empty_pools_in_status() {
            let json = r#"{"uptime_secs":0,"requests_total":0,"requests_failed":0,"stream_requests":0,"cache_hits":0,"active_connections":0,"prompt_tokens":0,"completion_tokens":0,"retries":0,"key_demotions":0,"upstream_5xx":0,"upstream_4xx":0,"pools":[],"alert_count":0}"#;
            let s: StatusResp = serde_json::from_str(json).unwrap();
            assert!(s.pools.is_empty());
            assert_eq!(s.alert_count, 0);
        }

        #[test]
        fn it_deserializes_keys_response() {
            let json = r#"{
                "pools": [
                    {
                        "pool_id": "pool-a",
                        "keys": [
                            {"key_hash": "abc123def456", "weight": 5, "healthy": true, "success_rate": "98.5%"},
                            {"key_hash": "789abc012def", "weight": 1, "healthy": false, "success_rate": "0.0%"}
                        ]
                    }
                ]
            }"#;
            let r: KeysResp = serde_json::from_str(json).unwrap();
            assert_eq!(r.pools.len(), 1);
            assert_eq!(r.pools[0].pool_id, "pool-a");
            assert_eq!(r.pools[0].keys.len(), 2);
            assert_eq!(r.pools[0].keys[0].key_hash, "abc123def456");
            assert_eq!(r.pools[0].keys[0].weight, 5);
            assert!(r.pools[0].keys[0].healthy);
            assert!(!r.pools[0].keys[1].healthy);
        }

        #[test]
        fn it_deserializes_client_key_record() {
            let json = r#"{"key_hash":"abcd1234abcd","tenant_id":"org-x","created_at":1700000000000,"enabled":true,"label":"prod-key"}"#;
            let k: ClientKeyRecord = serde_json::from_str(json).unwrap();
            assert_eq!(k.key_hash, "abcd1234abcd");
            assert_eq!(k.tenant_id, "org-x");
            assert_eq!(k.created_at, 1700000000000);
            assert!(k.enabled);
            assert_eq!(k.label, "prod-key");
        }

        #[test]
        fn it_deserializes_disabled_client_key() {
            let json = r#"{"key_hash":"dead","tenant_id":"default","created_at":0,"enabled":false,"label":""}"#;
            let k: ClientKeyRecord = serde_json::from_str(json).unwrap();
            assert!(!k.enabled);
            assert_eq!(k.label, "");
        }

        #[test]
        fn it_deserializes_client_key_list() {
            let json = r#"{"keys":[{"key_hash":"k1","tenant_id":"t1","created_at":1,"enabled":true,"label":"l1"},{"key_hash":"k2","tenant_id":"t2","created_at":2,"enabled":false,"label":"l2"}],"total":2}"#;
            let r: ClientKeyList = serde_json::from_str(json).unwrap();
            assert_eq!(r.total, 2);
            assert_eq!(r.keys.len(), 2);
        }

        #[test]
        fn it_deserializes_quota_snapshot() {
            let json = r#"{"tenant_id":"tenant-1","daily_tokens_used":50000,"daily_tokens_limit":100000,"monthly_requests_used":1000,"monthly_requests_limit":5000}"#;
            let q: QuotaSnapshot = serde_json::from_str(json).unwrap();
            assert_eq!(q.tenant_id, "tenant-1");
            assert_eq!(q.daily_tokens_used, 50000);
            assert_eq!(q.daily_tokens_limit, Some(100000));
            assert_eq!(q.monthly_requests_used, 1000);
            assert_eq!(q.monthly_requests_limit, Some(5000));
        }

        #[test]
        fn it_deserializes_quota_without_limits() {
            let json = r#"{"tenant_id":"unlimited","daily_tokens_used":0,"daily_tokens_limit":null,"monthly_requests_used":0,"monthly_requests_limit":null}"#;
            let q: QuotaSnapshot = serde_json::from_str(json).unwrap();
            assert_eq!(q.daily_tokens_limit, None);
            assert_eq!(q.monthly_requests_limit, None);
        }

        // ── HTTP helper tests (mockito) ─────────────────────────────────

        #[test]
        fn do_get_returns_body_on_success() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("GET", "/admin/api/status")
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"ok":true}"#)
                .create();

            let url = format!("{}/admin/api/status", server.url());
            let result = do_get(&url);
            mock.assert();
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), r#"{"ok":true}"#);
        }

        #[test]
        fn do_get_returns_error_on_500() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("GET", "/admin/api/keys")
                .with_status(500)
                .with_body("Internal Server Error")
                .create();

            let url = format!("{}/admin/api/keys", server.url());
            let result = do_get(&url);
            mock.assert();
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("HTTP 500"));
        }

        #[test]
        fn do_post_sends_json_and_returns_body() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("POST", "/admin/api/client-keys")
                .match_header("content-type", "application/json")
                .match_body(r#"{"key":"sk-test","label":"test-label","tenant_id":"default"}"#)
                .with_status(200)
                .with_body(r#"{"key_hash":"hash123","tenant_id":"default","created_at":1,"enabled":true,"label":"test-label"}"#)
                .create();

            let url = format!("{}/admin/api/client-keys", server.url());
            let body = r#"{"key":"sk-test","label":"test-label","tenant_id":"default"}"#;
            let result = do_post(&url, body);
            mock.assert();
            assert!(result.is_ok());
            let parsed: ClientKeyRecord = serde_json::from_str(&result.unwrap()).unwrap();
            assert_eq!(parsed.key_hash, "hash123");
            assert_eq!(parsed.tenant_id, "default");
            assert!(parsed.enabled);
        }

        #[test]
        fn do_post_returns_error_on_400() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("POST", "/admin/api/client-keys")
                .with_status(400)
                .with_body(r#"{"error":"bad request"}"#)
                .create();

            let url = format!("{}/admin/api/client-keys", server.url());
            let result = do_post(&url, "{}");
            mock.assert();
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                err.contains("HTTP 400"),
                "expected 'HTTP 400' in error: {err}"
            );
            assert!(err.contains("bad request"), "expected body in error: {err}");
        }

        #[test]
        fn do_patch_sends_partial_update() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("PATCH", "/admin/api/client-keys/abc")
                .match_header("content-type", "application/json")
                .match_body(r#"{"enabled":false}"#)
                .with_status(200)
                .with_body(r#"{"key_hash":"abc","tenant_id":"t","created_at":1,"enabled":false,"label":""}"#)
                .create();

            let url = format!("{}/admin/api/client-keys/abc", server.url());
            let result = do_patch(&url, r#"{"enabled":false}"#);
            mock.assert();
            let rec: ClientKeyRecord = serde_json::from_str(&result.unwrap()).unwrap();
            assert_eq!(rec.key_hash, "abc");
            assert!(!rec.enabled);
        }

        #[test]
        fn do_delete_returns_body_on_success() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("DELETE", "/admin/api/client-keys/xyz")
                .with_status(200)
                .with_body(r#"{"key_hash":"xyz","tenant_id":"t","created_at":1,"enabled":true,"label":"l"}"#)
                .create();

            let url = format!("{}/admin/api/client-keys/xyz", server.url());
            let result = do_delete(&url);
            mock.assert();
            let rec: ClientKeyRecord = serde_json::from_str(&result.unwrap()).unwrap();
            assert_eq!(rec.key_hash, "xyz");
        }

        #[test]
        fn do_delete_returns_error_on_404() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("DELETE", "/admin/api/client-keys/nonexistent")
                .with_status(404)
                .with_body("not found")
                .create();

            let url = format!("{}/admin/api/client-keys/nonexistent", server.url());
            let result = do_delete(&url);
            mock.assert();
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("HTTP 404"));
        }

        // ── Connection error test (unreachable host) ─────────────────────

        #[test]
        fn do_get_returns_actionable_error_on_connection_refused() {
            // Port 1 is reserved and should refuse connections.
            let url = "http://127.0.0.1:1/nonexistent";
            let error = do_get(url).unwrap_err();
            assert!(error.contains(url));
            assert!(error.contains("--base-url <gateway-url>"));
        }

        #[test]
        fn export_get_uses_configured_base_url_and_query_parameters() {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("GET", "/admin/export")
                .match_query(mockito::Matcher::AllOf(vec![
                    mockito::Matcher::UrlEncoded("format".into(), "jsonl".into()),
                    mockito::Matcher::UrlEncoded("hours".into(), "24".into()),
                ]))
                .with_status(200)
                .with_body(r#"{"id":"request-1"}"#)
                .create();

            let url = format!("{}/admin/export?format=jsonl&hours=24", server.url());
            assert_eq!(do_get(&url).unwrap(), r#"{"id":"request-1"}"#);
            mock.assert();
        }

        #[test]
        fn export_accepts_hours_long_option_without_conflicting_with_help() {
            let cli = Cli::try_parse_from(["llm_proxy_cli", "export", "--hours", "12"]).unwrap();
            assert!(matches!(
                cli.command,
                Commands::Export { hours: 12, format } if format == "jsonl"
            ));
        }

        #[test]
        fn request_error_redacts_url_credentials() {
            let error = reqwest::blocking::get("http://127.0.0.1:1").unwrap_err();
            let message = request_error("http://client-key@127.0.0.1:1/admin/export", error);
            assert!(!message.contains("client-key"));
            assert!(message.contains("127.0.0.1:1/admin/export"));
        }

        #[test]
        fn redact_url_removes_credentials_from_malformed_url() {
            let url = "http://user:topsecret[REDACTED]@127.0.0.1:1/admin/export";
            let redacted = redact_url(url);
            assert!(!redacted.contains("user"));
            assert!(!redacted.contains("topsecret"));
            assert_eq!(redacted, "http://127.0.0.1:1/admin/export");
        }

        #[test]
        fn redact_url_hides_userinfo_in_url_without_scheme() {
            let url = "user:secret[REDACTED]@127.0.0.1:1/admin/export?api_key=secret[REDACTED]";
            let redacted = redact_url(url);
            assert!(!redacted.contains("user"));
            assert!(!redacted.contains("secret"));
            assert_eq!(redacted, "[invalid URL]");
        }

        #[test]
        fn redact_url_removes_query_parameters_from_valid_url() {
            let redacted = redact_url("http://127.0.0.1:1/admin/export?api_key=secret[REDACTED]");
            assert!(!redacted.contains("secret"));
            assert_eq!(redacted, "http://127.0.0.1:1/admin/export");
        }
    }
}

fn main() {
    #[cfg(feature = "cli")]
    if let Err(e) = cli_impl::run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    #[cfg(not(feature = "cli"))]
    {
        println!("llm_proxy_cli requires --features cli");
    }
}
