//! Why a song failed, as the window shows it.
//!
//! Each reason has a code and fields: the window translates it by the code
//! (`app/i18n/songErrors.ts`), while the English text stays for the log, the hub
//! and agents. The hub groups failures by that text, so it is word for word the
//! text of before. A reason without a kind of its own travels as `Raw` and is
//! shown as it is. The codes are the same in every music studio; a studio adds
//! codes for its own engine and never renames these.

use std::fmt;

use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum SongError {
    /// A reason with no kind of its own: shown as it is.
    Raw(String),
    TrainingOnCard,
    /// Lost with an engine that restarted: the card's memory ran out.
    LostVram,
    /// Lost with an engine that restarted: the computer's memory ran out.
    LostRam,
    /// Lost with an engine that restarted: the card failed, in the engine's own words when it gave any, and which
    /// card, compute capability, driver and build it was on a CUDA error.
    LostDevice { detail: Option<String>, card: Option<String> },
    /// Lost with an engine that restarted, whose last line says where it was.
    LostAtLine(String),
    Lost,
    /// Lost with an engine that ended on this exit code (`0xc0000005`), with its last line when it wrote one.
    LostExited { exit: String, line: Option<String> },
    /// Lost with an engine that could not listen on its port: `set` when STUDIO_ENGINE_PORT names it, else another
    /// program took the port the studio chose, which `note` says when it was not the engine's own.
    LostPort { port: u16, set: bool, note: Option<String> },
    /// Lost with an engine whose language model ran past the KV cache set in Settings, Engine.
    LostKvShort,
    /// The engine gave the song up for want of memory, in its own words.
    OutOfMemory(String),
    /// The engine gave the song up with this FATAL or ERROR line.
    EngineFatal(String),
    /// The engine's ERROR line: the DiT (`dit`) or the VAE wrote values that are not numbers.
    NotFinite { line: String, dit: bool, count: u64, total: u64 },
    /// The engine gave the song up and its log names no cause.
    EngineFailed,
    /// The request names a LoRA that is not installed.
    AdapterMissing(String),
    /// A LoRA whose layers none matched the chosen DiT.
    AdapterMisfit(String),
    /// A LoRA made for the other size of DiT than the one chosen.
    AdapterWrongSize { name: String, made_for: String, dit: String },
    /// What the engine returned is not a song: `problem` is one of `OUTPUT_PROBLEMS`.
    NotASong(&'static str),
    /// A source or reference recording longer than the engine takes.
    TrackTooLong { seconds: u64 },
    /// The request names a source or reference track the library does not have.
    TrackMissing { reference: bool, id: String },
    /// The request names a source or reference track that is in the library's bin.
    TrackInBin { reference: bool, title: String },
    /// The request names a model file that is gone from disk.
    ModelMissing(String),
    /// A task that works on a recording was asked for without one.
    SourceRequired(String),
    /// The language model read the song and gave it no length.
    NoLength,
}

/// The Windows status of a process that ran an instruction the processor does not have.
const ILLEGAL_INSTRUCTION: &str = "0xc000001d";

/// What a Windows exit status of the engine means, for the statuses a person can act on.
pub fn exit_meaning(exit: &str) -> Option<&'static str> {
    exit_entry(exit).map(|(_, _, words)| *words)
}

/// The exit statuses of the engine a person can act on: the statuses, the window's key for them
/// (`app/i18n/songErrors.ts` `EXIT_MEANINGS`) and the English words.
const EXIT_MEANINGS: &[(&[&str], &str, &str)] = &[
    (&["0xc0000005"], "access_violation", "a crash: an access violation"),
    (&[ILLEGAL_INSTRUCTION], "illegal_instruction", "an instruction this processor does not have"),
    (&["0xc0000409"], "aborted", "a crash: the engine aborted"),
    (&["0xc00000fd"], "stack_overflow", "a crash: a stack overflow"),
    (&["0xc0000374"], "corrupted_memory", "a crash: corrupted memory"),
    (&["0xc0000135", "0xc0000139", "0xc0000142"], "library", "a library of the engine did not load"),
    (&["0xc0000017", "0xc000012d"], "out_of_memory", "the computer ran out of memory"),
];

fn exit_entry(exit: &str) -> Option<&'static (&'static [&'static str], &'static str, &'static str)> {
    EXIT_MEANINGS.iter().find(|(statuses, _, _)| statuses.contains(&exit))
}

/// The window's key for what an exit status means; `none` for a status without one.
pub fn exit_meaning_key(exit: &str) -> &'static str {
    exit_entry(exit).map_or("none", |(_, key, _)| *key)
}

/// What `audio_pcm::output_problem` finds in the engine's output, with the code of each.
pub const OUTPUT_PROBLEMS: [(&str, &str); 3] =
    [("silence", "output_silence"), ("samples that are not numbers", "output_not_numbers"), ("one level held from start to end", "output_flat")];

const NOT_FINITE: (&str, &str) = (" are not finite (NaN or Inf)", " of ");

impl SongError {
    /// The reason the engine's FATAL or ERROR line gives, with its fields where
    /// the line is one the window explains in its own language.
    pub fn from_fatal(line: String) -> SongError {
        if let Some(not_finite) = not_finite(&line) {
            return not_finite;
        }
        SongError::EngineFatal(line)
    }

    /// What the engine's output was instead of a song, as `output_problem` names it.
    pub fn not_a_song(problem: &str) -> SongError {
        match OUTPUT_PROBLEMS.iter().find(|(said, _)| *said == problem) {
            Some((said, _)) => SongError::NotASong(said),
            None => SongError::Raw(format!("The engine returned {problem} instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause.")),
        }
    }

    /// The code the window finds the translation by; `None` for a reason shown as it is.
    pub fn code(&self) -> Option<&'static str> {
        Some(match self {
            SongError::Raw(_) => return None,
            SongError::TrainingOnCard => "training_on_card",
            SongError::LostVram => "lost_vram",
            SongError::LostRam => "lost_ram",
            SongError::LostDevice { detail: Some(_), .. } => "lost_device",
            SongError::LostDevice { detail: None, .. } => "lost_device_silent",
            SongError::LostAtLine(_) => "lost_at_line",
            SongError::Lost => "lost",
            SongError::LostExited { exit, .. } if exit == ILLEGAL_INSTRUCTION => "lost_illegal_instruction",
            SongError::LostExited { line: Some(_), .. } => "lost_exited_at_line",
            SongError::LostExited { .. } => "lost_exited",
            SongError::LostPort { set: true, .. } => "lost_port_set",
            SongError::LostPort { set: false, .. } => "lost_port_taken",
            SongError::LostKvShort => "lost_kv_short",
            SongError::OutOfMemory(_) => "out_of_memory",
            SongError::EngineFatal(_) => "engine_fatal",
            SongError::NotFinite { dit: true, .. } => "dit_not_finite",
            SongError::NotFinite { dit: false, .. } => "vae_not_finite",
            SongError::EngineFailed => "engine_failed",
            SongError::AdapterMissing(_) => "adapter_missing",
            SongError::AdapterMisfit(_) => "adapter_misfit",
            SongError::AdapterWrongSize { .. } => "adapter_wrong_size",
            SongError::NotASong(problem) => OUTPUT_PROBLEMS.iter().find(|(said, _)| said == problem).map_or("output_silence", |(_, code)| *code),
            SongError::TrackTooLong { .. } => "track_too_long",
            SongError::TrackMissing { reference: false, .. } => "source_missing",
            SongError::TrackMissing { reference: true, .. } => "reference_missing",
            SongError::TrackInBin { reference: false, .. } => "source_in_bin",
            SongError::TrackInBin { reference: true, .. } => "reference_in_bin",
            SongError::ModelMissing(_) => "model_missing",
            SongError::SourceRequired(_) => "source_required",
            SongError::NoLength => "no_length",
        })
    }

    /// The fields put into the translation: counts, names, and the engine's own words.
    pub fn params(&self) -> Map<String, Value> {
        let mut params = Map::new();
        let mut put = |key: &str, value: Value| {
            params.insert(key.to_string(), value);
        };
        match self {
            SongError::LostDevice { detail: Some(detail), card } => {
                put("detail", json!(match card {
                    Some(card) => format!("{detail}; {card}"),
                    None => detail.clone(),
                }));
            }
            SongError::OutOfMemory(detail) => put("detail", json!(detail)),
            SongError::LostAtLine(line) | SongError::EngineFatal(line) => put("line", json!(line)),
            SongError::LostExited { exit, line } => {
                put("exit", json!(exit));
                put("meaning", json!(exit_meaning_key(exit)));
                if let Some(line) = line {
                    put("line", json!(line));
                }
            }
            SongError::LostPort { port, .. } => put("port", json!(port)),
            SongError::NotFinite { count, total, .. } => {
                put("count", json!(count));
                put("total", json!(total));
            }
            SongError::AdapterMissing(name) | SongError::AdapterMisfit(name) | SongError::ModelMissing(name) => {
                put("name", json!(name))
            }
            SongError::AdapterWrongSize { name, made_for, dit } => {
                put("name", json!(name));
                put("made_for", json!(made_for));
                put("dit", json!(dit));
            }
            SongError::TrackTooLong { seconds } => put("length", json!(format!("{}:{:02}", seconds / 60, seconds % 60))),
            SongError::TrackMissing { id, .. } => put("id", json!(id)),
            SongError::TrackInBin { title, .. } => put("title", json!(title)),
            SongError::SourceRequired(task) => put("task", json!(task)),
            _ => {}
        }
        params
    }
}

fn not_finite(line: &str) -> Option<SongError> {
    let (end, of) = NOT_FINITE;
    let counted = line.strip_suffix(end)?;
    let (count, rest) = counted.split_once(of)?;
    let (total, what) = rest.split_once(' ')?;
    let dit = match what {
        "latent values" => true,
        "samples" => false,
        _ => return None,
    };
    Some(SongError::NotFinite { line: line.to_string(), dit, count: count.parse().ok()?, total: total.parse().ok()? })
}

impl fmt::Display for SongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SongError::Raw(text) => f.write_str(text),
            SongError::TrainingOnCard => f.write_str("a LoRA is training on the card; make songs once it finishes"),
            SongError::LostVram => f.write_str("The graphics card ran out of memory on this song and the engine restarted, so the song was lost. Choose a smaller model set in the model manager or a shorter song, and close whatever else uses the card; a card below the smallest set cannot make songs."),
            SongError::LostRam => f.write_str("The computer ran out of memory (RAM) on this song and the engine restarted, so the song was lost. Close other programs that take memory, or choose a smaller model set in the model manager or a shorter song."),
            SongError::LostDevice { detail: Some(detail), card } => {
                let card = card.as_deref().map(|card| format!(" on {card}")).unwrap_or_default();
                write!(f, "The graphics card failed during this song ({detail}){card} and the engine restarted, so the song was lost. The engine log is in Settings, Engine.")
            }
            SongError::LostDevice { detail: None, .. } => f.write_str("The graphics card failed during this song (a CUDA or driver error) and the engine restarted, so the song was lost. The engine log is in Settings, Engine."),
            SongError::LostAtLine(line) => write!(f, "The engine stopped during this song and restarted, so the song was lost; its last line was {line}. The engine log is in Settings, Engine."),
            SongError::Lost => f.write_str("The engine stopped during this song and restarted, so the song was lost. The engine log is in Settings, Engine."),
            SongError::LostExited { exit, line } => {
                let how = match exit_meaning(exit) {
                    Some(meaning) => format!("{meaning}, exit code {exit}"),
                    None => format!("exit code {exit}"),
                };
                match line {
                    Some(line) => write!(f, "The engine stopped during this song ({how}) and restarted, so the song was lost; its last line was {line}. The engine log is in Settings, Engine."),
                    None => write!(f, "The engine stopped during this song ({how}) and restarted, so the song was lost. The engine log is in Settings, Engine."),
                }
            }
            SongError::LostPort { port, set: true, .. } => write!(f, "The engine could not listen on port {port}, which STUDIO_ENGINE_PORT sets, so the song was lost: another program holds it. Close that program or set another port."),
            SongError::LostPort { port, set: false, note } => {
                let chosen = note.as_deref().map(|note| format!(" (At start {note}.)")).unwrap_or_default();
                write!(f, "The engine could not listen on port {port}, so the song was lost: another program took the port after the studio chose it.{chosen} Restart the studio: it chooses a free port at start.")
            }
            SongError::LostKvShort => f.write_str("The language model ran out of room on this song: its KV cache (Settings, Engine, LM KV cache) is smaller than the song needs, and the engine stopped. Leave that field empty to use the model's own size, or set it higher."),
            SongError::OutOfMemory(why) => write!(f, "There was not enough memory for this song ({why}). Choose a smaller model set in the model manager or a shorter song, and close whatever else uses the card or the computer's memory."),
            SongError::EngineFatal(line) | SongError::NotFinite { line, .. } => f.write_str(line),
            SongError::EngineFailed => f.write_str("the engine failed this job and its log names no cause"),
            SongError::AdapterMissing(name) => write!(f, "the adapter {name} is not installed"),
            SongError::AdapterMisfit(name) => write!(f, "The LoRA {name} does not fit the chosen DiT: none of its layers matched the model, so the song was not made. Switch the DiT above the create form to the model the LoRA was trained for, or leave the LoRA out."),
            SongError::AdapterWrongSize { name, made_for, dit } => write!(f, "{name} is a LoRA for {made_for} models and the chosen DiT is {dit}; switch the DiT above the create form to {made_for} or leave this LoRA out"),
            SongError::NotASong(problem) => write!(f, "The engine returned {problem} instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause."),
            SongError::TrackTooLong { seconds } => write!(f, "This track is {}:{:02} long, and the engine takes a source or reference of under 10 minutes. Use a part of it shorter than that.", seconds / 60, seconds % 60),
            SongError::TrackMissing { reference, id } => write!(f, "the {} track {id} is not in the library; choose another track", if *reference { "reference" } else { "source" }),
            SongError::TrackInBin { reference, title } => write!(f, "the {} track \"{title}\" is in the library's bin; restore it in Library - Bin or choose another track", if *reference { "reference" } else { "source" }),
            SongError::ModelMissing(name) => write!(f, "{name} is no longer on disk; choose another in Settings - Models"),
            SongError::SourceRequired(task) => write!(f, "{task} works on a recording; choose the source track"),
            SongError::NoLength => f.write_str("the language model gave the song no length; set the duration in the form"),
        }
    }
}

impl std::error::Error for SongError {}

impl From<String> for SongError {
    fn from(text: String) -> Self {
        SongError::Raw(text)
    }
}

impl From<&str> for SongError {
    fn from(text: &str) -> Self {
        SongError::Raw(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One reason of each code. The `match` without a wildcard does not let a new kind be forgotten here.
    fn one_of_each() -> Vec<SongError> {
        let mut all = vec![
            SongError::Raw("x".into()),
            SongError::TrainingOnCard,
            SongError::LostVram,
            SongError::LostRam,
            SongError::LostDevice { detail: Some("x".into()), card: Some("y".into()) },
            SongError::LostDevice { detail: None, card: None },
            SongError::LostAtLine("x".into()),
            SongError::Lost,
            SongError::LostExited { exit: "0xc0000005".into(), line: None },
            SongError::LostExited { exit: "0xc0000135".into(), line: Some("x".into()) },
            SongError::LostExited { exit: ILLEGAL_INSTRUCTION.into(), line: Some("x".into()) },
            SongError::LostPort { port: 8089, set: true, note: None },
            SongError::LostPort { port: 8090, set: false, note: Some("x".into()) },
            SongError::LostKvShort,
            SongError::OutOfMemory("x".into()),
            SongError::EngineFatal("x".into()),
            SongError::NotFinite { line: "x".into(), dit: true, count: 1, total: 2 },
            SongError::NotFinite { line: "x".into(), dit: false, count: 1, total: 2 },
            SongError::EngineFailed,
            SongError::AdapterMissing("x".into()),
            SongError::AdapterMisfit("x".into()),
            SongError::AdapterWrongSize { name: "x".into(), made_for: "XL".into(), dit: "BASE".into() },
            SongError::TrackTooLong { seconds: 754 },
            SongError::TrackMissing { reference: false, id: "x".into() },
            SongError::TrackMissing { reference: true, id: "x".into() },
            SongError::TrackInBin { reference: false, title: "x".into() },
            SongError::TrackInBin { reference: true, title: "x".into() },
            SongError::ModelMissing("x".into()),
            SongError::SourceRequired("cover".into()),
            SongError::NoLength,
        ];
        all.extend(OUTPUT_PROBLEMS.iter().map(|(problem, _)| SongError::NotASong(problem)));
        for error in &all {
            match error {
                SongError::Raw(_)
                | SongError::TrainingOnCard
                | SongError::LostVram
                | SongError::LostRam
                | SongError::LostDevice { .. }
                | SongError::LostAtLine(_)
                | SongError::Lost
                | SongError::LostExited { .. }
                | SongError::LostPort { .. }
                | SongError::LostKvShort
                | SongError::OutOfMemory(_)
                | SongError::EngineFatal(_)
                | SongError::NotFinite { .. }
                | SongError::EngineFailed
                | SongError::AdapterMissing(_)
                | SongError::AdapterMisfit(_)
                | SongError::AdapterWrongSize { .. }
                | SongError::NotASong(_)
                | SongError::TrackTooLong { .. }
                | SongError::TrackMissing { .. }
                | SongError::TrackInBin { .. }
                | SongError::ModelMissing(_)
                | SongError::SourceRequired(_)
                | SongError::NoLength => {}
            }
        }
        all
    }

    /// Each code is translated into every language of the window: a code without a translation fails here.
    #[test]
    fn every_code_has_a_translation_in_every_language() {
        let dictionary = include_str!("../../../app/i18n/songErrors.ts");
        let mut missing = Vec::new();
        for error in one_of_each() {
            let Some(code) = error.code() else { continue };
            let found = dictionary.matches(&format!("\n  {code}:")).count();
            if found != 5 {
                missing.push(format!("{code}: {found} of 5"));
            }
            for key in error.params().keys() {
                let named = dictionary.matches(&format!("{{{key}}}")).count();
                if named == 0 {
                    missing.push(format!("{code}: no translation uses {{{key}}}"));
                }
            }
        }
        assert!(missing.is_empty(), "without a translation: {missing:?}");
    }

    #[test]
    fn the_english_text_stays_what_the_hub_already_groups_by() {
        assert_eq!(SongError::EngineFailed.to_string(), "the engine failed this job and its log names no cause");
        assert_eq!(
            SongError::NotASong("silence").to_string(),
            "The engine returned silence instead of a song, so nothing was kept. Make it again; if it repeats, the engine log has the cause."
        );
        assert_eq!(
            SongError::TrackTooLong { seconds: 754 }.to_string(),
            "This track is 12:34 long, and the engine takes a source or reference of under 10 minutes. Use a part of it shorter than that."
        );
        assert_eq!(
            SongError::LostDevice { detail: Some("CUDA error: an illegal memory access was encountered".into()), card: Some("RTX 3060".into()) }.to_string(),
            "The graphics card failed during this song (CUDA error: an illegal memory access was encountered) on RTX 3060 and the engine restarted, so the song was lost. The engine log is in Settings, Engine."
        );
        assert_eq!(SongError::Raw("x".into()).code(), None);
        assert_eq!(SongError::not_a_song("something new").code(), None);
    }

    /// The engine's own lines, as acestep.cpp `pipeline-synth-ops.cpp` prints them after `ERROR:`.
    #[test]
    fn the_engine_s_lines_the_window_explains_are_read_with_their_fields() {
        let line = "12 of 3840000 samples are not finite (NaN or Inf)";
        let vae = SongError::from_fatal(line.into());
        assert_eq!(vae.code(), Some("vae_not_finite"));
        assert_eq!(vae.params()["count"], 12);
        assert_eq!(vae.params()["total"], 3840000);
        assert_eq!(vae.to_string(), line);

        let dit = SongError::from_fatal("64 of 96000 latent values are not finite (NaN or Inf)".into());
        assert_eq!(dit.code(), Some("dit_not_finite"));

        let other = SongError::from_fatal("T=15500 exceeds silence_latent max 15000, skipping".into());
        assert_eq!(other.code(), Some("engine_fatal"));
        assert_eq!(other.to_string(), "T=15500 exceeds silence_latent max 15000, skipping");
    }

    /// A song lost with an engine that exited gives the window what the exit status means and the engine's last line,
    /// as the English text does; a status without a meaning says `none`.
    #[test]
    fn a_lost_song_carries_what_the_exit_means_and_the_last_line_to_the_window() {
        let lost = SongError::LostExited { exit: "0xc0000135".into(), line: Some("load cublas64_13.dll".into()) };
        assert_eq!(lost.code(), Some("lost_exited_at_line"));
        let params = lost.params();
        assert_eq!(params["meaning"], "library");
        assert_eq!(params["line"], "load cublas64_13.dll");
        assert!(lost.to_string().contains("a library of the engine did not load"));
        let bare = SongError::LostExited { exit: "0x00000001".into(), line: None };
        assert_eq!(bare.code(), Some("lost_exited"));
        assert_eq!(bare.params()["meaning"], "none");
        assert!(bare.params().get("line").is_none());
        for (statuses, key, _) in EXIT_MEANINGS {
            assert!(statuses.iter().all(|status| exit_meaning_key(status) == *key));
        }
    }
}
