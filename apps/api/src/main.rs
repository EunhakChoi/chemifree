use std::error::Error;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use chemifree_api::{ApiState, DEFAULT_WEB_ORIGINS, LocalApplication};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{
    FileDocumentRepository, FilePreferenceStore, LocalCapabilities, configure_selected_registry,
    default_data_dir,
};
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
    /// Separately selected directory containing authorized local DATA.
    #[arg(long, env = "CHEMIFREE_MODEL_DATA_ROOT")]
    model_data_root: Option<PathBuf>,
    /// Explicitly trusted registration descriptor, separate from the DATA directory.
    #[arg(long, env = "CHEMIFREE_MODEL_TRUSTED_REGISTRY")]
    model_trusted_registry: Option<PathBuf>,
    /// Loopback web origin allowed to use the API (repeatable). Defaults to the local
    /// development and preview origins on ports 5173 and 4173.
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
    let data_dir = args
        .data_dir
        .or_else(default_data_dir)
        .ok_or("no user data directory found; pass --data-dir")?;
    let repository = FileDocumentRepository::new(&data_dir);
    repository.prune_stale_temporaries();
    let documents = repository.directory().to_path_buf();
    let preferences = FilePreferenceStore::new(&data_dir);
    preferences.prune_stale_temporaries();
    let registry = configure_selected_registry(
        args.model_data_root.as_deref(),
        args.model_trusted_registry.as_deref(),
    );
    let application = LocalApplication::new(LocalCapabilities, repository)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_preferences(preferences)
        .with_data_registry(registry);

    let mut token = [0_u8; 32];
    getrandom::fill(&mut token).map_err(|error| format!("no system randomness: {error}"))?;
    let token: String = token.iter().map(|byte| format!("{byte:02x}")).collect();

    let origins = if args.allow_origins.is_empty() {
        DEFAULT_WEB_ORIGINS.map(String::from).to_vec()
    } else {
        args.allow_origins
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, args.port)).await?;
    let address = listener.local_addr()?;
    let state = ApiState::new(application, token, address.port(), &origins)?;

    eprintln!("Chemifree API listening on http://{address}");
    eprintln!("Documents: {}", documents.display());
    let state = Arc::new(state);
    let drafts = chemifree_api::DraftWriter::start(Arc::clone(&state));
    let mut router = chemifree_api::router(state);
    if args.log_requests {
        router = chemifree_api::log_requests(router);
    }
    let served = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    // Unsaved work survives a stop in its recovery draft, also when serving ended in an error.
    for (id, error) in drafts.finish() {
        eprintln!("Could not draft unsaved changes to {id} before stopping: {error}");
    }
    served?;
    Ok(())
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
