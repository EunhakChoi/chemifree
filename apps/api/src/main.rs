use std::error::Error;
use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use chemifree_api::{ApiState, DEFAULT_WEB_ORIGINS, WebDir};
use chemifree_host::{
    InchiArgs, LocalStores, ServerLock, ServerLockError, local_application, with_selected_providers,
};
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
    /// How many computations (chem.aromaticity_convert, chem.cip) may run at once, 1 to 64.
    /// Defaults to the machine's cores, at most 4.
    // 1 to 64 is the range `Application::with_compute_concurrency` keeps as given. It clamps
    // any value outside that range, so the parser refuses those values instead.
    #[arg(
        long,
        env = "CHEMIFREE_COMPUTE_CONCURRENCY",
        value_name = "N",
        value_parser = clap::value_parser!(u8).range(1..=64)
    )]
    compute_concurrency: Option<u8>,
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

/// The exit status when `--port` is already in use. Other startup and serving failures exit with
/// 1, a command-line usage error with 2, and a data directory held by another server with
/// `EXIT_DATA_DIR_IN_USE`.
const EXIT_PORT_IN_USE: i32 = 3;

/// The exit status when another live server holds the data directory's lock. The message names
/// that server's pid and address when its record states them.
const EXIT_DATA_DIR_IN_USE: i32 = 4;

/// Runs the server. A failure is printed to standard error as `Error: {error}`, using the error's
/// `Display` text. `main` returns an `ExitCode` rather than a `Result`, because the runtime would
/// then print the `Debug` form, which quotes and escapes the message.
#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    // Checked before anything is created or pruned, so a bad web directory changes nothing.
    let web_dir = args.web_dir.as_deref().map(WebDir::open).transpose()?;
    let data_dir = args
        .data_dir
        .or_else(default_data_dir)
        .ok_or("no user data directory found; pass --data-dir")?;
    // The lock comes before pruning, so a refused second server changes nothing, and before the
    // bind, so a second default start names the holder instead of reporting a port conflict.
    let server_lock = match ServerLock::acquire(&data_dir) {
        Ok(server_lock) => Some(server_lock),
        // The data directory refuses writes and holds no lock file, so the server starts without
        // a lock. The warning is the one line that says so.
        Err(ServerLockError::NotCreated { path, error }) => {
            eprintln!(
                "Server lock not taken: {} cannot be created ({error}); the data directory is \
                 read-only, so another server may use it at the same time.",
                path.display()
            );
            None
        }
        Err(error @ ServerLockError::Held { .. }) => {
            eprintln!(
                "Chemifree API cannot start: {error}. Stop that server or pass another --data-dir."
            );
            // `process::exit` skips destructors. This process holds no lock and has started no
            // draft writer, so there is nothing that needs them.
            std::process::exit(EXIT_DATA_DIR_IN_USE);
        }
        Err(error @ ServerLockError::Io { .. }) => return Err(error.into()),
    };
    let stores = LocalStores::new(&data_dir);
    stores.prune_stale_temporaries();
    let documents = stores.documents_directory().to_path_buf();
    let application = with_selected_providers(
        local_application(stores),
        args.model_data_root.as_deref(),
        args.model_trusted_registry.as_deref(),
        args.inchi,
    );
    // The builder runs only when the flag is given, so the machine default stays otherwise.
    let application = match args.compute_concurrency {
        Some(computations) => application.with_compute_concurrency(usize::from(computations)),
        None => application,
    };

    let mut token = [0_u8; 32];
    getrandom::fill(&mut token).map_err(|error| format!("no system randomness: {error}"))?;
    let token: String = token.iter().map(|byte| format!("{byte:02x}")).collect();

    let origins = allowed_origins(args.allow_origins, web_dir.is_some());
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, args.port)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            eprintln!("{}", port_in_use_message(args.port));
            // `process::exit` skips destructors. That is safe here because no draft writer or
            // server has started yet.
            std::process::exit(EXIT_PORT_IN_USE);
        }
        Err(error) => return Err(error.into()),
    };
    let address = listener.local_addr()?;
    // A refused second start names this server from the record, which is written before the
    // listening line is printed. The record is informational, so a failure to write it is reported
    // and the server still starts. A server that holds no lock has no record to write.
    if let Some(server_lock) = &server_lock
        && let Err(error) = server_lock.record_url(&format!("http://{address}"))
    {
        eprintln!(
            "Could not record this server in {}: {error}",
            server_lock.path().display()
        );
    }
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
    // Held until the final drafts are written, so a second server cannot start during that write.
    drop(server_lock);
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

/// The one line printed when `port` is already in use on the loopback address.
fn port_in_use_message(port: u16) -> String {
    format!(
        "Chemifree API cannot start: port {port} on 127.0.0.1 is already in use. Stop the program \
         using it, or pass another port with --port <n> (--port 0 picks a free one)."
    )
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

    #[test]
    fn the_port_in_use_message_names_the_port_and_the_way_out() {
        assert_eq!(
            port_in_use_message(4321),
            "Chemifree API cannot start: port 4321 on 127.0.0.1 is already in use. Stop the program using it, or pass another port with --port <n> (--port 0 picks a free one)."
        );
    }
}
