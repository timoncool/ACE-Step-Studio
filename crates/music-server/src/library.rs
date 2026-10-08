use std::{env, fs, path::{Path, PathBuf}, sync::{Arc, Mutex}};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Library { connection: Arc<Mutex<Connection>>, media_dir: PathBuf }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Song { pub id:String, pub title:String, pub audio_path:Option<String>, pub caption:String, pub lyrics:String, pub metadata:serde_json::Value, pub generation_settings:serde_json::Value, pub engine_id:String, pub profile_id:Option<String>, pub replay_request:Option<serde_json::Value>, pub audio_codes:Option<serde_json::Value>, pub source:String, pub created_at:String, pub updated_at:String }
#[derive(Debug, Clone, Deserialize)]
pub struct SongInput { pub title:String, pub audio_path:Option<String>, #[serde(default)] pub caption:String, #[serde(default)] pub lyrics:String, #[serde(default)] pub metadata:serde_json::Value, #[serde(default)] pub generation_settings:serde_json::Value, pub engine_id:String, pub profile_id:Option<String>, pub replay_request:Option<serde_json::Value>, pub audio_codes:Option<serde_json::Value>, #[serde(default="manual_source")] pub source:String }
#[derive(Debug, Clone)]
pub struct GeneratedSongInput { pub title:Option<String>, pub metadata:serde_json::Value, pub caption:String, pub lyrics:String, pub generation_settings:serde_json::Value, pub replay_request:Option<serde_json::Value>, pub audio_codes:Option<serde_json::Value>, pub engine_id:String, pub profile_id:Option<String>, pub source:String, pub audio_extension:&'static str, pub audio:Vec<u8> }
#[derive(Debug, Clone)]
pub struct AudioImportInput { pub title:String, pub caption:String, pub lyrics:String, pub metadata:serde_json::Value, pub generation_settings:serde_json::Value, pub engine_id:String, pub profile_id:Option<String>, pub source:String, pub audio_extension:String, pub audio:Vec<u8> }
#[derive(Debug, Clone, Serialize)]
pub struct ImportedSong { pub song: Song, pub audio_filename: String }
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct Playlist { pub id:String, pub name:String, pub description:Option<String>, pub song_ids:Vec<String>, pub created_at:String, pub updated_at:String }
/// A line of the studio's journal: a change an agent made - what was done, to what
/// kind of thing and its name, put into words by the window in its language - or a
/// message the studio showed, with its tone.
/// How many of the latest journal lines are kept, and read.
pub const JOURNAL_KEPT:i64=500;
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct JournalEntry { #[serde(default)] pub id:i64, #[serde(default)] pub at:String, pub source:String, #[serde(default)] pub tone:String, #[serde(default)] pub verb:String, #[serde(default)] pub kind:String, #[serde(default)] pub target:String, #[serde(default)] pub text:String }
#[derive(Debug, Clone, Deserialize)] pub struct PlaylistInput { pub name:String, pub description:Option<String>, #[serde(default)] pub song_ids:Vec<String> }
fn manual_source()->String{"manual".into()}

/// Playable length measured from the audio itself.
///
/// The engine's replay request is sparse — a 60-second track omits the
/// `duration` field because 60 is the default — so the only dependable source
/// for a library row is the rendered file. WAV is read from its header; MP3 is
/// derived from the declared constant bitrate.
pub fn audio_duration_seconds(audio:&[u8],extension:&str,declared_bitrate_kbps:Option<u32>)->Option<f64>{
 match extension{
  "wav"=>{
   if audio.len()<44||&audio[0..4]!=b"RIFF"||&audio[8..12]!=b"WAVE"{return None}
   let fmt=audio.windows(4).position(|w|w==b"fmt ")?;
   let channels=u16::from_le_bytes(audio.get(fmt+10..fmt+12)?.try_into().ok()?) as f64;
   let rate=u32::from_le_bytes(audio.get(fmt+12..fmt+16)?.try_into().ok()?) as f64;
   let bits=u16::from_le_bytes(audio.get(fmt+22..fmt+24)?.try_into().ok()?) as f64;
   let data=audio.windows(4).position(|w|w==b"data")?;
   let payload=u32::from_le_bytes(audio.get(data+4..data+8)?.try_into().ok()?) as f64;
   let bytes_per_second=rate*channels*(bits/8.0);
   (bytes_per_second>0.0).then(||payload/bytes_per_second)
  }
  "mp3"=>{
   let bitrate=declared_bitrate_kbps.unwrap_or(128) as f64*1000.0;
   (bitrate>0.0).then(||(audio.len() as f64*8.0)/bitrate)
  }
  "flac"=>{
   use lofty::file::AudioFile;
   let file=lofty::flac::FlacFile::read_from(&mut std::io::Cursor::new(audio),lofty::config::ParseOptions::new()).ok()?;
   Some(file.properties().duration().as_secs_f64())
  }
  _=>None,
 }
}

/// A caption is a full style prompt, not a song name. Without an explicit
/// title the library shows a readable fragment instead of the whole prompt.
/// A caption written for this model is a labelled document, so its first line
/// is a section heading - "Global Metadata" - which makes a useless title. Skip
/// the headings and take the first line that actually describes the music.
const CAPTION_HEADINGS:[&str;5]=["global metadata","vocal details","arrangement","basic attributes","instrument lifecycle description"];
/// "bpm is 118" and "key is F# minor" describe the music but read terribly as a
/// name, so the title skips past them to the first descriptive phrase.
const CAPTION_ATTRIBUTES:[&str;5]=["bpm is","key is","scale is","tempo is","time signature"];
fn generated_title(caption:&str)->String{
 let first=caption.split(['\n','.',';']).map(str::trim)
  .find(|part|{
   let lowered=part.to_ascii_lowercase();
   !part.is_empty()
    && !CAPTION_HEADINGS.iter().any(|heading|lowered.starts_with(heading))
    && !CAPTION_ATTRIBUTES.iter().any(|attribute|lowered.contains(attribute))
  })
  .unwrap_or("Untitled track");
 let mut title=String::new();
 for word in first.split_whitespace(){
  if !title.is_empty() && title.chars().count()+1+word.chars().count()>48 {break}
  if !title.is_empty(){title.push(' ')}
  title.push_str(word);
 }
 if title.is_empty(){"Untitled track".into()}else{title}
}

/// What an image actually is, read off its first bytes.
///
/// Every picture format announces itself: a claim in a header or a filename is
/// a claim, this is the fact.
pub fn sniff_image_type(image:&[u8])->Option<&'static str>{
 if image.starts_with(&[0xFF,0xD8,0xFF]){return Some("image/jpeg")}
 if image.starts_with(&[0x89,b'P',b'N',b'G',0x0D,0x0A,0x1A,0x0A]){return Some("image/png")}
 if image.len()>=12&&&image[..4]==b"RIFF"&&&image[8..12]==b"WEBP"{return Some("image/webp")}
 None
}

#[cfg(test)]
mod image_type_tests {
    #[test]
    fn every_format_is_recognised_by_its_own_bytes() {
        assert_eq!(super::sniff_image_type(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(super::sniff_image_type(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), Some("image/png"));
        let mut webp = b"RIFF0000WEBP".to_vec();
        webp.extend_from_slice(b"VP8 ");
        assert_eq!(super::sniff_image_type(&webp), Some("image/webp"));
        assert_eq!(super::sniff_image_type(b"not a picture"), None);
    }
}

impl Library {
 /// The library lives in the same place as models and settings. Resolving it
    /// separately used to put the database under `<cwd>/data` whenever the
    /// service was started outside the desktop shell, so the same install
    /// showed two different libraries depending on how it was launched.
    pub fn open_default()->Result<Self>{let root=crate::studio_data_root().unwrap_or_else(||env::current_dir().unwrap_or_else(|_|PathBuf::from(".")).join("data"));Self::open_at(root.join("library.sqlite"),root.join("media"))}
 pub fn open_at(db_path:PathBuf,media_dir:PathBuf)->Result<Self>{if let Some(p)=db_path.parent(){fs::create_dir_all(p)?};let c=Connection::open(db_path)?;c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS songs(id TEXT PRIMARY KEY,title TEXT NOT NULL,audio_path TEXT,caption TEXT NOT NULL,lyrics TEXT NOT NULL,metadata_json TEXT NOT NULL,generation_settings_json TEXT NOT NULL,engine_id TEXT NOT NULL,profile_id TEXT,replay_request_json TEXT,audio_codes_json TEXT,source TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS playlists(id TEXT PRIMARY KEY,name TEXT NOT NULL,description TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS playlist_songs(playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,song_id TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,position INTEGER NOT NULL,PRIMARY KEY(playlist_id,song_id)); CREATE TABLE IF NOT EXISTS journal(id INTEGER PRIMARY KEY AUTOINCREMENT,at TEXT NOT NULL,source TEXT NOT NULL,tone TEXT NOT NULL DEFAULT '',verb TEXT NOT NULL DEFAULT '',kind TEXT NOT NULL DEFAULT '',target TEXT NOT NULL DEFAULT '',text TEXT NOT NULL DEFAULT '');")?;Ok(Self{connection:Arc::new(Mutex::new(c)),media_dir})}
 pub fn list_songs(&self)->Result<Vec<Song>>{let c=self.connection.lock().unwrap();let mut s=c.prepare("SELECT id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,engine_id,profile_id,replay_request_json,audio_codes_json,source,created_at,updated_at FROM songs ORDER BY created_at DESC, id DESC")?;Ok(s.query_map([],row_song)?.collect::<rusqlite::Result<_>>()?)}
 pub fn get_song(&self,id:&str)->Result<Option<Song>>{let c=self.connection.lock().unwrap();Ok(c.query_row("SELECT id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,engine_id,profile_id,replay_request_json,audio_codes_json,source,created_at,updated_at FROM songs WHERE id=?",[id],row_song).optional()?)}
 pub fn create_song(&self,input:SongInput)->Result<Song>{let now=now();let song=Song{id:uuid::Uuid::now_v7().to_string(),title:input.title,audio_path:input.audio_path,caption:input.caption,lyrics:input.lyrics,metadata:input.metadata,generation_settings:input.generation_settings,engine_id:input.engine_id,profile_id:input.profile_id,replay_request:input.replay_request,audio_codes:input.audio_codes,source:input.source,created_at:now.clone(),updated_at:now};self.save_song(&song)?;Ok(song)}
 pub fn import_generated_song(&self,input:GeneratedSongInput)->Result<ImportedSong>{
  if input.audio.is_empty(){anyhow::bail!("cannot import an empty audio result")}
  fs::create_dir_all(&self.media_dir).with_context(||format!("create media directory {}",self.media_dir.display()))?;
  let id=uuid::Uuid::now_v7().to_string();let filename=format!("{id}.{}",input.audio_extension);let target=self.media_dir.join(&filename);let temporary=self.media_dir.join(format!("{filename}.part"));
  {let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&temporary)?;use std::io::Write;file.write_all(&input.audio)?;file.sync_all()?;}
  fs::rename(&temporary,&target).with_context(||format!("publish generated audio {}",target.display()))?;
  let now=now();let title=input.title.map(|t|t.trim().to_owned()).filter(|t|!t.is_empty()).unwrap_or_else(||generated_title(&input.caption));let song=Song{id,title,audio_path:Some(target.display().to_string()),caption:input.caption,lyrics:input.lyrics,metadata:input.metadata,generation_settings:input.generation_settings,engine_id:input.engine_id,profile_id:input.profile_id,replay_request:input.replay_request,audio_codes:input.audio_codes,source:input.source,created_at:now.clone(),updated_at:now};
  if let Err(error)=self.save_song(&song){let _=fs::remove_file(&target);return Err(error.context("store generated song record"));}
  Ok(ImportedSong{song,audio_filename:filename})
 }
 pub fn import_audio_song(&self,input:AudioImportInput)->Result<ImportedSong>{
  if input.audio.is_empty(){anyhow::bail!("cannot import an empty audio file")}
  let extension=input.audio_extension.trim().to_ascii_lowercase();
  if !matches!(extension.as_str(), "mp3" | "wav" | "flac"){anyhow::bail!("only MP3, WAV and FLAC audio can be imported")}
  fs::create_dir_all(&self.media_dir).with_context(||format!("create media directory {}",self.media_dir.display()))?;
  let id=uuid::Uuid::now_v7().to_string(); let filename=format!("{id}.{extension}"); let target=self.media_dir.join(&filename); let temporary=self.media_dir.join(format!("{filename}.part"));
  {let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&temporary)?;use std::io::Write;file.write_all(&input.audio)?;file.sync_all()?;}
  fs::rename(&temporary,&target).with_context(||format!("publish imported audio {}",target.display()))?;
  let now=now(); let song=Song{id,title:input.title,audio_path:Some(target.display().to_string()),caption:input.caption,lyrics:input.lyrics,metadata:input.metadata,generation_settings:input.generation_settings,engine_id:input.engine_id,profile_id:input.profile_id,replay_request:None,audio_codes:None,source:input.source,created_at:now.clone(),updated_at:now};
  if let Err(error)=self.save_song(&song){let _=fs::remove_file(&target);return Err(error.context("store imported song record"));}
  Ok(ImportedSong{song,audio_filename:filename})
 }
 /// Stores a cover image next to the track audio and records its filename in
 /// the song metadata. Covers are a Studio-side concept: the engine never sees
 /// them, so they are stored as plain media rather than in the request record.
 pub fn store_song_cover(&self,id:&str,image:&[u8],media_type:&str)->Result<Song>{self.store_cover(id,image,media_type,None,None)}
 /// A cover chosen for the track that came from somewhere: the page it is
 /// recorded with says where and under what terms.
 pub fn store_chosen_cover(&self,id:&str,image:&[u8],media_type:&str,source:Option<&str>)->Result<Song>{self.store_cover(id,image,media_type,None,source)}
 /// A placeholder written into a track as its cover, recorded with the look it
 /// was drawn with: a change of look draws it again, a real cover replaces it.
 pub fn store_placeholder_cover(&self,id:&str,image:&[u8],media_type:&str,look:&str,source:Option<&str>)->Result<Song>{self.store_cover(id,image,media_type,Some(look),source)}
 fn store_cover(&self,id:&str,image:&[u8],media_type:&str,placeholder:Option<&str>,source:Option<&str>)->Result<Song>{
  if image.is_empty(){anyhow::bail!("cannot store an empty cover image")}
  // The declared type is a claim, the magic numbers are the fact. A model that
  // answered with a JPEG had it stored as `image/png`, and the tag inside the
  // mp3 said PNG over JPEG bytes - players that trust the tag showed nothing.
  let media_type=sniff_image_type(image).unwrap_or(media_type);
  let extension=match media_type.trim().to_ascii_lowercase().as_str(){
   "image/png"=>"png","image/jpeg"|"image/jpg"=>"jpg","image/webp"=>"webp",
   other=>anyhow::bail!("unsupported cover media type '{other}'; use PNG, JPEG or WebP"),
  };
  let Some(mut song)=self.get_song(id)? else{anyhow::bail!("song not found")};
  fs::create_dir_all(&self.media_dir).with_context(||format!("create media directory {}",self.media_dir.display()))?;
  let filename=format!("{id}-cover.{extension}");
  let target=self.media_dir.join(&filename);
  let temporary=self.media_dir.join(format!("{filename}.part"));
  {let mut file=fs::OpenOptions::new().create(true).write(true).truncate(true).open(&temporary)?;use std::io::Write;file.write_all(image)?;file.sync_all()?;}
  fs::rename(&temporary,&target).with_context(||format!("publish cover {}",target.display()))?;
  let previous=song.metadata.get("cover_filename").and_then(|v|v.as_str()).map(str::to_owned);
  if song.metadata.is_null(){song.metadata=serde_json::json!({})}
  if let Some(fields)=song.metadata.as_object_mut(){
   fields.insert("cover_filename".into(),serde_json::Value::String(filename.clone()));
   fields.insert("cover_media_type".into(),serde_json::Value::String(format!("image/{}",if extension=="jpg"{"jpeg"}else{extension})));
   match placeholder{Some(look)=>{fields.insert("cover_placeholder".into(),serde_json::Value::String(look.to_owned()));}None=>{fields.remove("cover_placeholder");}}
   match source{Some(page)=>{fields.insert("cover_source".into(),serde_json::Value::String(page.to_owned()));}None=>{fields.remove("cover_source");}}
  }
  song.updated_at=now();
  if let Err(error)=self.save_song(&song){let _=fs::remove_file(&target);return Err(error.context("store song cover record"))}
  if let Some(previous)=previous.filter(|previous|previous!=&filename){let _=fs::remove_file(self.media_dir.join(previous));}
  // a stem wears its song's cover: a new one reaches the stems separated from it
  let separated=|other:&Song|other.metadata.pointer("/derived/tool").and_then(|v|v.as_str())==Some("stems")&&other.metadata.pointer("/derived/from").and_then(|v|v.as_str())==Some(id);
  for stem in self.list_songs()?.into_iter().filter(separated){
   if let Err(error)=self.store_cover(&stem.id,image,media_type,placeholder,source){eprintln!("[ERROR] the cover of {id} did not reach its stem {}: {error:#}",stem.id)}
  }
  Ok(song)
 }
 /// Takes a placeholder back out of a track. A cover of its own stays.
 pub fn remove_placeholder_cover(&self,id:&str)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{anyhow::bail!("song not found")};
  if song.metadata.get("cover_placeholder").is_none(){return Ok(None)}
  let filename=song.metadata.get("cover_filename").and_then(|v|v.as_str()).map(str::to_owned);
  if let Some(fields)=song.metadata.as_object_mut(){
   for key in ["cover_filename","cover_media_type","cover_placeholder","cover_source"]{fields.remove(key);}
  }
  song.updated_at=now();
  self.save_song(&song).context("take the placeholder cover out of the record")?;
  if let Some(filename)=filename{let _=fs::remove_file(self.media_dir.join(filename));}
  Ok(Some(song))
 }
 pub fn cover_path_for_song(&self,song:&Song)->Option<(PathBuf,String)>{
  let filename=song.metadata.get("cover_filename")?.as_str()?;
  let media_type=song.metadata.get("cover_media_type").and_then(|v|v.as_str()).unwrap_or("image/png").to_owned();
  let path=self.media_file(filename)?;
  // Covers stored before the type was read off the bytes carry the wrong one
  // in their record. The file itself still says what it is.
  let media_type=Self::read_head(&path).and_then(|head|sniff_image_type(&head)).map(str::to_owned).unwrap_or(media_type);
  Some((path,media_type))
 }
 fn read_head(path:&std::path::Path)->Option<Vec<u8>>{
  use std::io::Read;
  let mut file=fs::File::open(path).ok()?;
  let mut head=[0u8;16];
  let read=file.read(&mut head).ok()?;
  Some(head[..read].to_vec())
 }
 /// Where media for this library lives. Stems are written here so they
 /// sit beside the track they came from.
 pub fn media_dir(&self)->&Path{&self.media_dir}
 pub fn media_file(&self,filename:&str)->Option<PathBuf>{if filename.is_empty()||filename.contains(['/', '\\'])||Path::new(filename).file_name().and_then(|x|x.to_str())!=Some(filename){return None}let path=self.media_dir.join(filename);path.is_file().then_some(path)}
 /// Where a stored audio path is now. Paths are stored whole, so a library whose
 /// folder moved - a portable copied elsewhere, a drive back under another
 /// letter - finds its files by name in its own media folder; nothing outside
 /// that folder is ever served.
 pub fn resolve_media(&self,stored:&str)->Option<PathBuf>{
  let root=self.media_dir.canonicalize().ok()?;
  let stored=PathBuf::from(stored);
  if let Ok(path)=stored.canonicalize(){if path.starts_with(&root){return Some(path)}}
  let by_name=self.media_dir.join(stored.file_name()?).canonicalize().ok()?;
  by_name.starts_with(&root).then_some(by_name)
 }
 pub fn media_path_for_song(&self,song:&Song)->Option<PathBuf>{self.resolve_media(song.audio_path.as_ref()?)}
 /// Stores karaoke timings with the track. They live in the metadata rather
 /// than a column of their own so an existing library needs no migration, and
 /// a track without them is simply a track nobody has timed yet.
 pub fn set_song_lrc(&self,id:&str,lrc:&str)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{return Ok(None)};
  let mut metadata=match song.metadata.take(){serde_json::Value::Object(map)=>map,_=>serde_json::Map::new()};
  if lrc.trim().is_empty(){metadata.remove("lrc");}else{metadata.insert("lrc".into(),serde_json::Value::String(lrc.to_owned()));}
  song.metadata=serde_json::Value::Object(metadata);
  song.updated_at=now();
  self.save_song(&song)?;
  Ok(Some(song))
 }

 /// The thumbs-up, kept with the track like its karaoke: every window and the
 /// agent read the same mark, and a profile or machine change keeps it.
 /// `liked_at` orders the liked list, newest first.
 pub fn set_song_liked(&self,id:&str,liked:bool)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{return Ok(None)};
  let mut metadata=match song.metadata.take(){serde_json::Value::Object(map)=>map,_=>serde_json::Map::new()};
  let already=metadata.get("liked").and_then(serde_json::Value::as_bool).unwrap_or(false);
  if liked&&!already{
   metadata.insert("liked".into(),serde_json::Value::Bool(true));
   metadata.insert("liked_at".into(),serde_json::Value::String(now()));
  }else if !liked{
   metadata.remove("liked");
   metadata.remove("liked_at");
  }
  song.metadata=serde_json::Value::Object(metadata);
  if liked==already{return Ok(Some(song))}
  // a like is not an edit of the track: updated_at, which its audio address
  // follows, stays, so a song liked while it plays is not loaded again
  self.save_song(&song)?;
  Ok(Some(song))
 }
 /// The person's own note on a song, kept with it; an empty one is taken away.
 pub fn set_song_note(&self,id:&str,note:&str)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{return Ok(None)};
  let mut metadata=match song.metadata.take(){serde_json::Value::Object(map)=>map,_=>serde_json::Map::new()};
  let note=note.trim();
  if note.is_empty(){metadata.remove("note");}else{metadata.insert("note".into(),serde_json::Value::String(note.to_string()));}
  song.metadata=serde_json::Value::Object(metadata);
  // a note is not an edit of the track: updated_at, which its audio address follows, stays
  self.save_song(&song)?;
  Ok(Some(song))
 }
 /// The liked songs, the latest like first.
 pub fn liked_songs(&self)->Result<Vec<Song>>{
  let liked_at=|song:&Song|song.metadata.get("liked_at").and_then(serde_json::Value::as_str).and_then(|at|at.parse::<u64>().ok()).unwrap_or(0);
  let mut songs:Vec<Song>=self.list_songs()?.into_iter().filter(|song|song.metadata.get("liked").and_then(serde_json::Value::as_bool).unwrap_or(false)).collect();
  songs.sort_by(|a,b|liked_at(b).cmp(&liked_at(a)));
  Ok(songs)
 }
 /// Adds a processed version of a track, stored in the media folder, and makes
 /// it the one that plays. The original's file is remembered on the first
 /// version, so a track can always go back to what it was generated as. Kept in
 /// the metadata, like the karaoke timings, so no migration is needed.
 pub fn select_song_version(&self,id:&str,version:&str)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{return Ok(None)};
  let original=song.metadata.get("original_audio_path").and_then(|v|v.as_str()).map(str::to_owned);
  let audio=if version=="original"{
   let original=original.context("this track has no other version")?;
   self.resolve_media(&original).context("the original's file is missing")?.to_string_lossy().into_owned()
  }else{
   let file=song.metadata.get("audio_versions").and_then(|v|v.as_array()).and_then(|list|list.iter().find(|v|v.get("id").and_then(|x|x.as_str())==Some(version))).and_then(|v|v.get("file")).and_then(|v|v.as_str()).map(str::to_owned).context("no such version")?;
   self.media_file(&file).context("the version's file is missing")?.to_string_lossy().into_owned()
  };
  if let Some(fields)=song.metadata.as_object_mut(){fields.insert("active_version".into(),serde_json::Value::String(version.to_owned()));}
  song.audio_path=Some(audio);
  song.updated_at=now();
  self.save_song(&song)?;
  Ok(Some(song))
 }
 /// Forgets a version; when it was playing, the original plays again.
 /// Returns the file to remove.
 pub fn remove_song_version(&self,id:&str,version:&str)->Result<Option<(Song,Option<PathBuf>)>>{
  let Some(song)=self.get_song(id)? else{return Ok(None)};
  let active=song.metadata.get("active_version").and_then(|v|v.as_str())==Some(version);
  let mut song=if active{self.select_song_version(id,"original")?.context("the track went away")?}else{song};
  let mut file=None;
  if let Some(list)=song.metadata.get_mut("audio_versions").and_then(|v|v.as_array_mut()){
   if let Some(index)=list.iter().position(|v|v.get("id").and_then(|x|x.as_str())==Some(version)){
    file=list[index].get("file").and_then(|v|v.as_str()).and_then(|name|self.media_file(name));
    list.remove(index);
   }
  }
  song.updated_at=now();
  self.save_song(&song)?;
  Ok(Some((song,file)))
 }
 pub fn update_song(&self,id:&str,input:SongInput)->Result<Option<Song>>{let Some(mut song)=self.get_song(id)? else{return Ok(None)};song.title=input.title;if input.audio_path.is_some(){song.audio_path=input.audio_path};song.caption=input.caption;song.lyrics=input.lyrics;song.metadata=merged_metadata(&song.metadata,input.metadata);song.generation_settings=input.generation_settings;song.engine_id=input.engine_id;song.profile_id=input.profile_id;if input.replay_request.is_some(){song.replay_request=input.replay_request};if input.audio_codes.is_some(){song.audio_codes=input.audio_codes};song.source=input.source;song.updated_at=now();self.save_song(&song)?;Ok(Some(song))}
 pub fn delete_song(&self,id:&str)->Result<bool>{let gone=self.connection.lock().unwrap().execute("DELETE FROM songs WHERE id=?",[id])?>0;if gone{crate::mcp::announce("library")}Ok(gone)}
 fn save_song(&self,s:&Song)->Result<()> {self.connection.lock().unwrap().execute("INSERT INTO songs VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,audio_path=excluded.audio_path,caption=excluded.caption,lyrics=excluded.lyrics,metadata_json=excluded.metadata_json,generation_settings_json=excluded.generation_settings_json,engine_id=excluded.engine_id,profile_id=excluded.profile_id,replay_request_json=excluded.replay_request_json,audio_codes_json=excluded.audio_codes_json,source=excluded.source,updated_at=excluded.updated_at",params![s.id,s.title,s.audio_path,s.caption,s.lyrics,s.metadata.to_string(),s.generation_settings.to_string(),s.engine_id,s.profile_id,s.replay_request.as_ref().map(|v|v.to_string()),s.audio_codes.as_ref().map(|v|v.to_string()),s.source,s.created_at,s.updated_at])?;crate::mcp::announce("library");Ok(())}
 pub fn list_playlists(&self)->Result<Vec<Playlist>>{let c=self.connection.lock().unwrap();let mut s=c.prepare("SELECT id,name,description,created_at,updated_at FROM playlists ORDER BY created_at DESC")?;Ok(s.query_map([],|r|playlist(&c,r))?.collect::<rusqlite::Result<_>>()?)}
 pub fn get_playlist(&self,id:&str)->Result<Option<Playlist>>{let c=self.connection.lock().unwrap();c.query_row("SELECT id,name,description,created_at,updated_at FROM playlists WHERE id=?",[id],|r|playlist(&c,r)).optional().map_err(Into::into)}
 pub fn create_playlist(&self,input:PlaylistInput)->Result<Playlist>{let now=now();let p=Playlist{id:uuid::Uuid::now_v7().to_string(),name:input.name,description:input.description,song_ids:input.song_ids,created_at:now.clone(),updated_at:now};self.save_playlist(&p)?;Ok(p)}
 pub fn update_playlist(&self,id:&str,input:PlaylistInput)->Result<Option<Playlist>>{let Some(mut playlist)=self.get_playlist(id)? else{return Ok(None)};playlist.name=input.name;playlist.description=input.description;playlist.song_ids=input.song_ids;playlist.updated_at=now();self.save_playlist(&playlist)?;Ok(Some(playlist))}
 pub fn delete_playlist(&self,id:&str)->Result<bool>{let gone=self.connection.lock().unwrap().execute("DELETE FROM playlists WHERE id=?",[id])?>0;if gone{crate::mcp::announce("library")}Ok(gone)}
 fn save_playlist(&self,p:&Playlist)->Result<()> {let mut c=self.connection.lock().unwrap();let tx=c.transaction()?;tx.execute("INSERT INTO playlists VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,updated_at=excluded.updated_at",params![p.id,p.name,p.description,p.created_at,p.updated_at])?;tx.execute("DELETE FROM playlist_songs WHERE playlist_id=?",[&p.id])?;for(pos,id)in p.song_ids.iter().enumerate(){tx.execute("INSERT INTO playlist_songs VALUES(?,?,?)",params![p.id,id,pos as i64])?;}tx.commit()?;crate::mcp::announce("library");Ok(())}

 /// Writes a line into the journal, keeping the latest ones: an older line
 /// goes as a new one comes.
 pub fn note_journal(&self,entry:&JournalEntry)->Result<JournalEntry>{
  let at=now();
  let id={
   let c=self.connection.lock().unwrap();
   c.execute("INSERT INTO journal(at,source,tone,verb,kind,target,text) VALUES(?,?,?,?,?,?,?)",params![at,entry.source,entry.tone,entry.verb,entry.kind,entry.target,entry.text])?;
   let id=c.last_insert_rowid();
   c.execute("DELETE FROM journal WHERE id<=?",[id-JOURNAL_KEPT])?;
   id
  };
  crate::mcp::announce("journal");
  Ok(JournalEntry{id,at,..entry.clone()})
 }
 /// The journal's latest lines, the newest first.
 pub fn journal(&self,limit:i64)->Result<Vec<JournalEntry>>{
  let c=self.connection.lock().unwrap();
  let mut statement=c.prepare("SELECT id,at,source,tone,verb,kind,target,text FROM journal ORDER BY id DESC LIMIT ?")?;
  Ok(statement.query_map([limit],|r|Ok(JournalEntry{id:r.get(0)?,at:r.get(1)?,source:r.get(2)?,tone:r.get(3)?,verb:r.get(4)?,kind:r.get(5)?,target:r.get(6)?,text:r.get(7)?}))?.collect::<rusqlite::Result<_>>()?)
 }
 pub fn clear_journal(&self)->Result<()>{
  self.connection.lock().unwrap().execute("DELETE FROM journal",[])?;
  crate::mcp::announce("journal");
  Ok(())
 }
 pub fn remove_journal_entry(&self,id:i64)->Result<bool>{
  let gone=self.connection.lock().unwrap().execute("DELETE FROM journal WHERE id=?",[id])?>0;
  if gone{crate::mcp::announce("journal")}
  Ok(gone)
 }
}
/// An edit's metadata over the stored one: what the studio keeps there itself
/// (processed versions, the original audio, karaoke, the cover) is not the
/// client's to drop by leaving it out, an edit with no metadata leaves it as it
/// is, and the like belongs to its own route, so an edit carrying an older copy
/// of it does not undo a like.
fn merged_metadata(stored:&serde_json::Value,edit:serde_json::Value)->serde_json::Value{
 const OWN_ROUTE:[&str;3]=["liked","liked_at","note"];
 let mut edit=match edit{serde_json::Value::Object(edit)=>edit,_=>serde_json::Map::new()};
 let nothing=serde_json::Map::new();
 let stored=stored.as_object().unwrap_or(&nothing);
 for key in OWN_ROUTE{
  match stored.get(key){Some(value)=>{edit.insert(key.into(),value.clone());}None=>{edit.remove(key);}}
 }
 for(key,value)in stored{edit.entry(key.clone()).or_insert_with(||value.clone());}
 serde_json::Value::Object(edit)
}
fn row_song(r:&rusqlite::Row)->rusqlite::Result<Song>{Ok(Song{id:r.get(0)?,title:r.get(1)?,audio_path:r.get(2)?,caption:r.get(3)?,lyrics:r.get(4)?,metadata:json(r.get::<_,String>(5)?),generation_settings:json(r.get::<_,String>(6)?),engine_id:r.get(7)?,profile_id:r.get(8)?,replay_request:r.get::<_,Option<String>>(9)?.map(json),audio_codes:r.get::<_,Option<String>>(10)?.map(json),source:r.get(11)?,created_at:r.get(12)?,updated_at:r.get(13)?})}fn json(s:String)->serde_json::Value{serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)}fn playlist(c:&Connection,r:&rusqlite::Row)->rusqlite::Result<Playlist>{let id:String=r.get(0)?;let mut q=c.prepare("SELECT song_id FROM playlist_songs WHERE playlist_id=? ORDER BY position")?;let song_ids=q.query_map([&id],|x|x.get(0))?.collect::<rusqlite::Result<_>>()?;Ok(Playlist{id,name:r.get(1)?,description:r.get(2)?,song_ids,created_at:r.get(3)?,updated_at:r.get(4)?})}fn now()->String{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs().to_string()}
#[cfg(test)]mod tests{use super::*;#[test]fn a_structured_caption_does_not_become_a_title_of_headings(){
  let caption="Global Metadata
Basic Attributes: bpm is 118. key is F# minor, and scale is minor. Darkwave, Synth-pop. Global Emotional Progression: haunting.";
  assert_eq!(generated_title(caption),"Darkwave, Synth-pop");
  assert_eq!(generated_title("Bright uplifting synth-pop, punchy drums"),"Bright uplifting synth-pop, punchy drums");
 }
#[test]fn imports_audio_and_full_provenance_atomically(){let root=std::env::temp_dir().join(format!("studio-library-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let result=db.import_generated_song(GeneratedSongInput{title:None,metadata:serde_json::Value::Null,caption:"c".into(),lyrics:"l".into(),generation_settings:serde_json::json!({"seed":1}),replay_request:Some(serde_json::json!({"audio_codes":"1,2,3,4,5,6,7,8","seed":1})),audio_codes:Some(serde_json::json!("1,2,3,4,5,6,7,8")),engine_id:"mm".into(),profile_id:Some("recommended-light".into()),source:"local_generation".into(),audio_extension:"mp3",audio:b"ID3".to_vec()}).unwrap();assert_eq!(db.get_song(&result.song.id).unwrap().unwrap().audio_codes,Some(serde_json::json!("1,2,3,4,5,6,7,8")));assert_eq!(fs::read(db.media_file(&result.audio_filename).unwrap()).unwrap(),b"ID3");drop(db);fs::remove_dir_all(&root).unwrap();}
#[test]fn playlist_can_be_read_updated_and_deleted(){let root=std::env::temp_dir().join(format!("studio-playlist-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let song_a=db.create_song(SongInput{title:"A".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;let song_b=db.create_song(SongInput{title:"B".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;let created=db.create_playlist(PlaylistInput{name:"Drafts".into(),description:None,song_ids:vec![song_a]}).unwrap();let updated=db.update_playlist(&created.id,PlaylistInput{name:"Finished".into(),description:Some("native".into()),song_ids:vec![song_b.clone()]}).unwrap().unwrap();assert_eq!(updated.name,"Finished");assert_eq!(db.get_playlist(&created.id).unwrap().unwrap().song_ids,vec![song_b]);assert!(db.delete_playlist(&created.id).unwrap());assert!(db.get_playlist(&created.id).unwrap().is_none());drop(db);fs::remove_dir_all(&root).unwrap();}
#[test]fn measures_wav_duration_from_the_header_and_mp3_from_its_bitrate(){
 // 1 second of 44.1 kHz stereo 16-bit PCM.
 let mut wav=Vec::new();
 wav.extend_from_slice(b"RIFF"); wav.extend_from_slice(&0u32.to_le_bytes()); wav.extend_from_slice(b"WAVE");
 wav.extend_from_slice(b"fmt "); wav.extend_from_slice(&16u32.to_le_bytes());
 wav.extend_from_slice(&1u16.to_le_bytes()); wav.extend_from_slice(&2u16.to_le_bytes());
 wav.extend_from_slice(&44100u32.to_le_bytes()); wav.extend_from_slice(&176400u32.to_le_bytes());
 wav.extend_from_slice(&4u16.to_le_bytes()); wav.extend_from_slice(&16u16.to_le_bytes());
 wav.extend_from_slice(b"data"); wav.extend_from_slice(&176400u32.to_le_bytes());
 wav.resize(wav.len()+176400,0);
 assert!((audio_duration_seconds(&wav,"wav",None).unwrap()-1.0).abs()<0.001);
 // 128 kbps MP3: 16 kB is one second.
 assert!((audio_duration_seconds(&vec![0u8;16000],"mp3",Some(128)).unwrap()-1.0).abs()<0.01);
 assert!(audio_duration_seconds(b"not audio","wav",None).is_none());
}
#[test]fn a_generated_song_gets_a_readable_title_instead_of_the_whole_caption(){
 assert_eq!(generated_title("cinematic synthwave instrumental, 1980s analog synthesizers, warm bassline, soaring lead melody, polished production"),"cinematic synthwave instrumental, 1980s analog");
 assert_eq!(generated_title("Night drive. Wide synths"),"Night drive");
 assert_eq!(generated_title("   "),"Untitled track");
}}
#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn a_rename_keeps_the_processed_versions_and_the_original() {
        let stored = serde_json::json!({ "tags": ["a"], "audio_versions": [{ "id": "v1", "file": "v1.wav" }], "original_audio_path": "song.mp3", "active_version": "v1" });
        let merged = merged_metadata(&stored, serde_json::json!({ "tags": ["b"], "bpm": 120 }));
        assert_eq!(merged["tags"], serde_json::json!(["b"]));
        assert_eq!(merged["bpm"], 120);
        assert_eq!(merged["active_version"], "v1");
        assert_eq!(merged["audio_versions"][0]["file"], "v1.wav");
        assert_eq!(merged["original_audio_path"], "song.mp3");
    }

    #[test]
    fn an_edit_neither_undoes_nor_makes_a_like() {
        let liked = serde_json::json!({ "liked": true, "liked_at": "100" });
        let merged = merged_metadata(&liked, serde_json::json!({ "liked": false, "tags": ["x"] }));
        assert_eq!(merged["liked"], true, "an older copy of the metadata does not take the like back");
        assert_eq!(merged["liked_at"], "100");
        let merged = merged_metadata(&serde_json::json!({ "tags": [] }), serde_json::json!({ "liked": true, "liked_at": "5" }));
        assert!(merged.get("liked").is_none() && merged.get("liked_at").is_none(), "only the like route sets it");
        let merged = merged_metadata(&serde_json::Value::Null, serde_json::json!({ "liked": true }));
        assert!(merged.get("liked").is_none(), "nor on a song with no metadata yet");
        let merged = merged_metadata(&serde_json::json!({ "liked": true, "liked_at": "100", "karaoke": { "file": "k.json" } }), serde_json::Value::Null);
        assert_eq!(merged["liked"], true, "an edit with no metadata leaves it as it is");
        assert_eq!(merged["karaoke"]["file"], "k.json");
    }

    #[test]
    fn a_like_is_kept_with_the_song_and_the_latest_comes_first() {
        let root = std::env::temp_dir().join(format!("library-liked-{}", uuid::Uuid::now_v7().simple()));
        let db = Library::open_at(root.join("library.sqlite"), root.join("media")).unwrap();
        let song = |title: &str| db.create_song(SongInput { title: title.into(), audio_path: None, caption: String::new(), lyrics: String::new(), metadata: serde_json::json!({ "tags": ["t"] }), generation_settings: serde_json::Value::Null, engine_id: "manual".into(), profile_id: None, replay_request: None, audio_codes: None, source: "manual".into() }).unwrap().id;
        let (first, second) = (song("first"), song("second"));
        assert!(db.liked_songs().unwrap().is_empty());
        db.set_song_liked(&first, true).unwrap();
        // the stamp is in seconds: the second like lands a second later
        let mut older = db.get_song(&first).unwrap().unwrap();
        older.metadata["liked_at"] = serde_json::json!("1");
        db.save_song(&older).unwrap();
        db.set_song_liked(&second, true).unwrap();
        let liked: Vec<String> = db.liked_songs().unwrap().into_iter().map(|song| song.id).collect();
        assert_eq!(liked, vec![second.clone(), first.clone()], "the latest like first");
        let stamp = db.get_song(&second).unwrap().unwrap().metadata["liked_at"].clone();
        db.set_song_liked(&second, true).unwrap();
        assert_eq!(db.get_song(&second).unwrap().unwrap().metadata["liked_at"], stamp, "liking again keeps the moment of the first like");
        db.set_song_liked(&second, false).unwrap();
        let taken_back = db.get_song(&second).unwrap().unwrap();
        assert!(taken_back.metadata.get("liked").is_none() && taken_back.metadata.get("liked_at").is_none());
        assert_eq!(taken_back.metadata["tags"][0], "t", "the rest of the metadata stays");
        assert!(db.set_song_liked("gone", true).unwrap().is_none());
        drop(db);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_stem_wears_the_cover_its_song_gets() {
        let root = std::env::temp_dir().join(format!("library-stem-cover-{}", uuid::Uuid::now_v7().simple()));
        let db = Library::open_at(root.join("library.sqlite"), root.join("media")).unwrap();
        let song = |title: &str, metadata: serde_json::Value| db.create_song(SongInput { title: title.into(), audio_path: None, caption: String::new(), lyrics: String::new(), metadata, generation_settings: serde_json::Value::Null, engine_id: "manual".into(), profile_id: None, replay_request: None, audio_codes: None, source: "manual".into() }).unwrap().id;
        let night = song("Night", serde_json::json!({}));
        let vocals = song("Night · vocals", serde_json::json!({ "derived": { "from": night, "from_title": "Night", "tool": "stems", "settings": { "stem": "vocals" } } }));
        let cover = song("Night · cover", serde_json::json!({ "derived": { "from": night, "from_title": "Night", "tool": "cover" } }));
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        db.store_song_cover(&night, &png, "image/png").unwrap();
        let stem = db.get_song(&vocals).unwrap().unwrap();
        let (path, media_type) = db.cover_path_for_song(&stem).expect("the stem has the song's cover");
        assert_eq!(fs::read(path).unwrap(), png);
        assert_eq!(media_type, "image/png");
        assert!(db.get_song(&cover).unwrap().unwrap().metadata.get("cover_filename").is_none(), "a cover version is a song of its own");
        drop(db);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_journal_keeps_its_latest_lines() {
        let root = std::env::temp_dir().join(format!("library-journal-{}", uuid::Uuid::now_v7().simple()));
        let db = Library::open_at(root.join("library.sqlite"), root.join("media")).unwrap();
        let line = |n: usize| JournalEntry { id: 0, at: String::new(), source: "agent".into(), tone: String::new(), verb: "created".into(), kind: "song".into(), target: format!("song {n}"), text: String::new() };
        for n in 0..502 {
            db.note_journal(&line(n)).unwrap();
        }
        let kept = db.journal(1000).unwrap();
        assert_eq!(kept.len(), 500, "the oldest lines go as new ones come");
        assert_eq!(kept[0].target, "song 501", "the newest first");
        assert!(!kept[0].at.is_empty(), "each line keeps its moment");
        assert!(db.remove_journal_entry(kept[0].id).unwrap());
        assert_eq!(db.journal(1).unwrap()[0].target, "song 500");
        db.clear_journal().unwrap();
        assert!(db.journal(10).unwrap().is_empty());
        drop(db);
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod media_tests {
    use super::*;

    #[test]
    fn a_library_whose_folder_moved_still_finds_its_songs() {
        let root = std::env::temp_dir().join(format!("library-moved-{}", uuid::Uuid::now_v7().simple()));
        let db = Library::open_at(root.join("library.sqlite"), root.join("media")).unwrap();
        fs::create_dir_all(root.join("media")).unwrap();
        fs::write(root.join("media").join("song.mp3"), b"ID3").unwrap();
        // stored while the portable folder sat on another drive
        let found = db.resolve_media(r"Z:\old place\data\media\song.mp3").unwrap();
        assert_eq!(found, root.join("media").join("song.mp3").canonicalize().unwrap());
        assert!(db.resolve_media(r"Z:\old place\data\media\gone.mp3").is_none());
        fs::write(root.join("outside.mp3"), b"ID3").unwrap();
        assert!(db.resolve_media(&root.join("outside.mp3").to_string_lossy()).is_none());
        let _ = fs::remove_dir_all(root);
    }
}
