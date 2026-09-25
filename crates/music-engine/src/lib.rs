//! The music engine: `server` supervises the engine process every studio runs
//! the same way, `model` and `train` hold what belongs to this studio's model.

pub mod model;
pub mod process_group;
pub mod server;
pub mod train;
