# Changelog

What changed, newest first. Dates are release dates; the studio is versioned by its
Windows build.

## 2026-09-25 — 2.1.1

### Fixed

- **Create video everywhere.** The song menu of the Library page, of the song details and of
  the player had no Create video (nor Re-render, Reuse prompt or Delete in some of them): each
  place built its own menu. There is one song menu now, the same wherever it opens, and Create
  video is also a button on every track row and in the song details.
- **A menu near the bottom of a panel** opens upward, or the panel scrolls it into view,
  instead of hiding its last items under the edge.

## 2026-09-25 — 2.1.0

### Added

- **Train further.** A finished run that is not there yet goes on from where it stopped:
  set the steps to reach and press **Train further** on the run. The same recipe and songs,
  the optimizer as it was; the loss chart and the checkpoints continue instead of starting
  over. Runs trained with the LoRA method only - the trainer keeps no state for PiSSA and
  HOT-PiZZA to go on from, and the run card says so. MCP: `training_continue`.
- **Section tags for your own lyrics.** The tag button beside the lyrics lays them out in
  [verse], [chorus], [bridge]... without changing a word: the assistant only says where each
  section starts, and the lines go under the tags as written. MCP: `assistant_sections`.

### Fixed

- **Renaming a track.** The pencil in the song details and the title in the library did
  nothing for most tracks: songs of the local library carried no owner, so the studio took
  them for someone else's. Every track can be renamed from both places now, and the library
  row shows a pencil on hover.
- **Models kept in several folders.** **Use models I already have** looked only at the top
  of the folder you pick; it now searches its subfolders too (hidden ones aside), off the
  window's thread.
- **Wider side panels.** The create panel and the song details stretch up to 1200 px on a
  wide screen, never past 40% of the window; a double click on the edge puts the default
  width back, and the arrow keys move the focused edge.

## 2026-09-25 — 2.0.0

### Added

- **Any track to MIDI.** A song, a stem or a processed take becomes multi-instrument MIDI -
  34 instrument groups and drums, each on its own channel - with MuScriptor (Kyutai & Mirelo)
  on the GPU through HOT-Step's native port. It is on the tools page and in every track's
  menu (**To MIDI**); a piano roll fills in while it listens, the MIDI plays against the
  original with a crossfade and per-instrument mute and solo, and the .mid is kept beside the
  track and saved from there. Nothing is installed up front: the transcriber (126 MB) and the
  chosen model - small 0.4 GB, medium 1.2 GB, large 5.5 GB - download the first time, or
  ahead from the tools page. The weights come from an open mirror of the official files, so
  no Hugging Face sign-in is needed; they are CC BY-NC 4.0, for non-commercial use. MCP:
  `midi_transcribe`, `midi_get`, `midi_status`, `midi_install`, `midi_remove`,
  `midi_delete`, `midi_cancel`, and `studio_wait until: midi`.
- **Tracks made by tools are tracks of the library.** Stems, a kept processing and a re-render
  are new tracks linked to the one they were made from: the list says **Made from «...»**
  with the tool, the click opens the original, and the track keeps the tool's settings.
  Splitting a song again replaces its stems instead of adding more.
- **An MCP server in the studio.** `http://127.0.0.1:8765/mcp` gives an agent 159 tools: every
  route of the studio's API, called inside the process, and the window itself - a
  screenshot, its controls, the player and the video editor - through a bridge the page
  answers. Files are passed by their path; MiniMax's caption rules and reference captions
  are tools, resources and prompts, so a connected agent writes instead of the studio's
  small assistant. `docs/mcp-skill.md` is the skill an agent reads.
- **The agent as the studio's assistant.** Pick **Agent (MCP)** as the writing assistant and
  the write buttons and a dataset preparation ask the connected agent what they would ask
  the local model, with the same instructions and answer schema. Settings has an **Agent
  (MCP)** page: whether an agent and the window are connected, the address and the lines to
  paste into Claude Code or any other client.
- **MCP 2026-07-28.** The server speaks the stateless revision (`server/discover`, per-request
  `_meta`, `Mcp-Method`/`Mcp-Name` headers checked against the body, cacheable lists,
  structured results) and the handshake revisions for older clients, and answers only this
  computer's agents and its own window. An agent reads and fills the create page's form,
  sees every control of the window with its label and the song it belongs to, shows the user
  a message and reads the window's console. `llms.txt` and the README tell an agent given
  the repository how to install, connect and start.
- **A dataset in one drop.** The training page is a three-step wizard: drop a folder of
  songs, check them, train. Albums with a cue sheet are cut into songs; titles and artists
  come from the tags, the file name and the folders.
- **Lyrics from the databases players use.** LRCLIB, then QQ Music, then Kugou, matched by
  title, artist and length, kept word for word. Only a song none of them knows has its
  vocals separated and is heard by Whisper, which is told the language the found lyrics are
  in and has its usual hallucinations (subtitle credits, captions of sounds, 674 known
  phrases in 11 languages) filtered out.
- **Sections without touching the words.** For a published sheet the assistant only says
  where each section starts; the studio puts the sheet's own lines under the tags, so no
  line can be lost or merged. Lines sung more than once are marked so the chorus stands out.
- **Every song shows where it is.** Lyrics and descriptions appear as each song is done,
  with the stage and its count on top. One model is on the card at a time, each loaded once
  for the whole batch.
- **Picks up after a restart.** Each song keeps its lyrics and style state in the dataset,
  and the job itself is kept on disk: after a crash or a restart the preparation carries on
  by itself and redoes nothing. A failed or unfinished song has a button that finishes just
  that song.
- **A trigger word from the start.** Every dataset gets a rare word made from its name
  (`nrmnkhffn` for "Нейромонах Феофан"); a word the user clears stays cleared.
- **README lists every model** the studio downloads — training, listening, lyrics, stems,
  assistant — with its direct link, size and the folder it goes in.
- **Stop by steps or by epochs.** A fixed number of steps, or a number of passes over the
  songs.
- **Describing by ear on the card.** MOSS-Music listens to every song and writes its
  structured caption; tempo and key are measured by Beat This! and S-KEY on the card, loaded
  once.

### Fixed

- **Resampling to 16 kHz is band-limited.** Every reader of 16 kHz audio - Whisper, Parakeet,
  and now MIDI - got the audio through linear interpolation, which folds everything above
  8 kHz back into the band as noise: MuScriptor heard a clean vocal as distorted guitar and
  drums. The studio now filters before it decimates.
- `studio_wait` until idle waited for songs, the preparation and training only; it now waits
  for stems, MIDI, covers, karaoke and processing too, and refuses a `job_id` that names no
  job instead of saying "still running" forever.
- A LoRA passed to `song_create` without strengths ran at zero on every slot; it now starts
  where the create page starts it - its own strengths, else full on every slot it touches.
- A song's delete names the files it could not remove (a player holding one) instead of
  only logging them. MCP names the studio's own version.
- `video_set` and `create_form_set` refuse a field or a value the window does not have
  (`aspectRatio`, not `aspect_ratio`), instead of saying "Set" and changing nothing.
- A kept processing is named by its label; a render started by an agent can no longer leave
  the next manual export going to the studio's folder.
- The assistant's JSON schema reached llama-server in a field it does not read, so local
  answers were never held to it; it now goes where llama-server reads it.
- The engine watcher no longer starts the music engine, and with it unloads the assistant,
  while a preparation or a training run holds the card.
- A song deleted during a preparation fails alone instead of stopping the job.
- A title that starts with a number keeps it ("99 Luftballons").

## 2026-09-24 — 1.6.2

### Fixed

- **Runs on every NVIDIA card from the GTX 900 series on.** For some cards the engine carried
  only PTX, which a driver older than CUDA 13 cannot compile, and the first song failed with
  "PTX was compiled with an unsupported toolchain". The studio now ships two CUDA builds of
  the engine with compiled code for every architecture, and picks the one the card and its
  driver run: CUDA 13 for Turing and newer (GTX 16, RTX 20–50, Tesla T4, A100, RTX A-series,
  L4/L40, H100) with driver 580 or newer; CUDA 12 for Maxwell, Pascal and Volta (GTX 900/1000,
  Titan X/Xp/V, Tesla M40, P40, P100, V100) and for any card on a driver from 525 to 579. The
  cuBLAS of that build is downloaded once, as before.
- **Cards before Ampere** get the engine's FP16 clamp on their own: their tensor cores
  accumulate in FP16, which can overflow into silence.

### Engine

- minimaxmusic.cpp 120a4f6: backends load at run time and the studio names the CUDA build.
  The audio is the same to the byte.

## 2026-09-24 — 1.6.1

### Fixed

- **The create page keeps what was typed in it.** Leaving it for the library, search or any
  other page reset it to the defaults: the style, the lyrics, the score and every setting
  were lost. The page now stays as it was left
  ([#1](https://github.com/timoncool/YuE2-Studio/issues/1)).
- **Songs play after the studio's folder moves.** The library kept each song's full path,
  so a drive that came back under another letter after a restart, or a portable folder
  copied elsewhere, left every song saying it was no longer available while the files
  sat in the media folder. Songs are now found by name in the studio's own media folder.
- **Errors say why.** Stem separation, karaoke and cover art showed only the first line
  of a failure ("load the separation model ..."), without its cause; the whole reason is
  shown now. When the graphics card cannot load the separation model, the message says
  to choose the processor instead.

### Added

- **Select everything in the LoRA catalogue** that is not downloaded yet, in one click,
  and download it as one set.

## 2026-09-24 — 1.6.0

### Added

- **LoRA.** A LoRA page with the installed files, a catalogue of ready sliders by ntc-ai
  and a search on Hugging Face that downloads what you pick. In the create form each LoRA
  gets its own strength for the language model (the composition) and for the DiT (the
  sound), and its trigger word goes into the caption for you. The engine merges LoRA and
  LoKr into either half at load and reads PEFT, LyCORIS, diffusers and ComfyUI files
  (minimaxmusic.cpp fork `adapters`, 12534e3), with the rsLoRA scale honoured.
- **Training your own LoRA.** An optional tab on the LoRA page. 5–20 songs of one artist
  or style become a language-model LoRA on your card with HOT-Step's `mm3-lm-train` and
  its HOT-PiZZA recipe: rank 128, AdamW at 8e-5 with warm-up, a 1536-frame window that
  fits a 24 GB card, the depth decoder's acoustic loss, a checkpoint every 100 steps.
  Every setting is editable under Advanced, with the defaults one click away. The
  assistant writes each song's caption by ear, in MiniMax's own structure, and each
  checkpoint goes into the LoRA library in one click. The trainer and its weights (about
  10.5 GB) download only when you open training; it needs an RTX 30-series card or newer
  with 22 GB of VRAM.
- **Datasets travel between studios.** A dataset is a folder with `dataset.json` and its
  audio; import one from YuE2 Studio or show the folder to take it there.
- **Audio processing.** Noise reduction, the Spectral Lifter, a vocal naturaliser, your own
  VST3 plugins in a chain, and mastering to a reference track, from a track's menu or the
  Tools page. Plugins are found in the system VST3 folders, each is set up in its own
  window, and they run in a host process of their own, so a plugin that crashes does not
  take the studio with it. Compare before and after while it plays; keep the result as a
  version of the track, next to the untouched original, or throw it away.

### Changed

- **MP3 is made by the studio.** The engine renders 32-bit float and the studio encodes
  the MP3 with LAME at 320 kbps, so nothing is lost before the encoder.
- **Fewer DiT steps keep their detail.** Below 30 steps the engine raises the flow shift
  by itself, to `29/(steps-1)`.

### Fixed

- **The Light and Minimal model sets make songs.** Their lighter files come from a second
  community set written in llama.cpp's naming, with the DiT's q, k and v in one matrix, and
  the engine read only the original naming: it stopped on the first tensor, and the
  language model of those sets was not even recognised. The engine now reads both layouts
  as the same weights; a render on the Light DiT matches the Q8_0 one to 0.985.
- **Clearing the create form, resetting its parameters and opening a saved prompt work
  again.** Each of them stopped the form on a setting that had been removed.
- **Broken engine output is no longer saved as silence.** A NaN in the rendered audio
  became the peak the MP3 was normalised to and turned the whole track silent; the song
  now fails with a message saying so.

## 2026-09-24 — 1.5.2

### Changed

- **The engine follows minimaxmusic.cpp up to 448e880.** It draws the initial noise the way
  the reference does, keeps the float fields of a request exact, fixes a Vulkan read of
  the batch stride, stops with a clear error when its port is already taken, and brings
  ggml up to date.
- **Newer runtimes for the add-ons.** The assistant downloads llama.cpp b11146 (CUDA 13.4)
  instead of b9966, and karaoke downloads ONNX Runtime 1.30.0 instead of 1.24.2. Add-ons
  already installed keep working on the versions they have.
- **A new studio in the family.** The news page announces YuE2 Studio, which grew out of
  this one, with links to it and to ACE-Step Studio.

### Fixed

- **The audio editor's waveform library is now in the repository.** The folder was ignored
  by git, so a clean checkout built an editor that opened blank; released builds were not
  affected. A test now checks that every file the editor loads is built in.

## 2026-08-19 — 1.4.0

### Added

- **The studio can read audio, not only write it.** MiniMax published Music 3
  without the encoder that turns audio into the codes the model generates, so a
  finished track could never be handed back to it. The encoder was reconstructed
  by the community; it is exported to ONNX and published as
  [nerualdreming/open-rvq-encoder-minimax-music3-169m-v4-onnx](https://huggingface.co/nerualdreming/open-rvq-encoder-minimax-music3-169m-v4-onnx),
  verified against the PyTorch reference with every code identical. The path
  runs on parts already here: `neural-codec` from the engine turns audio into
  VAE latents, ONNX Runtime turns those into codes, and the engine renders from
  them. What that gives today is a re-render of a whole track through the model.
  Continuing past the end and repainting one section need the engine to accept
  codes as a prefix or a masked span, and its replay path takes neither.
- **Lighter quantisations for every role**, from a second community set: the
  language model down to Q4_K_M, Q4_K_S and Q3_K_M, the DiT to Q4_K_S and
  Q3_K_M, the depth decoder to Q4/Q5/Q6, plus MXFP4 and NVFP4 for all three -
  ggml declares both types and the CUDA backend carries kernels for them.
- **A Minimal profile** at about 6.4 GB, for 8 GB cards that were previously
  told to use the cloud, and Light drops from 8.8 GB to 7.7 GB.
- **Models you already have can be adopted** instead of downloaded again:
  a folder picker matches files by name, then by exact size, and hard-links
  them into place.
- **Six Whisper models and both Parakeet precisions**, matching Dub Studio's
  line-up, with the Gemma quantisations alongside them.

## 2026-08-19 — 1.3.5

### Fixed

- **An instrumental was sung.** The lyrics box keeps what was in it, so the
  words of the previous track went to the engine even with the instrumental
  switch on. An instrumental now submits no lyrics at all.

- **A clean installation never started.** The engine was launched once, at
  startup, and only if a complete set of weights was already on disk - so a
  first install downloaded its models, nothing started them, and the window
  waited on "loading the models into memory" until the studio was restarted by
  hand. The supervisor now watches: whenever the weights are there and nothing
  answers on the engine port, it brings the engine up. The same gap swallowed a
  crashed engine.
- **A 24 GB card was recommended a 26.6 GB set.** The tiers were round numbers;
  they are now the sets' own weights, so a 4090 is pointed at Quality Q8 and
  the full native set is only recommended from 30 GB.

### Changed

- **Six Whisper models instead of two** — tiny, base, small, medium, large-v3
  and large-v3-turbo, the same line-up Dub Studio offers.
- **The OpenRouter key and a self-hosted server address are typed where they
  are needed**, in the group that uses them, rather than on another page.
- **Every local engine is now a choice, not a list of files.** Karaoke offers
  Parakeet, Whisper or OpenRouter and what to run it on, and one button
  installs or removes the whole set - a runtime and five model files are one
  recogniser, not six decisions. The card is the default.
- Every model row states the same things in the same order - what it is, what
  it weighs, whether it is installed, which variant - with download and remove
  beside it, the way Dub Studio states it.
- The controls are shared components now. The settings panels had each drawn
  their own tabs, inputs and buttons, so the same decision looked like a
  different control depending on the page.
- A partial settings request no longer replaces the whole assistant
  configuration: sending a provider used to blank the model, the path and the
  reasoning effort.

## 2026-08-19 — 1.3.4

### Fixed

- **An installed studio kept its models on the system drive.** Portability was
  decided by a `portable.flag` file that only the portable archive ever
  contained, so installing into `F:\AI` still put twenty-five gigabytes of
  weights, the library and the media into `%LOCALAPPDATA%`. The studio now
  keeps its data beside its own executable whenever it can actually write
  there - it tries, rather than reasoning about permissions - and falls back to
  the profile only for a read-only location such as Program Files.
- **The engine started before its libraries had finished downloading.**
  Fetching cuBLAS only queued the download and returned at once, so the engine
  was launched into a folder that did not have it yet and failed exactly as if
  nothing had been fetched. The install now waits, and reports the failure if
  the files do not arrive.
- Automatic cover art was on by default, which does nothing without an
  OpenRouter key and costs money with one. It is off until switched on.
- The OpenRouter catalogue was requested every time the providers page opened,
  with or without a key, so people without one watched a spinner and then read
  an error they could do nothing about. It is only fetched once a key is saved.

## 2026-08-19 — 1.3.3

### Fixed

- **The installed executable had a different name from the portable one.** The
  installer wrote `minimax-music3-studio-desktop.exe`, after the Rust crate,
  while the portable build named the same binary `MiniMax-Music3-Studio.exe`:
  one studio under two names depending on how it arrived. Both are now
  `MiniMax-Music3-Studio.exe`, and the installer deletes the old name so an
  updated folder is not left with a dead 52 MB copy and a shortcut pointing at
  whichever was clicked first.

## 2026-08-19 — 1.3.2

### Fixed

- **The engine could not start on a machine without the CUDA Toolkit.**
  `mm-server.exe` loads `ggml.dll`, which loads `ggml-cuda.dll`, which imports
  `cublas64_13.dll` and through it `cublasLt64_13.dll` — all static imports,
  resolved by Windows before the engine's own code runs. Neither library was
  shipped, so the process died in the loader with "cublas64_13.dll was not
  found" and no fallback to the processor was possible. The studio now installs
  them itself, from NVIDIA's own redistributable archive, beside the engine
  binary where the loader looks first. A machine that already has them — a CUDA
  Toolkit on PATH — downloads nothing.
- **The Visual C++ runtime was missing the same way.** The engine and ggml
  import `MSVCP140.dll`, `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll` and
  `VCOMP140.DLL`. When they are absent the studio downloads Microsoft's own
  redistributable and runs it, once, and only then.
- **The download buttons for the optional CUDA libraries did nothing.** The
  endpoint started the download inside a task that threw away its own result,
  then answered "started" regardless - so a refusal ("another download is
  already running") vanished into a successful reply, and no interface could
  have reported it. The refusal now reaches the caller, and the interface reads
  the reply instead of discarding it.
- All five interface languages declared `karaokeOff` twice, and the second
  silently replaced the first.
- **The writing assistant kept the graphics card after it had answered.** Gemma
  holds around five gigabytes; the engine then asked for eleven more and died
  on a 24 GB card, and the studio reported a queued job that never ran. The
  assistant is now unloaded as soon as it answers, and again before the engine
  starts — unless "keep models in VRAM between jobs" is on, which is exactly
  what that setting is for. It starts itself again on the next request.
- **A crashed engine is restarted instead of ending the request.** A job that
  found no engine used to come back as "mm-server is unavailable"; the
  supervisor now brings it back and sends the job again.
- **Running out of video memory says so.** The engine's own log is read when a
  job cannot be submitted, and a card that ran out of room is reported as
  that — instead of "download the five components", which pointed at models
  already on disk.

### Added

- The starting screen reports the library download with real percentages and
  gigabytes, and the "this is taking too long" warning no longer fires while
  half a gigabyte is on its way.
- A release now checks itself: a test walks the import tables of the built
  engine bundle and fails if it names a library that is neither beside it nor
  installed on first start. It found the Visual C++ runtime immediately.

### Changed

- The studio opens in its dark theme unless the user has chosen otherwise.
- The CUDA build carries PTX for the newest architecture as well, so a
  Blackwell variant without its own device code has something to compile from
  instead of failing at the first kernel launch — NVIDIA's own "Building for
  Maximum Compatibility" rule. ggml rewrites the flag to `120a-virtual`,
  because its Blackwell kernels use instructions that exist only there, so this
  does not reach past Blackwell.

## 2026-08-19 — 1.3.1

### Added

- **A portable copy keeps everything inside its own folder** — models, the
  library, media, logs, settings, temporary files and the WebView cache all sit
  beside the executable. Nothing is written into the user profile, so deleting
  the folder deletes the studio.
- **Downloads can be undone** — every ready-made set, every per-role
  quantisation and every optional model has a remove button beside the one that
  fetched it, and the panel shows the folder they live in with a button that
  opens it.
- **A local model fetches itself on first use** — choosing Parakeet or Whisper
  is the instruction to use it, so the first track that needs timings downloads
  the model, reporting real percentages, and then does the work.

### Changed

- The engine now dies with the studio however the studio ends — a job object
  ties the process tree together, so a force-closed window no longer leaves the
  engine holding the graphics card.
- The local writing assistant is constrained by a JSON schema at the sampling
  level, and its context doubled to 16384 tokens. It could previously answer
  with prose, with a fenced block, with a list where a string belonged, or run
  out of room mid-answer.
- Lyrics are written in the language of the request. The rule that keeps the
  caption English - the engine reads it - had been swallowing the song too.
- Cover art and cloud transcription stay silent without a key instead of
  reporting a failure nobody can act on.

### Fixed

- A settings page that changed one capability erased every other choice, which
  is how a studio with a downloaded engine started answering "the local music
  engine is not configured" and queueing jobs forever.
- The download panel sent `component_ids` while the service read `ids`, so
  pressing download on the 11.9 GB set fetched the 26.6 GB one. An empty
  request is now refused outright rather than falling back to a default.
- Removing weights left the download that would resume them in the studio's
  state, so deleted files came back by themselves on the next start.
- A job that names weights no longer on disk is refused with an explanation
  instead of being sent to the engine, which spent a minute loading nothing.
- The window recovers on its own when the service takes a moment longer to
  start, instead of leaving a browser connection error on screen for good.

## 2026-08-18

### Added

- **Stem separation** — a finished track is split into six tracks (drums, bass, other,
  vocals, guitar, piano) by HT-Demucs through ONNX Runtime. Runtime is chosen the same way
  as everywhere else: automatic, GPU or CPU. The model sits among the optional downloads
  and is fetched only on request. Controls live in Studio tools; the track menu opens a
  song there already selected.
- **Automatic cover art** — a cover is drawn as soon as a track finishes, if the switch is
  on, using the prompt the writing assistant produced along with the title and the length.
- **Cover prompt templates** with `{title}`, `{style}`, `{lyrics}`, `{excerpt}` and
  `{duration}`, and a default template that can be set in Settings.
- **ID3v2.4 tags on exported MP3s** — title, artist, album, genre, tempo, lyrics and the
  cover image.
- **A running account of background work** — covers and karaoke timings report themselves
  while they happen instead of finishing in silence, and the library rereads a track as
  soon as its work is done.

### Changed

- Covers are requested as a square 1024×1024. Image models answered in their own habit
  before, and a 1408×768 frame was cropped to a strip in every card.
- The image format is read from the picture's own bytes rather than from what was declared
  about it, and that type is what reaches the file name, the HTTP response and the tag
  inside the MP3.
- Track titles follow ACE-Step Studio's rule — a chorus line, then any sung line, then the
  description — and the description branch now names the genre instead of the caption's
  heading and measurements.
- Tracks are signed **MiniMax Music 3**, the same name that goes into the artist tag. The
  fork's web-era "Anonymous" is gone.
- Karaoke is refused for an instrumental before any recogniser is involved, on both the
  local and the cloud path, and the button is not offered for a track with no words. The
  failures now say what happened in the language of the interface.
- The writing assistant chooses the length of a track when the request does not, up to 360
  seconds, and fields the user filled in are handed to the model to build around.

### Fixed

- The MiniMax `music-caption-rewriter` skill reached neither the local nor the cloud model:
  the reference captions were selected and then never added to the prompt.
- Genre never made it into the tag, because only the caption's first phrase was examined
  and that phrase is always the heading.
- Downloads no longer re-request a package that finished but was not published, and an
  interrupted file is published atomically so a half-written DLL is never taken for an
  installed one.

## Earlier

The studio grew out of [ACE-Step Studio](https://github.com/timoncool/ACE-Step-Studio) and
was rebuilt around MiniMax Music3: a native Rust service inside a Tauri window, supervising
`minimaxmusic.cpp` — no Python and no Node.js in the shipped runtime.
