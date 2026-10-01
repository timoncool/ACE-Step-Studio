//! A .mid from the MIDI editor read into the notes a library track's MIDI
//! keeps beside the file, with every instrument named the way the transcriber
//! names it, so a track's MIDI reads the same whichever of the two made it.

use crate::midi::Note;
use crate::smf;

/// The transcriber's instrument groups and the first General MIDI program (from 0) of each.
const INSTRUMENTS: [(&str, u8); 34] = [
    ("acoustic_piano", 0),
    ("electric_piano", 2),
    ("chromatic_percussion", 8),
    ("organ", 16),
    ("acoustic_guitar", 24),
    ("clean_electric_guitar", 26),
    ("distorted_electric_guitar", 29),
    ("acoustic_bass", 32),
    ("electric_bass", 33),
    ("violin", 40),
    ("viola", 41),
    ("cello", 42),
    ("contrabass", 43),
    ("orchestral_harp", 46),
    ("timpani", 47),
    ("string_ensemble", 48),
    ("synth_strings", 50),
    ("voice", 52),
    ("orchestra_hit", 55),
    ("trumpet", 56),
    ("trombone", 57),
    ("tuba", 58),
    ("french_horn", 60),
    ("brass_section", 61),
    ("soprano_and_alto_sax", 64),
    ("tenor_sax", 66),
    ("baritone_sax", 67),
    ("oboe", 68),
    ("english_horn", 69),
    ("bassoon", 70),
    ("clarinet", 71),
    ("flutes", 72),
    ("synth_lead", 80),
    ("synth_pad", 88),
];

const DRUMS: &str = "drums";

/// The instrument a program is read as: the group it falls in, or `program_<n>` above the groups the transcriber knows.
fn instrument_of(program: u8) -> String {
    if program >= 96 {
        return format!("program_{program}");
    }
    INSTRUMENTS.iter().rev().find(|(_, first)| *first <= program).map(|(name, _)| name.to_string()).unwrap_or_else(|| format!("program_{program}"))
}

/// The instruments the notes play, in the order each first sounds.
pub fn instruments(notes: &[Note]) -> Vec<String> {
    let mut ordered: Vec<&Note> = notes.iter().collect();
    ordered.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut names: Vec<String> = Vec::new();
    for note in ordered {
        if !names.contains(&note.instrument) {
            names.push(note.instrument.clone());
        }
    }
    names
}

/// A file's notes in seconds through its tempo map, ordered by start and pitch.
pub fn read(data: &[u8]) -> Result<Vec<Note>, String> {
    let song = smf::read(data)?;
    let division = f64::from(song.division);
    let seconds = |tick: u64| -> f64 {
        let (mut total, mut last, mut tempo) = (0.0, 0u64, smf::DEFAULT_TEMPO);
        for &(change, value) in &song.tempos {
            if change >= tick {
                break;
            }
            total += (change - last) as f64 * f64::from(tempo) / 1e6 / division;
            (last, tempo) = (change, value);
        }
        total + (tick - last) as f64 * f64::from(tempo) / 1e6 / division
    };
    let mut programs: std::collections::BTreeMap<u8, u8> = std::collections::BTreeMap::new();
    for track in &song.tracks {
        for (channel, program) in &track.programs {
            programs.entry(*channel).or_insert(*program);
        }
    }
    let mut notes = Vec::new();
    for track in &song.tracks {
        for note in &track.notes {
            let instrument = if note.channel == smf::DRUM_CHANNEL {
                DRUMS.to_string()
            } else {
                instrument_of(track.programs.get(&note.channel).or_else(|| programs.get(&note.channel)).copied().unwrap_or(0))
            };
            notes.push(Note { pitch: note.pitch, start: seconds(note.start), end: seconds(note.end.max(note.start + 1)), instrument, velocity: Some(note.velocity.max(1)) });
        }
    }
    if notes.is_empty() {
        return Err("the file has no notes".into());
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_read_in_seconds_with_their_instruments() {
        let piano = vec![(0, smf::text(smf::NAME, "Piano")), (0, smf::program(0, 1)), (0, smf::note_on(0, 60, 90)), (480, smf::note_off(0, 60))];
        let bass = vec![(0, smf::program(1, 33)), (480, smf::note_on(1, 40, 100)), (1440, smf::note_off(1, 40))];
        let drums = vec![(0, smf::note_on(smf::DRUM_CHANNEL, 36, 120)), (120, smf::note_off(smf::DRUM_CHANNEL, 36))];
        let conductor = vec![(0, smf::tempo(500_000)), (0, smf::meter(4, 4))];
        let data = smf::write(&[conductor, piano, bass, drums], smf::DIVISION, 1440).unwrap();
        let notes = read(&data).unwrap();
        let beat = 480.0 / f64::from(smf::DIVISION) * 0.5;
        assert_eq!(notes.len(), 3);
        assert_eq!((notes[0].instrument.as_str(), notes[0].pitch, notes[0].velocity), ("drums", 36, Some(120)));
        assert_eq!((notes[1].instrument.as_str(), notes[1].start, notes[1].end), ("acoustic_piano", 0.0, beat));
        assert_eq!((notes[2].instrument.as_str(), notes[2].start, notes[2].end), ("electric_bass", beat, beat * 3.0));
        assert_eq!(instruments(&notes), ["drums", "acoustic_piano", "electric_bass"]);
    }

    #[test]
    fn programs_fall_in_the_transcribers_groups() {
        assert_eq!(instrument_of(1), "acoustic_piano");
        assert_eq!(instrument_of(30), "distorted_electric_guitar");
        assert_eq!(instrument_of(73), "flutes");
        assert_eq!(instrument_of(100), "program_100");
    }

    #[test]
    fn a_file_that_is_not_midi_or_has_no_notes_is_refused() {
        assert!(read(b"not midi").is_err());
        let empty = smf::write(&[vec![(0, smf::tempo(500_000))]], smf::DIVISION, 0).unwrap();
        assert_eq!(read(&empty).unwrap_err(), "the file has no notes");
    }
}
