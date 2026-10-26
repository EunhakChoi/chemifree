use std::error::Error;

use chemifree_application::Application;
use chemifree_providers::ScaffoldCapabilities;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chemifree", version, about = "Chemifree local CLI (scaffold)")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect the local application's liveness contract without starting HTTP.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// List actual implementation status using the same application service as HTTP.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let application = Application::new(ScaffoldCapabilities);

    match args.command {
        Command::Status { json } => {
            let health = application.health();
            if json {
                println!("{}", serde_json::to_string_pretty(&health)?);
            } else {
                println!(
                    "{} {} · {} · API {} · {}",
                    health.service, health.version, health.stage, health.api_version, health.status
                );
                println!("Liveness only. Run `chemifree capabilities` for implementation status.");
            }
        }
        Command::Capabilities { json } => {
            let response = application.capabilities();
            if json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                println!("Chemifree capabilities · API {}", response.api_version);
                for capability in response.capabilities {
                    println!("{}: {}", capability.id, capability.status);
                    println!("  {}", capability.description);
                }
            }
        }
    }
    Ok(())
}
