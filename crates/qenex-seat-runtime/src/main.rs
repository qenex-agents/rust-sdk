//! The QENEX seat runtime.
//!
//! One process serves exactly one seat. It exposes that seat's execution
//! surface over MCP (stdio transport) so a bot's agent can run code, read and
//! write files inside the seat workspace without ever reaching another seat
//! or the host's global state.
//!
//! The seat is bound at startup from the environment:
//!
//! * `QENEX_SEAT` - the seat identifier, for example `coder`.
//! * `QENEX_SEAT_WORKSPACE` - the absolute path of the seat working tree.
//! * `QENEX_SEAT_API` - optional base URL of the seat runtime service.
//! * `QENEX_SEAT_ALLOW_GLOBAL=1` - operator escape hatch that disables the
//!   write confinement.
//!
//! The runtime refuses to start without a seat and a workspace. It never reads
//! or logs `QENEX_SEAT_TOKEN`.

mod paths;
mod seat;
mod tools;

use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::{self, EnvFilter};

use crate::seat::SeatBinding;
use crate::tools::SeatRuntime;

#[tokio::main]
async fn main() -> Result<()> {
    // Logs go to stderr; stdout carries the MCP protocol messages.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let binding = SeatBinding::from_env()?;
    tracing::info!(
        seat = %binding.seat(),
        workspace = %binding.workspace().display(),
        "qenex seat runtime starting"
    );

    let runtime = SeatRuntime::new(binding);
    let service = runtime.serve(stdio()).await.inspect_err(|error| {
        tracing::error!(error = ?error, "seat runtime failed to serve");
    })?;

    // Wait until the client closes the session.
    service.waiting().await?;
    Ok(())
}
