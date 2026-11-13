use std::error::Error;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use chemifree_api::{ApiState, DEFAULT_WEB_ORIGINS, WebDir};
use chemifree_host::{InchiArgs, LocalStores, local_application, with_selected_providers};
use chemifree_providers::default_data_dir;
use clap::Parser;
use clap::builder::FalseyValueParser;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(version, about = "Chemifree local API (loopback only)")]
struct Args {
    /// Listen on this port. The address is always IPv4 loopback.
    #[arg(long, default_value_t = 3001)]
    port: u16,
    /// Directory for saved documents. Defaults to the platform user-data directory.
    #[arg(long, env = "CHEMIFREE_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Built web app to serve from this origin beside the API. The directory must hold an
    /// `index.html`. Without it, the API serves no web files.
    #[arg(long, env = "CHEMIFREE_WEB_DIR")]
    web_dir: Option<PathBuf>,
    /// Optional native InChI registration; absent or rejected registration leaves the host usable.
    #[command(flatten)]
    inchi: InchiArgs,
    /// Separately selected directory containing authorized local DATA.
    #[arg(long, env = "CHEMIFREE_MODEL_DATA_ROOT")]
    model_data_root: Option<PathBuf>,
    /// Explicitly trusted registration descriptor, separate from the DATA directory.
    #[arg(long, env = "CHEMIFREE_MODEL_TRUSTED_REGISTRY")]
    model_trusted_registry: Option<PathBuf>,
    /// Loopback web origin allowed to use the API (repeatable). Defaults to the local
    /// development and preview origins on ports 5173 and 4173, or to none when `--web-dir`
    /// serves the web app from this origin.
    #[arg(long = "allow-origin", value_name = "ORIGIN")]
    allow_origins: Vec<String>,
    /// Log one line per request to standard error (method, path, status, duration), for
    /// diagnosing test runs. Headers and bodies are never logged.
    /// The variable accepts the usual on/off spellings (`1`, `true`, `0`, `false`, …).
    #[arg(long, env = "CHEMIFREE_LOG_REQUESTS", value_parser = FalseyValueParser::new())]
    log_requests: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    // Checked before anything is created or pruned, so a bad web directory changes nothing.
    let web_dir = args.web_dir.as_deref().map(WebDir::open).transpose()?;
    let data_dir = args
        .data_dir
        .or_else(default_data_dir)
        .ok_or("no user data directory found; pass --data-dir")?;
    let stores = LocalStores::new(&data_dir);
    stores.prune_stale_temporaries();
    let documents = stores.documents_directory().to_path_buf();
    let application = with_selected_providers(
        local_application(stores),
        args.model_data_root.as_deref(),
        args.model_trusted_registry.as_deref(),
        args.inchi,
    );

    let mut token = [0_u8; 32];
    getrandom::fill(&mut token).map_err(|error| format!("no system randomness: {error}"))?;
    let token: String = token.iter().map(|byte| format!("{byte:02x}")).collect();

    let origins = allowed_origins(args.allow_origins, web_dir.is_some());
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, args.port)).await?;
    let address = listener.local_addr()?;
    let state = ApiState::new(application, token, address.port(), &origins)?;
    let state = match web_dir {
        Some(web_dir) => state.with_web_dir(web_dir),
        None => state,
    };

    eprintln!("Chemifree API listening on http://{address}");
    if let Some(web_dir) = state.web_dir() {
        eprintln!("Web app: {}", web_dir.path().display());
    }
    eprintln!("Documents: {}", documents.display());
    let state = Arc::new(state);
    let drafts = chemifree_api::DraftWriter::start(Arc::clone(&state));
    let stopping = Arc::clone(&state);
    let mut router = chemifree_api::router(state);
    if args.log_requests {
        router = chemifree_api::log_requests(router);
    }
    // Cancel computations as the stop begins, so graceful shutdown does not wait for them.
    let served = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            stopping.cancel_computations();
        })
        .await;
    // Unsaved work survives a stop in its recovery draft, also when serving ended in an error.
    for (id, error) in drafts.finish() {
        eprintln!("Could not draft unsaved changes to {id} before stopping: {error}");
    }
    served?;
    Ok(())
}

/// The web origins the API accepts besides its own. `--allow-origin` replaces the defaults
/// entirely. Without it, an API that serves its own web app accepts none, since that app is
/// same-origin, and any other API accepts the development and preview origins.
fn allowed_origins(allow_origins: Vec<String>, serves_web: bool) -> Vec<String> {
    if !allow_origins.is_empty() {
        allow_origins
    } else if serves_web {
        Vec::new()
    } else {
        DEFAULT_WEB_ORIGINS.map(String::from).to_vec()
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            () = ctrl_c => {},
            () = terminate => {},
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_origins_replace_the_defaults() {
        let given = vec!["http://localhost:8080".to_owned()];
        assert_eq!(
            allowed_origins(given.clone(), /* serves_web= */ false),
            ["http://localhost:8080"]
        );
        assert_eq!(
            allowed_origins(given, /* serves_web= */ true),
            ["http://localhost:8080"]
        );
    }

    #[test]
    fn a_server_with_its_own_web_app_accepts_no_other_origin() {
        assert!(allowed_origins(Vec::new(), /* serves_web= */ true).is_empty());
    }

    #[test]
    fn a_server_without_its_web_app_accepts_the_development_origins() {
        assert_eq!(
            allowed_origins(Vec::new(), /* serves_web= */ false),
            DEFAULT_WEB_ORIGINS
        );
    }
}
