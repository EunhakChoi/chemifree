use std::error::Error;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use chemifree_api::{ApiState, DEFAULT_WEB_ORIGINS, LocalApplication};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, default_data_dir};
use clap::Parser;
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
    /// Loopback web origin allowed to use the API (repeatable). Defaults to the local
    /// development and preview origins on ports 5173 and 4173.
    #[arg(long = "allow-origin", value_name = "ORIGIN")]
    allow_origins: Vec<String>,
    /// Print one line per request (time, method, path, status, duration) to standard error.
    #[arg(long, env = "CHEMIFREE_LOG_REQUESTS")]
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
    let application = LocalApplication::new(LocalCapabilities, repository);

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
    let mut router = chemifree_api::router(Arc::new(state));
    if args.log_requests {
        router = chemifree_api::log_requests(router);
    }
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
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
