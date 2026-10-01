//! Standard MIDI Files: the notes and timing a file holds, and a file written
//! from them. Reading is forgiving where files in the wild are sloppy and
//! strict where a guess would put wrong notes into a track: running status
//! survives a meta event, a note-on at velocity 0 ends a note, a note left
//! sounding ends with its track, a note struck again before it was released
//! pairs its offs in order, chunks of other kinds are passed over, and a RIFF
//! 'RMID' wrapper is opened. A file cut short, a data byte with no event to
//! belong to, and a file timed in SMPTE frames are refused with the reason.
//! The reader keeps everything a file holds (names, texts, meters, keys,
//! markers) though the MIDI editor's notes use only notes, programs and tempos.
#![allow(dead_code)]

use std::collections::BTreeMap;

/// Ticks per quarter note in a file written here: a 1/32 note is 60 ticks.
pub const DIVISION: u32 = 480;
/// The channel General MIDI keeps for drums, counted from 0.
pub const DRUM_CHANNEL: u8 = 9;
/// Microseconds per quarter note until a file sets a tempo: 120 BPM.
pub const DEFAULT_TEMPO: u32 = 500_000;

pub const TEXT: u8 = 0x01;
pub const COPYRIGHT: u8 = 0x02;
pub const NAME: u8 = 0x03;
pub const INSTRUMENT: u8 = 0x04;
pub const LYRIC: u8 = 0x05;
pub const MARKER: u8 = 0x06;
pub const CUE: u8 = 0x07;
const END: u8 = 0x2F;
const TEMPO: u8 = 0x51;
const METER: u8 = 0x58;
const KEY: u8 = 0x59;
const TEXTS: [u8; 7] = [TEXT, COPYRIGHT, NAME, INSTRUMENT, LYRIC, MARKER, CUE];

fn data_bytes(kind: u8) -> usize {
    match kind {
        0xC | 0xD => 1,
        _ => 2,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Note {
    pub start: u64,
    pub end: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub channel: u8,
}

/// A track chunk as read. Names and lyrics stay bytes: only their reader can
/// tell which encoding a file was written in.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub index: usize,
    pub name: Vec<u8>,
    pub notes: Vec<Note>,
    pub programs: BTreeMap<u8, u8>,
    pub texts: Vec<(u64, u8, Vec<u8>)>,
    pub end: u64,
}

/// A whole file: the timing every track shares is gathered from all of them,
/// since files disagree about which track carries it.
#[derive(Clone, Debug, Default)]
pub struct Song {
    pub format: u16,
    pub division: u32,
    pub tracks: Vec<Track>,
    pub tempos: Vec<(u64, u32)>,
    pub meters: Vec<(u64, u32, u32)>,
    pub keys: Vec<(u64, i32, bool)>,
    pub markers: Vec<(u64, Vec<u8>)>,
}

impl Song {
    pub fn end(&self) -> u64 {
        self.tracks.iter().map(|track| track.end).max().unwrap_or(0)
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
    context: String,
}

impl Cursor<'_> {
    fn more(&self) -> bool {
        self.at < self.data.len()
    }

    fn byte(&mut self) -> Result<u8, String> {
        let value = *self.data.get(self.at).ok_or_else(|| format!("the file ends in the middle of {}", self.context))?;
        self.at += 1;
        Ok(value)
    }

    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        if self.at + count > self.data.len() {
            return Err(format!("the file ends in the middle of {}", self.context));
        }
        let value = &self.data[self.at..self.at + count];
        self.at += count;
        Ok(value)
    }

    fn number(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for _ in 0..4 {
            let byte = self.byte()?;
            value = (value << 7) | u64::from(byte & 0x7F);
            if byte < 0x80 {
                return Ok(value);
            }
        }
        Err(format!("a length in {} runs past four bytes, which no MIDI file writes", self.context))
    }
}

fn unwrap_riff(data: &[u8]) -> Result<&[u8], String> {
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"RMID" {
        return Ok(data);
    }
    let mut at = 12;
    while at + 8 <= data.len() {
        let kind = &data[at..at + 4];
        let size = u32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
        if kind == b"data" {
            return Ok(&data[at + 8..(at + 8 + size).min(data.len())]);
        }
        at += 8 + size + (size & 1);
    }
    Err("this RIFF file says it holds MIDI, but it has no 'data' chunk".into())
}

fn meta_event(song: &mut Song, track: &mut Track, tick: u64, kind: u8, payload: &[u8]) {
    if kind == TEMPO && payload.len() == 3 {
        let value = u32::from_be_bytes([0, payload[0], payload[1], payload[2]]);
        if value > 0 {
            song.tempos.push((tick, value));
        }
    } else if kind == METER && payload.len() >= 2 {
        if payload[0] > 0 && payload[1] <= 6 {
            song.meters.push((tick, u32::from(payload[0]), 1 << payload[1]));
        }
    } else if kind == KEY && payload.len() >= 2 {
        let sharps = i32::from(payload[0] as i8);
        if (-7..=7).contains(&sharps) && payload[1] <= 1 {
            song.keys.push((tick, sharps, payload[1] == 1));
        }
    } else if kind == MARKER {
        song.markers.push((tick, payload.to_vec()));
    } else if kind == NAME && track.name.is_empty() {
        track.name = payload.to_vec();
    }
    if TEXTS.contains(&kind) {
        track.texts.push((tick, kind, payload.to_vec()));
    }
}

fn read_track(body: &[u8], index: usize, song: &mut Song) -> Result<Track, String> {
    let mut track = Track { index, ..Track::default() };
    let mut cursor = Cursor { data: body, at: 0, context: format!("track {}", index + 1) };
    let mut tick = 0u64;
    let mut status: Option<u8> = None;
    let mut sounding: BTreeMap<(u8, u8), Vec<(u64, u8)>> = BTreeMap::new();
    while cursor.more() {
        tick += cursor.number()?;
        let first = cursor.byte()?;
        if first == 0xFF {
            let kind = cursor.byte()?;
            let length = cursor.number()? as usize;
            let payload = cursor.take(length)?.to_vec();
            if kind == END {
                break;
            }
            meta_event(song, &mut track, tick, kind, &payload);
            continue;
        }
        if first == 0xF0 || first == 0xF7 {
            let length = cursor.number()? as usize;
            cursor.take(length)?;
            continue;
        }
        let values: Vec<u8> = if first & 0x80 != 0 {
            if first >= 0xF0 {
                return Err(format!("track {} holds a {first:02X} event, which does not belong in a MIDI file", index + 1));
            }
            status = Some(first);
            let mut values = Vec::new();
            for _ in 0..data_bytes(first >> 4) {
                values.push(cursor.byte()?);
            }
            values
        } else {
            let Some(current) = status else {
                return Err(format!("track {} has a data byte where an event should begin", index + 1));
            };
            let mut values = vec![first];
            for _ in 1..data_bytes(current >> 4) {
                values.push(cursor.byte()?);
            }
            values
        };
        if values.iter().any(|value| value & 0x80 != 0) {
            return Err(format!("track {} has an event cut short of its data bytes", index + 1));
        }
        let current = status.expect("an event has a status");
        let (kind, channel) = (current >> 4, current & 0x0F);
        if kind == 0x9 && values[1] > 0 {
            sounding.entry((channel, values[0])).or_default().push((tick, values[1]));
        } else if kind == 0x8 || kind == 0x9 {
            if let Some(opened) = sounding.get_mut(&(channel, values[0])) {
                if !opened.is_empty() {
                    let (start, velocity) = opened.remove(0);
                    track.notes.push(Note { start, end: tick, pitch: values[0], velocity, channel });
                }
            }
        } else if kind == 0xC {
            track.programs.entry(channel).or_insert(values[0]);
        }
    }
    track.end = tick;
    for ((channel, pitch), opened) in sounding {
        for (start, velocity) in opened {
            track.notes.push(Note { start, end: tick, pitch, velocity, channel });
        }
    }
    track.notes.sort();
    Ok(track)
}

/// The song a Standard MIDI File holds, or why the file cannot be read.
pub fn read(data: &[u8]) -> Result<Song, String> {
    let data = unwrap_riff(data)?;
    if data.len() < 4 || &data[..4] != b"MThd" {
        return Err("this is not a MIDI file: it does not begin with 'MThd'".into());
    }
    if data.len() < 14 {
        return Err("the MIDI header is cut short".into());
    }
    let size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if size < 6 {
        return Err(format!("the MIDI header is {size} bytes long, shorter than the format allows"));
    }
    let format = u16::from_be_bytes([data[8], data[9]]);
    let count = u16::from_be_bytes([data[10], data[11]]) as usize;
    let division = u16::from_be_bytes([data[12], data[13]]);
    if division & 0x8000 != 0 {
        return Err("the file is timed in SMPTE frames rather than beats, which this reader does not follow".into());
    }
    if division == 0 {
        return Err("the file gives a quarter note no ticks".into());
    }
    let mut song = Song { format, division: u32::from(division), ..Song::default() };
    let mut at = 8 + size;
    while at + 8 <= data.len() && song.tracks.len() < count {
        let kind = &data[at..at + 4];
        let length = u32::from_be_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
        let body = &data[at + 8..(at + 8 + length).min(data.len())];
        at += 8 + length;
        if kind != b"MTrk" {
            continue;
        }
        let index = song.tracks.len();
        let track = read_track(body, index, &mut song)?;
        song.tracks.push(track);
    }
    if song.tracks.is_empty() {
        return Err("the MIDI file holds no track".into());
    }
    song.tempos.sort_by_key(|row| row.0);
    song.meters.sort_by_key(|row| row.0);
    song.keys.sort_by_key(|row| row.0);
    song.markers.sort_by_key(|row| row.0);
    Ok(song)
}

/// A variable-length quantity, the way delta times and meta lengths are written.
pub fn number(value: u64) -> Result<Vec<u8>, String> {
    if value > 0x0FFF_FFFF {
        return Err(format!("{value} does not fit a MIDI variable-length number"));
    }
    let mut value = value;
    let mut out = vec![(value & 0x7F) as u8];
    value >>= 7;
    while value > 0 {
        out.push(((value & 0x7F) as u8) | 0x80);
        value >>= 7;
    }
    out.reverse();
    Ok(out)
}

pub fn meta(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF, kind];
    out.extend(number(payload.len() as u64).expect("a meta event this module writes fits"));
    out.extend_from_slice(payload);
    out
}

pub fn tempo(microseconds: u32) -> Vec<u8> {
    meta(TEMPO, &microseconds.to_be_bytes()[1..])
}

/// A time signature, with the metronome at every quarter and eight 32nds to a quarter.
pub fn meter(numerator: u32, denominator: u32) -> Vec<u8> {
    let power = 31 - denominator.max(1).leading_zeros();
    meta(METER, &[numerator as u8, power as u8, 24, 8])
}

/// A key signature: sharps positive, flats negative.
pub fn key(sharps: i32, minor: bool) -> Vec<u8> {
    meta(KEY, &[sharps as i8 as u8, u8::from(minor)])
}

pub fn text(kind: u8, value: &str) -> Vec<u8> {
    meta(kind, value.as_bytes())
}

pub fn program(channel: u8, value: u8) -> Vec<u8> {
    vec![0xC0 | channel, value]
}

pub fn note_on(channel: u8, pitch: u8, velocity: u8) -> Vec<u8> {
    vec![0x90 | channel, pitch, velocity]
}

pub fn note_off(channel: u8, pitch: u8) -> Vec<u8> {
    vec![0x80 | channel, pitch, 0]
}

fn rank(event: &[u8]) -> u8 {
    if event[0] == 0xFF {
        0
    } else if event[0] >> 4 == 0x8 {
        1
    } else {
        2
    }
}

/// A format-1 file from tracks of `(tick, event)`. At one tick meta events
/// come first and note-offs before the rest, so a note that ends where the
/// same pitch starts again is not cut short by a player pairing by pitch;
/// every track runs to `end`, so closing bars of rest stay in the file.
pub fn write(tracks: &[Vec<(u64, Vec<u8>)>], division: u32, end: u64) -> Result<Vec<u8>, String> {
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend((division as u16).to_be_bytes());
    for events in tracks {
        let mut ordered: Vec<(usize, &(u64, Vec<u8>))> = events.iter().enumerate().collect();
        ordered.sort_by_key(|(position, (tick, event))| (*tick, rank(event), *position));
        let mut body = Vec::new();
        let mut last = 0u64;
        for (_, (tick, event)) in ordered {
            body.extend(number(tick - last)?);
            body.extend_from_slice(event);
            last = *tick;
        }
        body.extend(number(end.saturating_sub(last))?);
        body.extend(meta(END, b""));
        out.extend(b"MTrk");
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_file_reads_back_with_its_notes_and_timing() {
        let conductor = vec![(0, text(NAME, "Song")), (0, tempo(500_000)), (0, meter(3, 4)), (0, key(-3, true)), (960, text(MARKER, "chorus"))];
        let voice = vec![(0, text(NAME, "Vocal")), (0, program(0, 53)), (0, note_on(0, 60, 100)), (480, note_off(0, 60)), (480, note_on(0, 60, 100)), (960, note_off(0, 60))];
        let data = write(&[conductor, voice], DIVISION, 1440).unwrap();
        let song = read(&data).unwrap();
        assert_eq!((song.format, song.division, song.tracks.len()), (1, 480, 2));
        assert_eq!(song.tempos, vec![(0, 500_000)]);
        assert_eq!(song.meters, vec![(0, 3, 4)]);
        assert_eq!(song.keys, vec![(0, -3, true)]);
        assert_eq!(song.markers, vec![(960, b"chorus".to_vec())]);
        let notes = &song.tracks[1].notes;
        assert_eq!(notes.len(), 2, "a note ending where the same pitch starts again is not cut short");
        assert_eq!((notes[0].start, notes[0].end, notes[1].start, notes[1].end), (0, 480, 480, 960));
        assert_eq!(song.tracks[1].name, b"Vocal".to_vec());
        assert_eq!(song.end(), 1440);
    }

    #[test]
    fn running_status_and_zero_velocity_offs_are_read() {
        let mut body = Vec::new();
        body.extend([0x00, 0x90, 60, 90, 0x60, 62, 90, 0x60, 60, 0, 0x60, 62, 0]);
        body.extend([0x00, 0xFF, 0x2F, 0x00]);
        let mut data = b"MThd".to_vec();
        data.extend([0, 0, 0, 6, 0, 0, 0, 1, 0x01, 0xE0]);
        data.extend(b"MTrk");
        data.extend((body.len() as u32).to_be_bytes());
        data.extend(body);
        let song = read(&data).unwrap();
        let notes = &song.tracks[0].notes;
        assert_eq!(notes.iter().map(|note| (note.start, note.end, note.pitch)).collect::<Vec<_>>(), vec![(0, 192, 60), (96, 288, 62)]);
    }

    #[test]
    fn what_is_not_a_midi_file_is_refused_with_the_reason() {
        assert!(read(b"RIFF").unwrap_err().contains("MThd"));
        let mut smpte = b"MThd".to_vec();
        smpte.extend([0, 0, 0, 6, 0, 1, 0, 1, 0xE7, 0x28]);
        assert!(read(&smpte).unwrap_err().contains("SMPTE"));
    }
}
