#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::StatusCode;
use studio_server::cli::ServeArgs;
use studio_server::persist::PersistBackend;
use studio_server::secret::ProviderKind;
use studio_server::serve_embedded;

fn embedded_args(project: std::path::PathBuf) -> ServeArgs {
    ServeArgs {
        project,
        port: 0,
        host: "127.0.0.1".to_string(),
        dev_api_key: None,
        dev_endpoint: "https://api.anthropic.com".to_string(),
        dev_model: "claude-opus-4-7".to_string(),
        debug_session: false,
        dev_provider_kind: ProviderKind::Anthropic,
        persist_session: PersistBackend::None,
        persist_session_file: None,
    }
}

#[tokio::test]
async fn embedded_server_binds_ephemeral_loopback_and_serves_login() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let args = embedded_args(tmp.path().to_path_buf());

    let server = serve_embedded(&args).await.expect("embedded server");
    assert!(server.bound_addr().ip().is_loopback());
    assert_ne!(server.bound_addr().port(), 0);

    let resp = reqwest::get(format!("{}login", server.base_url()))
        .await
        .expect("GET /login");
    assert_eq!(resp.status(), StatusCode::OK);

    server.shutdown().await.expect("shutdown embedded server");
}
