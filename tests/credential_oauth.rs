//! OAuth credential refresh against a mockito token endpoint.

use llm_proxy::config::KeyEntry;
use llm_proxy::credential::{CredentialRuntime, XaiOAuthEndpoints};

#[tokio::test]
async fn refreshes_expired_xai_token() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/oauth2/token")
        .match_body(mockito::Matcher::Regex("grant_type=refresh_token".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600}"#,
        )
        .create_async()
        .await;

    let runtime = CredentialRuntime::with_xai(
        None,
        XaiOAuthEndpoints {
            token_url: format!("{}/oauth2/token", server.url()),
            device_code_url: format!("{}/oauth2/device/code", server.url()),
            client_id: "test-client".into(),
        },
    );
    let stale = KeyEntry::oauth("old-access", "old-refresh", "xai", 1, Some(1));
    let fresh = runtime.ensure_fresh(&stale).await.unwrap();
    assert_eq!(fresh.key, "new-access");
    assert_eq!(fresh.refresh.as_deref(), Some("new-refresh"));
    assert_eq!(fresh.identity_hash(), stale.identity_hash());
}

#[tokio::test]
async fn skips_refresh_when_access_token_is_still_valid() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/oauth2/token")
        .with_status(500)
        .expect(0)
        .create_async()
        .await;

    let runtime = CredentialRuntime::with_xai(
        None,
        XaiOAuthEndpoints {
            token_url: format!("{}/oauth2/token", server.url()),
            device_code_url: format!("{}/oauth2/device/code", server.url()),
            client_id: "test-client".into(),
        },
    );
    let future = chrono_like_future();
    let entry = KeyEntry::oauth("live-access", "refresh", "xai", 1, Some(future));
    let fresh = runtime.ensure_fresh(&entry).await.unwrap();
    assert_eq!(fresh.key, "live-access");
    mock.assert_async().await;
}

fn chrono_like_future() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + 3_600_000
}
