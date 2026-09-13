//! CLI management tool for llm_proxy (v2.0).
//!
//! Extended with client-key CRUD, OAuth login, and live stream command.

#[cfg(feature = "cli")]
mod cli_impl {
    use clap::{Parser, Subcommand};
    use serde::Deserialize;

    #[derive(Parser, Debug)]
    #[command(
        name = "llm_proxy_cli",
        version,
        about = "Manage a running llm_proxy gateway (status, keys, OAuth, export).",
        long_about = "Talks to the admin HTTP API of a running llm_proxy process.\n\n\
Admin routes are IP-whitelisted; this CLI must run from an allowed address.\n\
Gateway URL is inferred from config.yaml (server.host:port) unless you pass --base-url.",
        after_help = "Examples:\n  \
    llm_proxy_cli status\n  \
    llm_proxy_cli --base-url http://127.0.0.1:4000 keys\n  \
    llm_proxy_cli oauth login xai --pool grok_pool --apply\n  \
    llm_proxy_cli oauth import-pi --pool grok_pool --apply\n  \
    llm_proxy_cli export --format jsonl --hours 24\n  \
    llm_proxy_cli import-models --source models-store.json --db ./data/llm_proxy.db\n  \
    llm_proxy_cli client-keys add sk-example --tenant default --label ci\n",
        arg_required_else_help = true,
        subcommand_required = true,
        flatten_help = true
    )]
    struct Cli {
        /// Gateway base URL (scheme://host:port, no trailing path).
        /// Default: LLM_PROXY_BASE_URL, else server.host:port in --config/config.yaml,
        /// else http://127.0.0.1:8080.
        #[arg(short, long, value_name = "URL", default_value_t = default_cli_base_url())]
        base_url: String,

        /// Path to config.yaml (same as LLM_PROXY_CONFIG). Used to infer --base-url
        /// and the SQLite path for oauth --apply fallback / import-models.
        #[arg(short = 'c', long, value_name = "PATH")]
        config: Option<String>,

        /// Optional admin bearer token sent as `Authorization: Bearer ...`.
        /// The stock server authenticates admin by IP allowlist; leave empty unless
        /// you put a reverse proxy in front that expects this header.
        #[arg(
            short = 'k',
            long,
            value_name = "TOKEN",
            default_value = "",
            hide_default_value = true
        )]
        admin_key: String,

        #[command(subcommand)]
        command: Commands,
    }

    #[derive(Subcommand, Debug)]
    enum Commands {
        /// Print uptime, request counters, and per-pool key health.
        Status,
        /// List upstream key hashes, weights, and success rates per pool.
        Keys,
        /// Dump Prometheus metrics from GET /metrics.
        Metrics,
        /// Hot-reload managed config from SQLite into the running process.
        Reload,
        /// Show recent alert events (JSON admin feed).
        Alerts,
        /// CRUD for tenant client API keys (the keys callers send to this gateway).
        #[command(name = "client-keys", alias = "ck")]
        ClientKeys {
            #[command(subcommand)]
            action: ClientKeyAction,
        },
        /// Print the live waterfall dashboard URL (open in a browser).
        Live,
        /// Export recent request logs from the gateway.
        Export {
            /// Output format accepted by GET /admin/export.
            #[arg(short, long, value_name = "FORMAT", default_value = "jsonl", value_parser = ["jsonl", "json", "parquet"])]
            format: String,
            /// How many hours of logs to include.
            #[arg(short = 'H', long, value_name = "HOURS", default_value = "24")]
            hours: u32,
        },
        /// Import model metadata from a JSON file into SQLite (does not need the gateway).
        #[command(name = "import-models")]
        ImportModels {
            /// Path to models-store.json (or equivalent).
            #[arg(long, value_name = "FILE", default_value = "models-store.json")]
            source: String,
            /// SQLite database path. Default: db.path in config.yaml.
            #[arg(long, value_name = "FILE", default_value_t = default_db_path())]
            db: String,
            /// Overwrite existing model rows instead of skipping them.
            #[arg(long, default_value_t = false)]
            overwrite: bool,
        },
        /// Device-code / token import for subscription OAuth (xAI SuperGrok).
        Oauth {
            #[command(subcommand)]
            action: OauthAction,
        },
    }

    #[derive(Subcommand, Debug)]
    enum OauthAction {
        /// Device-code login (currently issuer `xai` = SuperGrok / X Premium).
        /// Prints a YAML key fragment; with --apply also POSTs it to the gateway
        /// (or writes local SQLite if the gateway is down).
        Login {
            /// OAuth issuer. Only `xai` is implemented.
            #[arg(value_name = "ISSUER", default_value = "xai")]
            issuer: String,
            /// Target key pool id (required with --apply).
            #[arg(long, value_name = "POOL_ID")]
            pool: Option<String>,
            /// Apply the credential to --pool on the running gateway.
            #[arg(long, default_value_t = false, requires = "pool")]
            apply: bool,
            /// Key weight inside the pool (weighted-random routing).
            #[arg(long, value_name = "N", default_value_t = 1)]
            weight: u32,
        },
        /// Import xAI OAuth tokens from ~/.pi/agent/auth.json (pi `/login xai`).
        #[command(name = "import-pi")]
        ImportPi {
            /// Target key pool id (required with --apply).
            #[arg(long, value_name = "POOL_ID")]
            pool: Option<String>,
            /// Apply the credential to --pool on the running gateway.
            #[arg(long, default_value_t = false, requires = "pool")]
            apply: bool,
            /// Key weight inside the pool (weighted-random routing).
            #[arg(long, value_name = "N", default_value_t = 1)]
            weight: u32,
        },
    }

    #[derive(Subcommand, Debug)]
    enum ClientKeyAction {
        /// List client keys (hash, tenant, enabled, label). Never prints plaintext.
        List,
        /// Create a client key.
        Add {
            /// Plaintext client key (stored hashed + plaintext-at-rest in SQLite).
            #[arg(value_name = "KEY")]
            key: String,
            /// Tenant id this key belongs to.
            #[arg(short, long, value_name = "TENANT", default_value = "default")]
            tenant: String,
            /// Optional human label for the dashboard.
            #[arg(short, long, value_name = "LABEL", default_value = "")]
            label: String,
        },
        /// Patch enabled/tenant/label for an existing client key.
        Update {
            /// First 12 hex chars of SHA-256 (as shown by `client-keys list`).
            #[arg(value_name = "KEY_HASH")]
            key_hash: String,
            /// Enable or disable the key (`true` / `false`).
            #[arg(short = 'e', long, value_name = "BOOL")]
            enabled: Option<bool>,
            /// Move the key to another tenant.
            #[arg(short, long, value_name = "TENANT")]
            tenant: Option<String>,
            /// Replace the dashboard label.
            #[arg(short, long, value_name = "LABEL")]
            label: Option<String>,
        },
        /// Replace the plaintext of an existing client key (hash will change).
        Rotate {
            /// Current key hash from `client-keys list`.
            #[arg(value_name = "KEY_HASH")]
            key_hash: String,
            /// New plaintext client key.
            #[arg(short, long, value_name = "KEY")]
            new_key: String,
        },
        /// Delete a client key by hash.
        Delete {
            #[arg(value_name = "KEY_HASH")]
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
        let resp = attach_auth(client.get(url))
            .send()
            .map_err(|e| request_error(url, e))?;
        if resp.status().is_success() {
            resp.text().map_err(|e| e.to_string())
        } else {
            Err(format!("HTTP {}", resp.status()))
        }
    }

    fn do_post(url: &str, body: &str) -> Result<String, String> {
        let client = reqwest::blocking::Client::new();
        let resp = attach_auth(client.post(url))
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
        let resp = attach_auth(client.patch(url))
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
        let resp = attach_auth(client.delete(url))
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

    static ADMIN_KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    static CONFIG_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    fn attach_auth(
        builder: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        match ADMIN_KEY.get().map(|s| s.as_str()).unwrap_or("") {
            "" => builder,
            key => builder.header("Authorization", format!("Bearer {key}")),
        }
    }

    pub fn run() -> Result<(), String> {
        let cli = Cli::parse();
        if let Some(ref path) = cli.config {
            let _ = CONFIG_PATH.set(path.clone());
        }
        if !cli.admin_key.is_empty() {
            let _ = ADMIN_KEY.set(cli.admin_key.clone());
        }

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

            Commands::Live => {
                println!("Connect to:  {}/admin/live", cli.base_url);
                println!("WebSocket:   {}/admin/live", http_to_ws(&cli.base_url));
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
            Commands::Oauth { action } => match action {
                OauthAction::Login {
                    issuer,
                    pool,
                    apply,
                    weight,
                } => {
                    if issuer != "xai" {
                        return Err(format!("unsupported oauth issuer '{issuer}'"));
                    }
                    let runtime =
                        tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {e}"))?;
                    let entry = runtime.block_on(async {
                        let http = reqwest::Client::new();
                        let endpoints = llm_proxy::credential::XaiOAuthEndpoints::default();
                        llm_proxy::credential::xai::login(&http, &endpoints, |prompt| {
                            println!("xAI device login");
                            println!("----------------");
                            println!("1. Open this URL in a browser:");
                            println!("   {}", prompt.verification_uri);
                            println!("2. Confirm this code: {}", prompt.user_code);
                            println!(
                                "The code expires in {} seconds. Waiting for authorization...",
                                prompt.expires_in_seconds
                            );
                        })
                        .await
                        .map_err(|e| e.to_string())
                    })?;
                    let mut entry = entry;
                    entry.weight = weight;
                    print_oauth_yaml(&entry);
                    if apply {
                        let pool =
                            pool.ok_or_else(|| "--apply requires --pool <pool_id>".to_string())?;
                        apply_oauth_key(&cli.base_url, &pool, &entry)?;
                    }
                }
                OauthAction::ImportPi {
                    pool,
                    apply,
                    weight,
                } => {
                    let mut entry = import_pi_xai()?;
                    entry.weight = weight;
                    print_oauth_yaml(&entry);
                    if apply {
                        let pool =
                            pool.ok_or_else(|| "--apply requires --pool <pool_id>".to_string())?;
                        apply_oauth_key(&cli.base_url, &pool, &entry)?;
                    }
                }
            },
        }
        Ok(())
    }

    fn print_oauth_yaml(entry: &llm_proxy::config::KeyEntry) {
        println!("Login succeeded.");
        println!("YAML fragment for config.yaml (pools.<pool_id>.keys):");
        println!("      - type: oauth");
        println!(
            "        issuer: {}",
            entry.issuer.as_deref().unwrap_or("xai")
        );
        println!("        key: {:?}", entry.key);
        println!(
            "        refresh: {:?}",
            entry.refresh.as_deref().unwrap_or_default()
        );
        if let Some(expires) = entry.expires {
            println!("        expires: {expires}");
        }
        println!("        weight: {}", entry.weight);
    }

    fn apply_oauth_key(
        base_url: &str,
        pool: &str,
        entry: &llm_proxy::config::KeyEntry,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "key": entry.key,
            "weight": entry.weight,
            "type": entry.cred_type_str(),
            "refresh": entry.refresh,
            "expires": entry.expires,
            "issuer": entry.issuer,
        })
        .to_string();
        for url_base in candidate_base_urls(base_url) {
            let url = format!("{url_base}/admin/api/pools/{pool}/keys");
            match do_post(&url, &body) {
                Ok(resp) => {
                    println!("Applied to pool {pool} via {url_base}: {resp}");
                    return Ok(());
                }
                Err(e) if e.contains("HTTP 404") => {
                    let create_url = format!("{url_base}/admin/api/pools");
                    let create_body = serde_json::json!({
                        "id": pool,
                        "strategy": "weighted_random",
                        "keys": [serde_json::from_str::<serde_json::Value>(&body).unwrap_or(serde_json::Value::Null)],
                    })
                    .to_string();
                    match do_post(&create_url, &create_body) {
                        Ok(resp) => {
                            println!("Created pool {pool} via {url_base}: {resp}");
                            return Ok(());
                        }
                        Err(create_err) => {
                            println!("gateway {url_base} rejected pool create: {create_err}");
                        }
                    }
                }
                Err(e) => {
                    println!("gateway {url_base} unreachable: {e}");
                }
            }
        }
        persist_oauth_local(pool, entry)?;
        println!(
            "Wrote oauth credential to local sqlite. Restart the gateway to load it, or pass --base-url http://127.0.0.1:<port>"
        );
        Ok(())
    }

    fn candidate_base_urls(cli_base: &str) -> Vec<String> {
        let mut out = Vec::new();
        let push = |list: &mut Vec<String>, url: String| {
            if !list.iter().any(|u| u == &url) {
                list.push(url);
            }
        };
        push(&mut out, cli_base.trim_end_matches('/').to_owned());
        if let Some(from_cfg) = base_url_from_config() {
            push(&mut out, from_cfg);
        }
        out
    }

    fn config_path() -> std::path::PathBuf {
        if let Some(path) = CONFIG_PATH.get() {
            return std::path::PathBuf::from(path);
        }
        std::env::var("LLM_PROXY_CONFIG")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("config.yaml"))
    }

    fn base_url_from_config() -> Option<String> {
        let raw = std::fs::read_to_string(config_path()).ok()?;
        let v: serde_yaml::Value = serde_yaml::from_str(&raw).ok()?;
        let server = v.get("server")?;
        let host = server
            .get("host")
            .and_then(|h| h.as_str())
            .unwrap_or("127.0.0.1");
        let host = if host == "0.0.0.0" { "127.0.0.1" } else { host };
        let port = server.get("port").and_then(|p| p.as_u64())?;
        Some(format!("http://{host}:{port}"))
    }

    fn default_cli_base_url() -> String {
        std::env::var("LLM_PROXY_BASE_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_owned())
            .filter(|s| !s.is_empty())
            .or_else(base_url_from_config)
            .unwrap_or_else(|| "http://127.0.0.1:8080".into())
    }

    fn default_db_path() -> String {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|raw| serde_yaml::from_str::<serde_yaml::Value>(&raw).ok())
            .and_then(|doc| {
                doc.get("db")
                    .and_then(|d| d.get("path"))
                    .and_then(|p| p.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "./data/llm_proxy.db".into())
    }

    fn http_to_ws(url: &str) -> String {
        if let Some(rest) = url.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = url.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            url.to_owned()
        }
    }

    fn persist_oauth_local(
        pool_id: &str,
        entry: &llm_proxy::config::KeyEntry,
    ) -> Result<(), String> {
        let path = config_path();
        let raw =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut doc: serde_yaml::Value =
            serde_yaml::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;

        let key_obj = serde_yaml::Mapping::from_iter([
            (
                serde_yaml::Value::String("type".into()),
                serde_yaml::Value::String(entry.cred_type_str().into()),
            ),
            (
                serde_yaml::Value::String("issuer".into()),
                serde_yaml::Value::String(entry.issuer.clone().unwrap_or_else(|| "xai".into())),
            ),
            (
                serde_yaml::Value::String("key".into()),
                serde_yaml::Value::String(entry.key.clone()),
            ),
            (
                serde_yaml::Value::String("refresh".into()),
                serde_yaml::Value::String(entry.refresh.clone().unwrap_or_default()),
            ),
            (
                serde_yaml::Value::String("expires".into()),
                serde_yaml::Value::Number(entry.expires.unwrap_or(0).into()),
            ),
            (
                serde_yaml::Value::String("weight".into()),
                serde_yaml::Value::Number(entry.weight.into()),
            ),
        ]);
        let mut pool_map = serde_yaml::Mapping::new();
        pool_map.insert(
            serde_yaml::Value::String("keys".into()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(key_obj)]),
        );
        pool_map.insert(
            serde_yaml::Value::String("strategy".into()),
            serde_yaml::Value::String("weighted_random".into()),
        );
        let pools = doc
            .as_mapping_mut()
            .ok_or_else(|| "config.yaml root must be a mapping".to_string())?
            .entry(serde_yaml::Value::String("pools".into()))
            .or_insert(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        let pools = pools
            .as_mapping_mut()
            .ok_or_else(|| "pools must be a mapping".to_string())?;
        pools.insert(
            serde_yaml::Value::String(pool_id.to_owned()),
            serde_yaml::Value::Mapping(pool_map),
        );

        let providers = doc
            .as_mapping_mut()
            .ok_or_else(|| "config.yaml root must be a mapping".to_string())?
            .entry(serde_yaml::Value::String("providers".into()))
            .or_insert(serde_yaml::Value::Sequence(Vec::new()));
        if let Some(seq) = providers.as_sequence_mut() {
            let exists = seq.iter().any(|p| {
                p.get("pool_id").and_then(|v| v.as_str()) == Some(pool_id)
                    || p.get("id").and_then(|v| v.as_str()) == Some("xai")
            });
            if !exists {
                let mut provider = serde_yaml::Mapping::new();
                provider.insert(
                    serde_yaml::Value::String("id".into()),
                    serde_yaml::Value::String("xai".into()),
                );
                provider.insert(
                    serde_yaml::Value::String("kind".into()),
                    serde_yaml::Value::String("openai".into()),
                );
                provider.insert(
                    serde_yaml::Value::String("base_url".into()),
                    serde_yaml::Value::String("https://api.x.ai/v1".into()),
                );
                provider.insert(
                    serde_yaml::Value::String("pool_id".into()),
                    serde_yaml::Value::String(pool_id.to_owned()),
                );
                seq.push(serde_yaml::Value::Mapping(provider));
            }
        }

        let routing = doc
            .as_mapping_mut()
            .ok_or_else(|| "config.yaml root must be a mapping".to_string())?
            .entry(serde_yaml::Value::String("model_to_pool".into()))
            .or_insert(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        if let Some(map) = routing.as_mapping_mut() {
            map.entry(serde_yaml::Value::String("grok-4.6".into()))
                .or_insert(serde_yaml::Value::String(pool_id.to_owned()));
        }

        let db_path = doc
            .get("db")
            .and_then(|d| d.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or("./data/llm_proxy.db")
            .to_owned();
        let pool_id = pool_id.to_owned();
        let entry = entry.clone();
        let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {e}"))?;
        runtime.block_on(async move {
            let db = llm_proxy::db::connect(&db_path)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("INSERT OR IGNORE INTO key_pool (id, strategy, enabled) VALUES (?1, 'weighted_random', 1)")
                .bind(&pool_id)
                .execute(&db)
                .await
                .map_err(|e| format!("insert pool: {e}"))?;
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM key_entry WHERE pool_id = ?1 AND key_hash = ?2",
            )
            .bind(&pool_id)
            .bind(entry.identity_hash())
            .fetch_optional(&db)
            .await
            .map_err(|e| format!("lookup key: {e}"))?;
            if existing.is_none() {
                let mut tx = db.begin().await.map_err(|e| format!("begin: {e}"))?;
                llm_proxy::config_store::insert_key_entry(tx.as_mut(), &pool_id, &entry)
                    .await
                    .map_err(|e| e.to_string())?;
                tx.commit().await.map_err(|e| format!("commit: {e}"))?;
            }
            sqlx::query(
                "INSERT OR IGNORE INTO provider_config (id, kind, base_url, pool_id, enabled, metadata) \
                 VALUES ('xai', 'openai', 'https://api.x.ai/v1', ?1, 1, '{}')",
            )
            .bind(&pool_id)
            .execute(&db)
            .await
            .map_err(|e| format!("insert provider: {e}"))?;
            let routed: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM routing_config WHERE logical_model = 'grok-4.6' AND enabled = 1",
            )
            .fetch_optional(&db)
            .await
            .map_err(|e| format!("lookup routing: {e}"))?;
            if routed.is_none() {
                sqlx::query(
                    "INSERT INTO routing_config (logical_model, pool_id, enabled) VALUES ('grok-4.6', ?1, 1)",
                )
                .bind(&pool_id)
                .execute(&db)
                .await
                .map_err(|e| format!("insert routing: {e}"))?;
            } else {
                sqlx::query(
                    "UPDATE routing_config SET pool_id = ?1 WHERE logical_model = 'grok-4.6'",
                )
                .bind(&pool_id)
                .execute(&db)
                .await
                .map_err(|e| format!("update routing: {e}"))?;
            }
            println!("Updated sqlite {db_path}");
            Ok::<(), String>(())
        })?;
        Ok(())
    }

    fn import_pi_xai() -> Result<llm_proxy::config::KeyEntry, String> {
        let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        let path = std::path::PathBuf::from(home).join(".pi/agent/auth.json");
        let raw =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse auth.json: {e}"))?;
        let xai = v
            .get("xai")
            .ok_or_else(|| "no xai entry in auth.json".to_string())?;
        let cred_type = xai.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if cred_type != "oauth" {
            return Err("xai entry in auth.json is not oauth".into());
        }
        let access = xai
            .get("access")
            .and_then(|t| t.as_str())
            .ok_or_else(|| "xai.access missing".to_string())?;
        let refresh = xai
            .get("refresh")
            .and_then(|t| t.as_str())
            .ok_or_else(|| "xai.refresh missing".to_string())?;
        let expires = xai.get("expires").and_then(|t| t.as_i64());
        Ok(llm_proxy::config::KeyEntry::oauth(
            access, refresh, "xai", 1, expires,
        ))
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
        fn oauth_apply_requires_pool() {
            let err =
                Cli::try_parse_from(["llm_proxy_cli", "oauth", "login", "--apply"]).unwrap_err();
            let text = err.to_string();
            assert!(text.contains("pool") || text.contains("--pool"), "{text}");
        }

        #[test]
        fn root_help_documents_commands_and_examples() {
            let err = Cli::try_parse_from(["llm_proxy_cli", "--help"]).unwrap_err();
            let text = err.to_string();
            assert!(text.contains("oauth"), "{text}");
            assert!(text.contains("--base-url"), "{text}");
            assert!(text.contains("--config"), "{text}");
            assert!(text.contains("Examples:"), "{text}");
            assert!(text.contains("client-keys"), "{text}");
        }

        #[test]
        fn export_rejects_unknown_format() {
            let err =
                Cli::try_parse_from(["llm_proxy_cli", "export", "--format", "csv"]).unwrap_err();
            assert!(err.to_string().contains("jsonl"), "{}", err);
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

        #[test]
        fn http_to_ws_converts_schemes() {
            assert_eq!(http_to_ws("http://127.0.0.1:4000"), "ws://127.0.0.1:4000");
            assert_eq!(http_to_ws("https://example.com"), "wss://example.com");
        }

        #[test]
        fn cli_accepts_custom_base_url() {
            let cli = Cli::try_parse_from([
                "llm_proxy_cli",
                "--base-url",
                "http://127.0.0.1:4000",
                "status",
            ])
            .unwrap();
            assert_eq!(cli.base_url, "http://127.0.0.1:4000");
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
