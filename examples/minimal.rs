use clap::{Parser, Subcommand};
use rust_cli_release::{ReleaseSpec, update_current};

const RELEASE: ReleaseSpec = ReleaseSpec::new(
    "example-cli",
    "Example CLI",
    "owner/example-cli",
    "EXAMPLE_CLI",
    env!("CARGO_PKG_VERSION"),
);

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Update {
        #[arg(long, default_value = "latest")]
        version: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Update { version } => {
            let summary = update_current(&RELEASE, &version, false)?;
            println!("{}", serde_json::to_string(&summary)?);
        }
    }
    Ok(())
}
