#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use studio_server::persist::PersistBackend;
use studio_server::secret::ProviderKind;
use studio_server::{EmbeddedServer, ServeArgs, serve_embedded};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tracing_subscriber::EnvFilter;

struct DesktopState {
    _server: Mutex<Option<EmbeddedServer>>,
}

fn main() {
    init_tracing();

    let result = tauri::Builder::default()
        .setup(setup_desktop)
        .run(tauri::generate_context!());

    if let Err(e) = result {
        tracing::error!(error = %e, "Cobrust Studio desktop app exited with error");
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn setup_desktop(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = desktop_project_root()?;
    let args = desktop_serve_args(project_root);
    let server = tauri::async_runtime::block_on(serve_embedded(&args))
        .map_err(|e| std::io::Error::other(format!("failed to start embedded server: {e}")))?;
    let base_url = server.base_url().parse()?;

    app.manage(DesktopState {
        _server: Mutex::new(Some(server)),
    });

    WebviewWindowBuilder::new(app, "main", WebviewUrl::External(base_url))
        .title("Cobrust Studio")
        .inner_size(1280.0, 840.0)
        .min_inner_size(960.0, 640.0)
        .build()?;

    Ok(())
}

fn desktop_project_root() -> Result<PathBuf, std::io::Error> {
    match std::env::var_os("COBRUST_STUDIO_PROJECT") {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => std::env::current_dir(),
    }
}

fn desktop_serve_args(project: PathBuf) -> ServeArgs {
    ServeArgs {
        project,
        port: 0,
        host: "127.0.0.1".to_string(),
        dev_api_key: std::env::var("COBRUST_DEV_API_KEY").ok(),
        dev_endpoint: std::env::var("COBRUST_DEV_ENDPOINT")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
        dev_model: std::env::var("COBRUST_DEV_MODEL")
            .unwrap_or_else(|_| "claude-opus-4-7".to_string()),
        debug_session: false,
        enable_write_tools: std::env::var_os("COBRUST_ENABLE_WRITE_TOOLS").is_some(),
        dev_provider_kind: desktop_provider_kind(),
        persist_session: desktop_persist_backend(),
        persist_session_file: std::env::var_os("COBRUST_PERSIST_SESSION_FILE").map(PathBuf::from),
    }
}

fn desktop_provider_kind() -> ProviderKind {
    match std::env::var("COBRUST_DEV_PROVIDER_KIND")
        .unwrap_or_else(|_| "anthropic".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "openai" => ProviderKind::Openai,
        "synthetic" => ProviderKind::Synthetic,
        _ => ProviderKind::Anthropic,
    }
}

fn desktop_persist_backend() -> PersistBackend {
    match std::env::var("COBRUST_PERSIST_SESSION")
        .unwrap_or_else(|_| "keychain".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "none" => PersistBackend::None,
        "file" => PersistBackend::File,
        _ => PersistBackend::Keychain,
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
