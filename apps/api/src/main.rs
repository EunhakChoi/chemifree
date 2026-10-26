use std::{error::Error, net::Ipv4Addr};

use clap::Parser;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(version, about = "Chemifree local read-only API (scaffold)")]
struct Args {
    /// Listen on this port. The address is always IPv4 loopback.
    #[arg(long, default_value_t = 3001)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, args.port)).await?;
    eprintln!(
        "Chemifree API listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, chemifree_api::router())
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
