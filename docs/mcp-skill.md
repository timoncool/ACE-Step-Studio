---
name: ace-step-studio
description: Drive ACE-Step Studio on this computer through its MCP server - write and make songs with ACE-Step 1.5 (caption, lyrics, metadata), let its own language model plan or listen to a song, make covers, remixes, repaints and stems of a track, manage the library, draw covers, split stems, turn any track into MIDI, time karaoke, process audio, make video clips, play songs, install LoRA from the catalogue or Hugging Face, and build a LoRA from a folder of songs end to end, writing the captions and lyrics yourself instead of the studio's small assistant. Sees and works the studio's window like a user. Use whenever the user asks for anything the studio does.
---

# ACE-Step Studio through MCP

ACE-Step Studio serves MCP at `http://127.0.0.1:8792/mcp` while it is open
(Streamable HTTP, stateless JSON-RPC). Every tool runs the same code as a button of the
studio, and the user sees what you do in the studio's window.

## If the studio is not running yet

1. It is a Windows desktop application. If it is not installed, download the installer or
   the portable archive from https://github.com/timoncool/ACE-Step-Studio/releases/latest (it needs an
   NVIDIA card; the first start offers to download the models).
2. Start it. The MCP server is up as soon as its window is: `http://127.0.0.1:8792/mcp`.
   Nothing else to install - no npx, no bridge.
3. Connect (below), then call `studio_status`. If the models are missing, `models_catalog`
   and `models_download` fetch them.

## Connect

```bash
claude mcp add --transport http ace-step-studio http://127.0.0.1:8792/mcp
```

Other clients: `{ "mcpServers": { "ace-step-studio": { "type": "streamable-http", "url": "http://127.0.0.1:8792/mcp" } } }`.

The server also serves this skill (resource `studio://skill`, prompt `studio`) and the
writing guides (resources `studio://guide/<topic>`). It speaks MCP `2026-07-28` (stateless:
every request carries its version in `_meta`, `server/discover` describes the server) and
the handshake revisions `2025-11-25`, `2025-06-18` and `2025-03-26` through `initialize`.
Only this computer's agents and the studio's own window may connect.

The user sees it in the studio too: Settings, **Agent (MCP)** shows whether an agent is
connected and the address to paste.

## Ground rules

- **Start with `studio_status`.** It tells what runs now and whether the window is open.
- **Long work is a job**: songs, stems, MIDI, karaoke, preparation, training. Start it, then
  `studio_wait` (a `job_id`, or `until: stems | midi | processing | covers_and_karaoke |
  song_jobs | preparation | training | idle`) instead of
  polling. It returns within a minute (30 s by default, 55 at most) with how far the work got; call
  it again.
- **One heavy job holds the graphics card at a time.** While a LoRA trains no song is
  made; start training last.
- **Answers are short by default**: a song job is its status and the songs it made, a
  library song leaves out its audio codes, `lora_list` gives one line per LoRA. Pass
  `response_format: detailed` when you need every field.
- **Covers are drawn only with an image model set up** (`settings_get`, covers; an
  OpenRouter key). Without one a `cover_prompt` is kept but no cover appears;
  `cover_set_from_file` still works.
- **Look ids up, never guess them**: `library_songs_list`, `training_status`,
  `dataset_get`, `lora_list`, `models_status`.
- **Files on this computer are passed by path**: `dataset_add_folder`,
  `library_import_audio`, `cover_set_from_file`, `video_set`. `library_song_files` and
  `dataset_song_files` give the paths of the studio's own files.
- **One writer per song.** Either you write the caption and lyrics, or ACE-Step's own
  language model does (`think` on with empty lyrics, or `song_plan`) - never both on one
  song. You write better: read `writing_guide` and `writing_examples` first. Use
  `assistant_write` only when the user asks for the studio's assistant.
  `assistant_sections` tags lyrics the user wrote with their sections without changing a
  word - the tag button of the create form.

## What ACE-Step reads - read `writing_guide` for the full rules

ACE-Step 1.5 is two models: a language model (the planner) that can write the song and plan
its structure as audio codes, and the DiT that renders the sound. A song is a caption,
lyrics and metadata in fields of their own.

- **caption**: the sound, in English - comma-separated tags (18-28 for electronic genres,
  12-18 for acoustic ones), 2-4 plain sentences the way ACE-Step's own examples are
  written, or both. Genre and subgenres, mood, key instruments, vocal type, timbre,
  production, era. Never tempo, key, time signature or length in the caption - they have
  their own fields - and never the story: the caption is heard as sound.
- **lyrics**: sections tagged `[Intro]`, `[Verse 1]`, `[Pre-Chorus]`, `[Chorus]`,
  `[Bridge]`, `[Drop]`, `[Outro]`..., each tag alone on its line, a blank line between
  sections, 6-10 syllables per line. A tag may carry one refinement after a dash:
  `[Chorus - anthemic]`, `[Bridge - whispered]`. `UPPERCASE` for shouted lines,
  `(parentheses)` for backing vocals. Plain text in the song's language, no per-line
  language prefixes. An instrumental is exactly `[Instrumental]`.
- **Metadata**: `bpm`, `keyscale` ("A minor"), `timesignature` ("4"), `duration` in
  seconds, `vocal_language`. What you leave out the planner fills in when `think` is on.
- **think**: on, the planner plans the song's structure as audio codes before the DiT
  renders (`audio_cover_strength` sets the share of DiT steps that follow that plan). With a
  LoRA of the DiT (a sound LoRA) turn it off: ACE-Step's authors advise against the
  planner's codes with a trained LoRA, and the studio does the same.
- `writing_examples` returns ACE-Step's official example requests closest to your brief -
  match their shape and density.

## Recipes

**A song from an idea**

1. `writing_guide` topic `song`, `writing_examples` with the genre and mood.
2. Write the caption, lyrics and metadata yourself.
3. `song_create` (with `title`, `cover_prompt`, `duration`, `bpm`, `keyscale`), then
   `studio_wait` with its `job_id`.
4. `player_play` with the new song's id to let the user hear it; `ui_screenshot` shows it.

**A song written by ACE-Step's own planner**

`song_plan` with mode `inspire` and a one-line idea in `request.caption` returns a whole
request (caption, lyrics, metadata), written on the card; `song_create` takes it as it is.
Mode `format` completes the metadata of a caption and lyrics you already have.

**Covers, remixes and edits of a library song**

`song_create` with `source_song_id` and `task_type`: `cover` (new style, same melody;
`audio_cover_strength` how closely it follows), `cover-nofsq` (remix), `repaint`
(`repainting_start`/`repainting_end` in seconds), and with a base model `lego`, `extract`
(one `track`) or `complete` (tracks joined by `" | "`). `song_understand` listens to a
library song and returns its caption, lyrics, metadata and audio codes, which a new render
can follow through `audio_codes`.

**A LoRA from the catalogue or Hugging Face**

`lora_list` shows what is installed and the curated catalogue; `lora_install_catalog`
installs an entry. `lora_search_hf` searches Hugging Face (most liked first),
`lora_hf_files` lists a repository's files with the model size each fits (`2b` or `xl`)
or why the engine cannot use it, and `lora_install_hf` downloads one. A LoRA fits one DiT
size: use it with a DiT of that size (`models_status` shows the current one), and put its
trigger word in the caption.

**A LoRA from a folder of songs**

1. `training_status`. If the trainer or the listening pack is missing:
   `training_pack_install`, `training_listen_pack_install`. `base` names the unquantised
   DiT the run trains on; if it is not installed, `models_download` with `ids` from
   `base.download` and `select: false`.
2. `dataset_create` with the artist's name (the trigger word is made from it), then
   `dataset_add_folder` with the folder.
3. `dataset_prepare` with `lyrics: missing, style: missing, writer: agent`, then
   `studio_wait until: preparation`. The studio finds the lyrics in LRCLIB, QQ Music and
   Kugou, recognises only what they miss with Whisper, and has MOSS-Music describe every
   song by ear with the tempo and key measured. It leaves the lyric layout to you.
4. `dataset_get`. For each song:
   - `lyrics_state: found` - the words are there (from a database when `lyrics_source`
     names one: keep every word; `recognised`: fix the recogniser's mishearings). Lay them
     out in sections (`writing_guide` topics `sections` or `transcript`).
   - `style` holds the caption written by ear; correct what it got wrong (`writing_guide`
     topic `caption`), keeping the measured BPM and key in their own fields.
   - Save with `dataset_song_update`; what you write is final and marks the song done.
   - `lyrics_state: wanted` after preparation: nothing found it. `lyrics_find` with other
     spellings, or ask the user, or write it instrumental.
5. `training_start` with `recipe_defaults` from `training_status`: `epochs` and
   `target_loss` (the run stops earlier once the loss reaches it and keeps the best
   adapter). `studio_wait until: training`; `training_status` shows the epoch, loss and
   checkpoints.
6. `training_checkpoint_install` for the chosen checkpoint, then `song_create` with that
   LoRA in `adapters`, its trigger word in the caption and `think: false`.
7. Not there yet after the run? `training_continue` with `steps` - the epochs to reach in
   all - above the run's `resume_step`: it goes on from the adapter the run exported, same
   recipe, songs and DiT (`resume_refused` says why not).

**The create page, where the user can see it**

`song_create` makes a song directly. When the user wants to watch and adjust it first:
`ui_navigate` create, `create_form_set` with the fields (the user sees them fill in),
`create_form_get` to check, and `create_form_submit` to press Create.

**When you are the studio's writing assistant**

The user can pick **Agent (MCP)** as the assistant engine. Then the studio's write buttons,
the simple form and the lyric layout of a dataset preparation ask you instead of its local
model: `assistant_requests_wait` returns each request with the instructions and the answer
schema the local model would get; write the answer by them and send it with
`assistant_request_answer`. Keep calling `assistant_requests_wait` while the user works -
`studio_status` shows `assistant_requests_waiting`. A request waits 15 minutes.

**Talking to the user**

`ui_notify` shows the user a short message in the window. `ui_console` shows the errors the
window logged, when a button did nothing.

**A video clip**

The editor works in the studio's window, which must be visible while you edit and render:
a minimised window or a hidden tab holds the preview and the render.

1. `video_open` with a song id, `video_get` to see the presets and settings.
2. `video_set`: preset, aspect ratio, colours, effects, text layers, karaoke lyrics,
   a background picture or video from a path. `video_seek` and `ui_screenshot` to look.
3. `video_render`, then `video_get` until `export.saved` names the MP4 (or `export.error` says why not).

**Anything the tools do not cover**

`ui_read_page` lists every control of the window with a ref; `ui_click`, `ui_type`,
`ui_select`, `ui_press_key` work it like the user; `ui_navigate` and `ui_open_settings`
move around. Check the result with `ui_screenshot`.

## Tools by area

- **studio**: status, wait, system, capabilities, open data folder; **settings** get/set.
- **models**: status, catalog, download, adopt (files already on disk), select, cancel,
  remove; **engine**: options, presets, restart, logs.
- **song**: create, plan (ACE-Step's planner), understand (listen to a library song),
  defaults (what a field left out becomes), job get/list/cancel, replay.
- **writing**: guide, examples; **assistant**: write, sections, status, set, runtime,
  models; requests wait and answer (when you are the assistant).
- **library**: songs list, song get/update/delete/files, import audio, versions;
  **playlist**: list/create/update/delete.
- **cover**: draw, set from file, templates, prompt render; **karaoke**: make, delete,
  settings; **recogniser**: install/remove; **stems**: split, get; **separator**: status,
  install, settings; **midi**: status, transcribe, get, delete, install, remove, cancel;
  **processing**: start, get, keep, discard, reference; **vst**.
- **lora**: list, install from the catalogue, search Hugging Face, list a repository's
  files, install from Hugging Face, import files, update, delete.
- **dataset**: create, add folder or library songs, import, get, update, delete, song
  update/describe/delete/files, prepare (+ cancel, train after), reveal; **lyrics**: find;
  **training**: status, start, continue, cancel, checkpoint install, run delete, packs.
- **ui**: screenshot, read page, click, type, select, press key, scroll, navigate, open
  settings, notify, console; **create_form**: get, set, submit; **player**: state, play,
  pause, seek, next, previous, set; **video**: open, get, set, render, play, pause, seek,
  close.
- **openrouter**: status, key, catalog, log, complete, cover, transcribe.

## Turn a track into MIDI

1. `midi_transcribe` with `song_id` - a song, a stem, a processed take - or `path` of any
   audio file; `size` small, medium (default) or large. The transcriber and the model are
   downloaded the first time (0.1 GB plus 0.4, 1.2 or 5.5 GB); `midi_install` fetches them
   ahead. It runs on the card: NVIDIA from GTX 16 and RTX 20 on, driver 580 or newer.
2. `studio_wait until: midi`.
3. `midi_get` names the .mid on this computer, its model and instruments (34 groups and
   drums); `response_format: detailed` gives every note. `library_song_files` lists it too.
   A file named by path is written to the studio's `midi` folder (`midi_status` run.file).
4. The weights are MuScriptor by Kyutai & Mirelo, CC BY-NC 4.0: say so when the user wants
   the MIDI for commercial work.
