---
name: minimax-music3-studio
description: Drive MiniMax Music3 Studio on this computer through its MCP server - write and make songs with MiniMax Music 3 (structured caption, lyrics), manage the library, draw covers, split stems, turn any track into MIDI, time karaoke, process audio, make video clips, play songs, install LoRA, and build a LoRA from a folder of songs end to end, laying the lyrics out yourself instead of the studio's small assistant. Sees and works the studio's window like a user. Use whenever the user asks for anything the studio does.
---

# MiniMax Music3 Studio through MCP

MiniMax Music3 Studio serves MCP at `http://127.0.0.1:8765/mcp` while it is open
(Streamable HTTP, stateless JSON-RPC). Every tool runs the same code as a button of the
studio, and the user sees what you do in the studio's window.

## If the studio is not running yet

1. It is a Windows desktop application. If it is not installed, download the installer or
   the portable archive from https://github.com/timoncool/MiniMax-Music3-Studio/releases/latest (it needs an
   NVIDIA card; the first start offers to download the models).
2. Start it. The MCP server is up as soon as its window is: `http://127.0.0.1:8765/mcp`.
   Nothing else to install - no npx, no bridge.
3. Connect (below), then call `studio_status`. If the models are missing, `models_catalog`
   and `models_download` fetch them.

## Connect

```bash
claude mcp add --transport http minimax-studio http://127.0.0.1:8765/mcp
```

Other clients: `{ "mcpServers": { "minimax-studio": { "type": "streamable-http", "url": "http://127.0.0.1:8765/mcp" } } }`.

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
- **You write, not the studio's assistant.** The studio has a small local model (Gemma)
  for users without an agent. You write better: read `writing_guide` and
  `writing_examples` first and write the caption and lyrics yourself. Use
  `assistant_write` only when the user asks for the studio's assistant.
  `assistant_sections` tags lyrics the user wrote with their sections without changing a
  word - the tag button of the create form.

## What MiniMax Music 3 reads - read `writing_guide` for the full rules

- **caption**: three parts in English, each under its heading alone on a line:
  ```
  Global Metadata
  Basic Attributes: bpm is 96. key is A, and scale is minor. <Genre / Subgenre>.
  Global Emotional Progression: ...
  Application Scenarios & Imagery: ...
  Sonics & Production Profile: ...
  Vocal Details
  Vocal Gender & Timbre: Singer A (Female), ...
  Vocal Style: ...
  Harmony/Backing Vocals: ...
  Vocal FX: ...
  Arrangement
  Instrument Lifecycle Description (Primary/Secondary Layering):
  Primary: ...
  Secondary: ...
  Groove & Foundation Progression: ...
  Embellishments, Textures & Spatial FX: ...
  ```
  Roughly 250-450 words, concrete and section by section. No song title, no lyric lines.
- **lyrics**: only `[intro] [verse] [pre-chorus] [chorus] [post-chorus] [bridge]
  [instrumental] [solo] [outro]`, each tag alone on its line, a blank line between
  sections, about 12-16 sung words per 10 seconds, in the song's own language. Russian `ё`
  stays `ё`. An instrumental keeps the structure with no words.
- **duration_seconds** is required for every song.
- `writing_examples` returns whole reference captions of MiniMax's own prompting skill
  for the genre family closest to your idea - match their shape and density.

## Recipes

**A song from an idea**

1. `writing_guide` topic `song`, `writing_examples` with the genre and mood.
2. Write the caption and lyrics yourself.
3. `song_create` (with `title`, `cover_prompt` and `duration_seconds`), then
   `studio_wait` with its `job_id`.
4. `player_play` with the new song's id to let the user hear it; `ui_screenshot` shows it.

**A LoRA from a folder of songs**

1. `training_status`. If the trainer or the listening pack is missing:
   `training_pack_install`, `training_listen_pack_install`.
2. `dataset_create` with the artist's name (the trigger word is made from it), then
   `dataset_add_folder` with the folder.
3. `dataset_prepare` with `lyrics: missing, style: missing, writer: agent`, then
   `studio_wait until: preparation`. The studio finds the lyrics in LRCLIB, QQ Music and
   Kugou, recognises only what they miss with Whisper, and has MOSS-Music caption every
   song by ear with the tempo and key measured. It leaves the lyric layout to you.
4. `dataset_get`. For each song:
   - `lyrics_state: found` - the words are there (from a database when `lyrics_source`
     names one: keep every word; `recognised`: fix the recogniser's mishearings). Lay them
     out in sections (`writing_guide` topics `sections` or `transcript`).
   - `style` holds MOSS's caption; correct what it got wrong (`writing_guide` topic
     `caption`), keeping its shape and the measured BPM and key.
   - Save with `dataset_song_update`; what you write is final and marks the song done.
   - `lyrics_state: wanted` after preparation: nothing found it. `lyrics_find` with other
     spellings, or ask the user, or write it instrumental.
5. `training_start` with `recipe_defaults` from `training_status` (stop `steps` or
   `epochs`). `studio_wait until: training`; `training_status` shows step, loss and
   checkpoints.
6. `training_checkpoint_install` for the chosen step, then `song_create` with that LoRA
   in `adapters` and its trigger word in the caption.
7. Not there yet after the run? `training_continue` with `steps` above the run's
   `resume_step` from `training_status`: it goes on from the state the run finished with,
   same recipe and songs, and stops at that step. Only LoRA-method runs that finished can
   go on (`resume_refused` says why not).

**The create page, where the user can see it**

`song_create` makes a song directly. When the user wants to watch and adjust it first:
`ui_navigate` create, `create_form_set` with the fields (the user sees them fill in),
`create_form_get` to check, and `create_form_submit` to press Create.

**When you are the studio's writing assistant**

The user can pick **Agent (MCP)** as the assistant engine. Then the studio's write buttons,
and the lyric layout of a dataset preparation, ask you instead of its local model:
`assistant_requests_wait` returns each request with the instructions and the answer schema
the local model would get; write the answer by them and send it with
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
  remove; **engine**: options,
  presets, restart, logs.
- **song**: create, defaults (what a field left out becomes), job get/list/cancel, replay.
- **writing**: guide, examples; **assistant**: write, status, set, runtime, models;
  requests wait and answer (when you are the assistant).
- **library**: songs list, song get/update/delete/files, import audio, versions;
  **playlist**: list/create/update/delete.
- **cover**: draw, set from file, templates, prompt render; **karaoke**: make, delete,
  settings; **recogniser**: install/remove; **stems**: split, get; **separator**: status,
  install, settings; **midi**: status, transcribe, get, delete, install, remove, cancel; **processing**: start, get, keep, discard, reference; **vst**.
- **lora**: list, install from the catalogue or Hugging Face, import files, update,
  delete.
- **dataset**: create, add folder or library songs, import, get, update, delete, song
  update/describe/delete/files, prepare (+ cancel, train after), reveal; **lyrics**: find;
  **training**: status, start, cancel, checkpoint install, run delete, packs.
- **ui**: screenshot, read page, click, type, select, press key, scroll, navigate, open
  settings, notify, console; **create_form**: get, set, submit; **player**: state, play, pause, seek, next, previous, set; **video**: open,
  get, set, render, play, pause, seek, close.
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
