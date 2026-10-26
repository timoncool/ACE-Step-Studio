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
- **What you change is written down where the user reads it.** Every change you make - a
  generation started, a song edited or deleted, a playlist, stems, karaoke, a dataset and its
  preparation, training, a LoRA, a model downloaded or removed, a setting - goes into the
  studio's Activity journal as what was done, to what and its name; a job is written as
  started. Reads, the window's own controls (`ui_*`, the player, the create form, the video
  editor, the visualizer) and answers that store nothing leave no line. A tool that writes
  over what was stored (a song or playlist edit, a dataset prepared again, stems made again,
  karaoke, MIDI, a cover) is marked destructive, so your client asks the user first.
- **A like lives with the song**: `library_liked` (the latest like first) and
  `library_song_like` work without the window open.
- **A stem belongs to its song**: the window shows it under the song it was separated
  from, `stems_split` on a song that has stems replaces the ones it makes again, and a stem
  itself is not split.
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
   Kugou, recognises only what they miss with CrisperWhisper, and has MOSS-Music describe every
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

**Listening: equalizer, visualiser, Winamp**

- The day's best: `library_songs_list` with `since: "today"`, `library_liked` (the user's
  thumbs-up), then `player_play` with `song_ids` plays them as the queue.

- Equalizer: `equalizer_get` names every preset; `equalizer_set` with `preset` ("Rock",
  "Vocal Booster"...), or `bands` / `preamp_db`, `balance`, `mono`, `panel_open`,
  `save_preset`. `.EQF` files: `equalizer_import`, `equalizer_export`.
- Visualiser: `visualizer_set` with `place` (panel, window), `fullscreen`, `engine`
  (milkdrop, spectrum), `preset` (`visualizer_presets` searches them), `look`, `step`.
- Winamp mode, the whole window as a Winamp 2 player: `winamp_set` `on: true` (with a
  `skin` from `winamp_skins`), `on: false` to come back. While it is on, `player_*` drive it;
  `winamp_set` also opens its windows, shades them, sets its equalizer and MilkDrop,
  its `scale` (1.2 = 120 %) and `skip_menu`.
  New skins: `winamp_museum` opens the museum, `winamp_skin_add` takes a downloaded `.wsz`.

**Anything the tools do not cover**

`ui_read_page` lists every control of the window with a ref; `ui_click`, `ui_type`,
`ui_select`, `ui_press_key` work it like the user; `ui_navigate` and `ui_open_settings`
move around. Check the result with `ui_screenshot`.

## Tools by area

- **studio**: `studio_status`, `studio_wait`, `studio_system`, `resources_rescan` (models and LoRAs put in the folders by hand), `studio_capabilities`, `studio_open_data_folder` (in Explorer, for the user), `runtime_status` (midi, training pack, training listen), `system_gpus` (the cards and the one in use).
- **settings**: `settings_get`, `settings_set`.
- **network**: `network_proxy_get`, `network_proxy_set` (no proxy, the system's or your own), `network_proxy_test`, `network_access_get`, `network_access_set` (opening the studio from other computers, a new key).
- **journal**: `journal_read` (the Activity page), `journal_clear`, `journal_remove`.
- **models**: `models_status`, `models_catalog`, `models_download`, `models_adopt` (files already on disk), `models_select`, `models_cancel_download`, `models_remove`.
- **engine**: `engine_options_get`, `engine_options_set`, `engine_restart`, `engine_presets_get`, `engine_preset_apply`, `engine_logs`, `diagnostics_get` (the diagnostics and their text for an issue), `diagnostics_report` (a bug-report zip in the data folder's reports).
- **song**: `song_defaults` (what a field left out becomes), `song_create` (playlist_id puts the made songs into a playlist), `song_plan` (ACE-Step's planner), `song_understand` (listen to a library song), `song_job_get`, `song_jobs_list`, `song_job_cancel`, `song_replay`.
- **writing**: `writing_guide`, `writing_examples`.
- **assistant**: `assistant_requests_wait`, `assistant_request_answer` (when you are the assistant), `assistant_sections`, `assistant_write`, `assistant_status`, `assistant_set`, `assistant_local_models`, `assistant_cli_agents` (Claude Code, Codex, opencode, Antigravity, Muse Code: installed or how to install), `assistant_runtime`, `assistant_model_install`, `assistant_model_remove`, `assistant_cancel_download`, `assistant_start`, `assistant_stop`.
- **library**: `library_songs_list` (since/until), `library_liked`, `library_song_like`, `library_song_get`, `library_song_update`, `library_song_delete`, `library_import_audio`, `library_file_tags_get`, `library_file_tags_set`, `library_song_files`, `library_version_select`, `library_version_delete`, `library_song_describe_style` (by ear: the caption, tempo, key and metre of a recording that came without them), `library_song_note_set` (the song's note).
- **playlist**: `playlist_list`, `playlist_create`, `playlist_update`, `playlist_delete`, `playlist_get`.
- **cover**: `cover_draw`, `cover_templates_get`, `cover_templates_set`, `cover_set_from_file`, `cover_prompt_render`, `cover_look_get`, `cover_look_set` (the photo or pattern of tracks without a cover, unique: each track's pattern drawn from its words and music style, kept in the file or not), `cover_scenes`, `cover_photos_search`, `video_backgrounds_search`, `song_cover_photo_set`, `song_cover_pattern_set`.
- **karaoke**: `karaoke_make`, `karaoke_settings_get`, `karaoke_settings_set`, `recogniser_install`, `recogniser_remove`, `recogniser_cancel_download`, `karaoke_delete`.
- **stems**: `stems_split`, `stems_get`.
- **separator**: `separator_status`, `separator_runtime`, `separator_runtime_install`, `separator_cancel_download`, `separator_install`, `separator_remove`, `separator_settings_get`, `separator_settings_set`.
- **midi**: `midi_status`, `midi_transcribe`, `midi_get`, `midi_delete`, `midi_install`, `midi_remove`, `midi_cancel`, `midi_set` (a .mid of your own).
- **processing**: `processing_start`, `processing_get`, `processing_keep`, `processing_discard`, `processing_set_reference`.
- **vst**: `vst_list`, `vst_scan`, `vst_open_editor`.
- **lora**: `lora_list`, `lora_install_catalog`, `lora_search_hf`, `lora_hf_files`, `lora_install_hf`, `lora_update`, `lora_delete`, `lora_import_folder` (every LoRA of a folder, linked without a copy), `lora_import_files`, `lora_cancel_download`, `lora_export_comfyui` (a trained LoRA as one file for ComfyUI's native ACE-Step 1.5).
- **dataset**: `dataset_import_folder`, `dataset_reveal`, `dataset_song_files`, `dataset_prepare_train_after`, `dataset_get`, `dataset_create`, `dataset_add_folder`, `dataset_add_library_songs`, `dataset_update`, `dataset_delete`, `dataset_song_update`, `dataset_song_describe`, `dataset_song_delete`, `lyrics_find`, `dataset_prepare`, `dataset_take_as_is` (no assistant: found lyrics kept as they are), `dataset_prepare_cancel`.
- **training**: `training_pack_cancel`, `training_status`, `training_pack_install`, `training_listen_pack_install`, `training_start`, `training_continue`, `training_cancel`, `training_checkpoint_install`, `training_run_delete`.
- **ui**: `ui_screenshot`, `ui_read_page`, `ui_click`, `ui_type`, `ui_select`, `ui_press_key`, `ui_scroll`, `ui_navigate`, `ui_open_settings`, `ui_notify`, `ui_console`.
- **create form**: `create_form_get`, `create_form_set`, `create_form_submit`.
- **player**: `player_state`, `player_play`, `player_pause`, `player_seek`, `player_next`, `player_previous`, `player_queue_add`, `player_set` (repeat none, all, one, or stop after the track).
- **equalizer**: `equalizer_get`, `equalizer_set`, `equalizer_import`, `equalizer_export`.
- **visualizer**: `visualizer_get`, `visualizer_set`, `visualizer_presets`.
- **winamp**: `winamp_get`, `winamp_set`, `winamp_skins`, `winamp_skin_add`, `winamp_museum`.
- **video**: `video_open`, `video_get`, `video_set`, `video_render`, `video_play`, `video_pause`, `video_seek`, `video_close`.
- **openrouter**: `openrouter_status`, `openrouter_set_key`, `openrouter_catalog`, `openrouter_catalog_refresh`, `openrouter_log`, `openrouter_complete`, `openrouter_cover`, `openrouter_transcribe`.

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
