use clap::{Parser, Subcommand, ValueEnum};

use crate::AppError;
use crate::backend::CaptureBackend;
use crate::backend::pipewire::PipeWireBackend;

#[derive(Debug, Parser)]
#[command(name = "midijitter", about = "Measure MIDI clock jitter")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List available MIDI source ports.
    Devices {
        #[arg(long, value_enum, default_value_t = Backend::Pipewire)]
        backend: Backend,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Pipewire,
}

pub fn run() -> Result<(), AppError> {
    match Cli::parse().command {
        Command::Devices {
            backend: Backend::Pipewire,
        } => {
            let sources = PipeWireBackend.enumerate()?;
            if sources.is_empty() {
                println!("No MIDI source ports found.");
            } else {
                for source in sources {
                    println!(
                        "{}\t{}\tnode-id={} port-id={}",
                        source.display_name,
                        source.stable_identity(),
                        source.node_id,
                        source.port_id
                    );
                }
            }
            Ok(())
        }
    }
}
