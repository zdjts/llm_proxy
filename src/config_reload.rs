//! SIGHUP-based config hot-reload.
//!
//! Listens for SIGHUP signals and reloads `config.yaml` without restarting.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

use crate::config::Config;

pub fn spawn_config_reloader(
    config_path: PathBuf,
    initial: Arc<Config>,
) -> (watch::Receiver<Arc<Config>>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = watch::channel(initial);

    let handle = tokio::spawn(async move {
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("SIGHUP not available: {e}");
                return;
            }
        };

        loop {
            sighup.recv().await;
            tracing::info!("received SIGHUP, reloading config...");

            match Config::load(&config_path) {
                Ok(new_config) => {
                    if new_config.validate().is_ok() {
                        tracing::info!("config reloaded successfully");
                        let _ = tx.send(Arc::new(new_config));
                    } else {
                        tracing::error!("config validation failed after reload");
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to reload config");
                }
            }
        }
    });

    (rx, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn watcher_returns_initial_config() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(
            tmp,
            r#"
server:
  host: "0.0.0.0"
  port: 9090
auth:
  client_keys:
    - {{ key: "sk-test" }}
db:
  path: ":memory:"
failover:
  enabled: false
pools:
  p1:
    keys:
      - {{ key: "sk-k", weight: 1 }}
providers:
  - id: test
    pool_id: p1
    base_url: "http://localhost/v1"
model_to_pool:
  "m": p1
"#
        )
        .unwrap();
        let config = Config::load(tmp.path()).unwrap();
        let (rx, _handle) = spawn_config_reloader(tmp.path().to_path_buf(), Arc::new(config));
        let initial = rx.borrow().clone();
        assert_eq!(initial.server.port, 9090);
    }
}
