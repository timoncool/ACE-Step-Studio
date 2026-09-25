//! What belongs to ACE-Step 1.5 rather than to the engine family.

/// A part of the model an adapter can change, with its own strength.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct AdapterSlot {
    /// The engine's name for the part, as `/props` adapter_info flags it.
    pub id: &'static str,
    /// What changing that part does to a song, for the interface to name.
    pub role: &'static str,
}

/// ACE-Step's planner LM writes the song - metadata, lyrics, the audio codes
/// that fix its structure - and the DiT renders its sound. An adapter is
/// trained for one of them.
pub const ADAPTER_SLOTS: &[AdapterSlot] =
    &[AdapterSlot { id: "lm", role: "composition" }, AdapterSlot { id: "dit", role: "sound" }];
