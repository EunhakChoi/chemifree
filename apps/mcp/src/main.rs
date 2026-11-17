//! The `chemifree-mcp` binary: the read-only MCP server, over stdio, for a running Chemifree API
//! server. Stdout carries the protocol, so diagnostics go to stderr.

use std::path::PathBuf;
use std::process::ExitCode;

use chemifree_mcp::client::ApiClient;
use chemifree_mcp::discovery::{Discovery, ServerUrl};
use chemifree_mcp::tools::ChemifreeServer;
use chemifree_providers::default_data_dir;
use clap::Parser;
use rmcp::ServiceExt;

/// Where to find the running API server.
#[derive(Debug, Parser)]
#[command(
    name = "chemifree-mcp",
    version,
    about = "Read-only MCP server for a running Chemifree API server"
)]
struct Args {
    /// The server's address, `http://127.0.0.1:<port>`. Without it, the address comes from the
    /// record that the running server keeps in its data directory.
    #[arg(long, env = "CHEMIFREE_SERVER_URL")]
    server_url: Option<ServerUrl>,
    /// The data directory where the running server records its address. Defaults to the platform
    /// data directory.
    #[arg(long, env = "CHEMIFREE_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let discovery = Discovery::new(args.server_url, args.data_dir.or_else(default_data_dir));
    let server = ChemifreeServer::new(ApiClient::new(discovery));
    let running = match server.serve(rmcp::transport::stdio()).await {
        Ok(running) => running,
        Err(error) => {
            eprintln!("chemifree-mcp: the MCP session could not start: {error}");
            return ExitCode::FAILURE;
        }
    };
    match running.waiting().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("chemifree-mcp: the MCP session ended with an error: {error}");
            ExitCode::FAILURE
        }
    }
}
