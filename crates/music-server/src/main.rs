//! Standalone entry point for the studio service.
//!
//! The desktop application embeds `music_server::serve` directly; this binary
//! exists for development and for headless use.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    music_server::serve().await
}
