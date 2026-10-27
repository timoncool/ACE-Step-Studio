//! Standalone entry point for the studio service.
//!
//! The desktop application embeds `music_server::serve` directly; this binary
//! exists for development and for headless use.

fn main() -> anyhow::Result<()> {
    if !music_engine::process_group::bind_children_to_this_process() {
        music_server::studio_log!("this process could not create its own job object; the engine is stopped by the supervisor only");
    }
    music_server::apply_saved_gpu();
    let served = tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(music_server::serve());
    music_engine::process_group::terminate_group_members();
    served
}
