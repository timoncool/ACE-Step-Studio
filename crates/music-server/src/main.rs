//! Standalone entry point for the studio service.
//!
//! The desktop application embeds `music_server::serve` directly; this binary
//! exists for development and for headless use.

fn main() -> anyhow::Result<()> {
    music_server::apply_saved_gpu();
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(music_server::serve())
}
