# Changelog

What changed, newest first. Dates are release dates.

## 2026-09-26 — 2.0.0

A new studio on a new stack. The Python and Node.js folder of 1.x is replaced by one Windows
executable: a Rust service and a React interface in a Tauri window, on
[acestep.cpp](https://github.com/ServeurpersoCom/acestep.cpp), the native C++/GGML engine of
ACE-Step 1.5. No Python, no PyTorch, no Node.js at run time.

### Added

- **An installer that updates itself, or a portable folder.** Models, songs and settings stay
  in the studio's own folder.
- **GGUF models from Q4 to BF16.** Five ready sets from 3.3 GB (2B turbo, 4 GB cards) to
  19.9 GB (XL turbo BF16), and every DiT of ACE-Step 1.5 — 2B and XL, turbo, SFT, base, the
  shift and continuous turbos, merges, MXFP4, the Regrind fine-tune — in a switcher above
  the create form. The planner and the VAE are chosen in Settings.
- **One writer at a time.** The simple form turns one line into a song through the writing
  assistant when it is set up, or through ACE-Step's own planner otherwise — never both. The
  assistant writes by ACE-Step's official songwriting guide. Caption rewriting is off by
  default, as in ACE-Step's own interface.
- **Plan strength** — how many of the DiT's steps follow the planner's audio codes.
- **A LoRA catalogue and Hugging Face search.** 131 LoRA picked by likes and downloads,
  described in five languages, and a search of everything Hugging Face has for ACE-Step;
  every file's header is read before it downloads, and the studio says which model size it
  fits or why the engine cannot use it. A LoRA of the sound turns the planner's audio codes
  off, as ACE-Step's authors advise.
- **Train your own LoRA** with HOT-Step's ace-train and its LoKR recipe, on the unquantised
  weights of the DiT you use; datasets prepared by the studio, and **Train further** from the
  adapter a run left.
- **Stems, MIDI, karaoke, audio processing, VST3 and video clips** from the studio family.
- **MCP**: 163 tools for an agent, and the agent as the studio's writing assistant.
- **FLAC** output.

### Not carried over from 1.x

Flow-edit and the "Most Natural" repaint mode (Python pipeline only), the model tools
(BF16 converter, merger, LoRA baking), Pollinations covers, and macOS and Linux builds.

## 2026-05-05

### Added
- **OpenRouter LLM provider** — bring-your-own-key alternative to the local LM. Pick any model (Claude, GPT-4o, DeepSeek, Llama 3.x, 200+ supported, **including many free ones — DeepSeek R1 free, Llama 3.3 free, Gemini 2.0 free etc.**), get instant lyrics + caption + BPM/key/duration metadata + a visual `coverPrompt` from a one-line description. Persists model choice across sessions, streams output with a real-time preview. Local LM keeps working in parallel — toggle anytime, both modes coexist.
- **Pollinations.ai auto cover generation** — server-side, parallel with audio render, fire-and-forget. Never blocks the audio pipeline; cover_url is filled via background UPDATE 5–30 s after audio completes. The visual prompt is the LLM-generated `coverPrompt` (or a keyword fallback) — sent to Pollinations as-is, no extra style modifiers. **Anonymous tier is fully free** (no account needed, just slower); optional tokens (`pk_…` / `sk_…` from auth.pollinations.ai) lift to Seed tier (1 req/5 s, no watermark, full model catalogue).
- **Manual cover regeneration modal** — `ImagePlus` button on every owned song row in SongList and in the RightSidebar Main Actions. Lets the user pick any Pollinations image model, write a custom prompt, *Try again* until satisfied, **or upload a custom image from disk** (JPEG/PNG/WEBP, max 10 MB). The picked image replaces both `songs.cover_url` AND the embedded ID3 cover frame inside the MP3, so any external player sees the new picture in downloads.
- **Auto-pipeline ID3 re-embed** — when Pollinations finishes the auto cover, `attachCover` now also reads the source MP3, replaces the embedded ID3 image frame with the new picture, and writes it back. Previously the downloaded MP3 kept the seeded picsum thumbnail forever even though the in-app cover was correct.
- **LLM `coverPrompt` field** in SongDraft schema — the OpenRouter system prompt now also asks for a 1–2 sentence visual album-cover description per song, fed straight into Pollinations as the cover prompt.
- **Detailed pre-flight stages** — the OpenRouter pre-flight call now reports `stageOpenRouterConnecting → stageOpenRouterStreaming → stageOpenRouterFinalizing` driven by stream events, so a stuck card tells you whether the issue is on connect, in mid-stream or in JSON parsing.
- **Cancel + 90 s timeout for OpenRouter pre-flight** — the cancel button on the placeholder card now actually aborts the in-flight OR HTTP request (was previously inert). Cancel-all and Reset-all also abort every pending pre-flight aborter so OR responses can't 'win the race' and spawn audio jobs after the user cancelled. A hard 90-second timeout kills hung requests automatically.
- **Queue refactor** — instant N/10 click counter, instant placeholder card at click time (no more 20-second wait staring at an empty list while LLM pre-flight runs), FIFO drain barrier so bulk clicks chain through pre-flight + audio sequentially.
- **i18n** — 52 new keys × 5 languages (en/ru/zh/ja/ko) for stage labels (incl. 3 OR sub-stages), Pollinations panel, OpenRouter panel, cover-regen modal.

### Fixed
- **`resetGeneration` queue deadlock** — the global Reset-all button cleared `activeJobsRef` but never called `drainQueueWaiters()` or reset `pendingClickCount`, so any pre-flight click parked on `waitForJobsToDrain` would hang forever and the badge stayed stuck.
- **Simple-mode + local LM + Pollinations toggle ON** — Simple-mode `onGenerate` payload didn't include the `pollinations` field, so backend `startCoverGen` was never called → covers silently never generated. Now threaded through both Simple and Custom branches.
- **Auto-pipeline race vs manual cover save** — gate the auto-pipeline `attachCover` UPDATE on `cover_url IS NULL` so a user's manual save during in-flight Pollinations gen is never silently overwritten.
- **Orphan cover file on extension change** — manual `.webp` upload over an existing `/audio/.../{songId}.jpg` left the old file on disk forever. The endpoint now deletes the previous local cover when the extension differs.
- **Cover-jobs Map resurrection** — added a tombstone Set so an in-flight Pollinations Promise resolving after `consumeCoverState` no longer re-inserts the entry (~300 KB Buffer leak per cancel/fail averted).
- **`handleGenerate` auth-bail leak** — if `isAuthenticated/token` flipped false between CreatePanel click and the App.tsx handler, the placeholder card and `pendingClickCount` slot were leaked. Now both are cleaned up before showing the username modal.
- **`clampInt('')` UX** in PollinationsPanel — width/height inputs no longer snap to 256 the moment the user clears the field to retype a different number.

### Changed
- **CSP `connectSrc`** allows `gen.pollinations.ai`, `image.pollinations.ai`, `openrouter.ai`. Browser-direct fetches to OpenRouter and Pollinations work without a server proxy.
- **CSP `imgSrc`** now allows `blob:` so the cover-regen modal preview can render `URL.createObjectURL(blob)` images.
- **Storage abstraction** gained an optional `read(key)` method (Local implements; remote providers like S3 may omit so callers fall back to skipping retag instead of paying a download).
- **Removed the 16 deterministic art-style modifiers** from `cover-jobs.ts` — the LLM-generated `coverPrompt` is already a tailored visual description, appending another fixed style label was clobbering it. The prompt now goes to Pollinations as-is.

## 2026-05-04

### Added
- **DCW (Differential Correction in Wavelet domain)** — CVPR 2026 paper, training-free quality boost on every sampler step. Default ON. New panel in Custom mode: Mode (low/high/double/pix) + Wavelet (haar/db4/sym4/sym8/...) + 2 strength sliders
- **Retake** — variation seed with Variance slider (variance-preserving blend with independent noise draw)
- **Flow-edit** (#1156) — text-edit overlay morphing src toward target prompt/lyrics on text2music + cover + cover-nofsq tasks. Full UI panel in Custom mode: source caption + source lyrics + n_min/n_max range + n_avg stability
- **Repaint "Most Natural" mode** — 4th option in Repaint task (next to Conservative/Balanced/Aggressive). Maps to maximum source preservation (injection_ratio=1.0, crossfade=25 frames + 0.05s). Useful for iterative repaint refinement with minimum drift from source across multiple passes
- **ScragVAE** — alternative community VAE swap via `ACESTEP_VAE_CHECKPOINT` env var
- **MLX DCW** — DCW correction for Apple Silicon path (haar native, other bases via fallback)
- **`use_legacy_cfg_prompt`** A/B toggle for old vs training-aligned LM CFG prompt format

### Fixed
- **`infer_steps` on turbo / xl-turbo** — was silently clamped to 8 steps, now respects UI value (1–20)
- **LM CFG uncond prompt** aligned with training dropout format (#1127, #1128) — better lyrics quality, missing `\n\n` after `</think>` restored
- **MLX DiT static buffers** materialized before worker use on Apple Silicon (#1166)
- **`GenerationParams` None handling** — no longer crashes on `None` numeric fields (#1027)
- **Handler kwargs** no longer silently swallowed on base/turbo/xl-turbo paths
- **Express → Gradio**: `/create_sample` (Gradio) → `/v1/create_sample_from_query` (our HTTP) — Magic Wand now generates real lyrics from description (was always returning `[Instrumental]`)
- **Express → Gradio**: `/format_caption` + `/format_lyrics` (removed) → `/format_input` (single FastAPI endpoint)
- **Express → Gradio**: `/load_random_simple_description` (removed) → `/create_random_sample` (FastAPI)
- **`/auto_label_all` and `/init_service_wrapper`** — restored as named Gradio endpoints (was lost to `/lambda_N` after lambda wrapping)
- **`api_routes._get_project_root`** — fixed walking 5 levels up instead of 3 (was returning `acestep/ui` instead of project root, breaking random sample examples)
- **`unhandledRejection` global handler** in Express — `@gradio/client` no longer crashes Node when stream closes after error

### Changed
- **Synced with upstream ACE-Step v0.1.7** + post-release fixes (May 1, 2026) — 72 upstream commits merged across 148 files
- **`scheduler_type`** propagated end-to-end through wiring → batch_management → progress → GenerationParams → inference → handler → service_generate → model (was disconnected after merge, now fully wired)
- **`pytorch-wavelets >= 1.3.0`** + **`pywavelets >= 1.9.0`** — added to `install.bat` and Pinokio launcher (required for DCW)

## 2026-04-23

### Added
- **Pinokio launcher** — one-click cross-platform install via [Pinokio](https://pinokio.co). Install on Windows / Linux (x64 & aarch64) / macOS (Apple Silicon & Intel) / AMD / CPU without touching `install.bat`. Launcher repo: [timoncool/ACE-Step-Studio-pinokio](https://github.com/timoncool/ACE-Step-Studio-pinokio)
- **MLX native acceleration** on Apple Silicon — uses Apple Metal Performance Shaders directly for LM inference

## 2026-04-17

### Fixed
- **Model submodule downloader** — `download_submodel` reported success when the target directory was empty; now validates that files actually downloaded

## 2026-04-14

### Added
- **`run-no-lm.bat`** — start without LM for more VRAM (cover/repaint/text2music work, no thinking/enhance/auto-lyrics)
- **Sidebar shows "LM off"** when no LM loaded instead of hiding the line
- **Bottom player play button** works without clicking a song first — falls back to selected song
- **Lyrics textarea collapses** when instrumental mode is on

### Changed
- **Gradio args refactored to named parameters** — no more positional array counting, impossible to shift params
- **Default LM model** changed to 0.6B with PT backend everywhere (gpu_config, llm_inference, api_routes, Express)
- **Quick Settings** (Duration, BPM, Key, Time Signature, Variations) now visible in both Simple and Custom modes
- **Auto-title** picks first line from Chorus/Hook instead of first line of lyrics; max 2 phrases, cut at sentence boundary
- **Song DB records** now store actual server model state, not frontend params

### Fixed
- **LRC not showing after reload** — liked songs overwrote my songs in Map, losing lrcContent; mapSong now falls back to snake_case `lrc_content`
- **LRC missing in song details / video studio** — `getSong` and `getFullSong` didn't map `lrc_content` → `lrcContent`
- **LM backend dropdown** always showed PT — health endpoint read wrong attribute (`llm_handler.backend` → `llm_handler.llm_backend`)
- **LM backend not passed to /v1/init** — PT/vLLM selection was ignored, always loaded vLLM
- **vLLM not freed on unload** — `unload()` called `reset()` instead of `exit()`, CUDA graphs and KV cache stayed in VRAM
- **RAM leak on model switch** — old model/vae/text_encoder not deleted before loading new ones; pinned memory not unpinned
- **keyScale select** passed React event object instead of string value (broke DiT metas)
- **timeSignature select** — same bug
- **LM model/backend desync** — dropdowns now sync from server on every health poll (with editing guard)
- **Switch-model log** no longer says "Unloading DiT" when only LM changes
- **Instruction per task type** — cover and repaint now use correct instruction strings
- **gc.collect + empty_cache** after LM unload before loading new model
- **Video render: background image missing** — render re-fetched via proxy instead of using bgImageRef directly
- **Video render: CCTV date/REC overlay missing** — only drawn in preview, now in render too with matching font/size
- **Video render: Chrome "page not responding"** — yield every 30 frames prevents dialog
- **Video render: PayloadTooLargeError** — body-parser limit increased from 10mb to 50mb
- **Cyrillic filenames** in uploaded audio — multer latin1→UTF-8 decode
- **Stale generating songs** blocked real songs with lrcContent in refreshSongsList merge

## 2026-04-13

### Added
- **Tools page** in sidebar (between Search and Training) with two utilities:
  - **BF16 Converter** — convert safetensors from FP32/FP16 to BFloat16 (~50% size reduction)
  - **Model Merger** — merge two ACE-Step models with adjustable alpha blending
- `reinstall.bat` — clean reinstall preserving models, data, and output
- Sampler mode selection (Euler / Heun) in generation settings
- Changelog tab on News page (reads from CHANGELOG.md)

### Changed
- Training page redesigned from single-column to responsive 2-column grid layout
- `update.bat` now properly updates all Python dependencies (not just ace-step)

### Fixed
- TypeScript samplerMode type narrowing error
- `reinstall.bat` warns user to close app instead of killing all node processes
- `update.bat` checks node exists before running npm steps

## 2026-04-12

### Added
- **Video Studio** with full WYSIWYG editor:
  - Resolution selector + lyrics overlay with styling
  - WYSIWYG drag for ALL elements (visualizer, lyrics, text layers)
  - Selection frame (pink dashed border) on hover/drag
  - Full playback controls — timeline seekbar + volume slider
  - Visualizer scale slider + scroll-to-resize
  - 3 lyrics styles — Lines, Scroll (marquee), Karaoke (progressive fill)
  - Lyrics color settings — text color, bg color/opacity, highlight color
  - Lyrics timing offset slider (-3s to +3s) for sync
  - Default aspect ratio 1:1 (square), default lyrics style Karaoke
  - Local FFmpeg — no CDN dependency for video rendering
  - Server-side FFmpeg encoding with GPU acceleration (NVENC)
  - Chunked video encoding — frames sent in batches of 50
- **LRC toggle** (ON/OFF) under vocal language section (Simple + Custom modes)
- **Audio blocks** split into independent Reference + Cover slots
- Waveform visualization on Reference and Cover audio players
- Drag region selection on waveform for Repaint mode
- Hints under Cover/Repaint sliders explaining what they do
- Cover strength % and task type display in Sources section
- Repaint strength + region display in Sources section
- Separate AI buttons for lyrics — Generate (Wand2) + Enhance (Sparkles)
- Clear VRAM error message shown as red toast (8s duration)
- XL Merge SFT+Turbo community model (by jeankassio) with metadata and download
- Guidance range unlocked 0-20 for all models
- Triton, Python headers, Flash Attention added to `install.bat`
- Multilingual news page with links support
- Training page with Coming Soon placeholder

### Changed
- Default audio cover strength from 100% to 50%

### Fixed
- Persist BPM/Key/Duration/TimeSignature across generations
- Don't overwrite manual BPM/Key/Duration from AI suggestions
- Null safety for BPM/Key/Duration loaded from settings
- Sync LM settings only on first connect + after model switch
- Job status set to failed on queue processing error
- nano-vllm engine cleanup — atexit.unregister for proper GC
- LLM unload — free KV cache CUDA memory properly
- Skip DiT reload when only LM model changes
- VRAM management: unload LM before loading new DiT
- Flash Attention made optional with wheel dependency
- Model hot-swap: correct LLM init args, health check
- Force int8 quantization for FP32 XL models
- Cover/repaint mode — audio file handling for Gradio
- Isolate simple/custom mode params, fix TypeScript errors
- Audio cover strength slider step from 5% to 1%
- Detect incomplete model downloads and re-download automatically
- Merge model detection and download flow
- Lyrics overlay sync — used audio time instead of Date.now()
- Karaoke progress capped at 5s per line, hidden after fully sung
- Video export lyrics sync + smooth audio analysis
- npm audit — 0 vulnerabilities
- `install.bat` — hatchling, nano-vllm, deps ordering, FFmpeg, server deps, vite build

## 2026-04-11

### Added
- **ACE-Step 1.5 XL Studio** — portable AI music generation app (initial release)
- **Web UI**: Create, Library, Search, Training (placeholder), News pages
- **Simple and Custom** generation modes
- **Single terminal mode** — Express manages Python pipeline + serves frontend
- **Model hot-swap** via /v1/init Gradio API route
- **Video Studio** — resolution selector + lyrics overlay (early version)
- Audio upload in Simple mode with inline Cover/Repaint controls
- LM model selector (4B/1B) with auto-download
- vLLM backend selector with persistence
- Generation queue with concurrent job tracking
- Real-time generation progress via Gradio submit events
- Persist generation settings in database
- Multi-language support (EN, RU, ZH, JA, KO) — all strings i18n'd
- System monitoring widget (GPU/VRAM/RAM/CPU temp)
- Backend connection state indicator (backend off vs Gradio starting)
- Portable installation (embedded Python 3.12 + Node.js 22)
- Song library with playlists, likes, and search
- Right sidebar with song details — BPM/Key/Duration/Model display
- Reuse restores ALL generation params including seed/BPM/key
- Generation time (stopwatch icon) next to model badge
- Dark/light theme toggle
- User authentication system
- Resizable panels for create/songlist/details layout
- Embed ID3 tags in generated MP3 files
- Auto LRC generation and store timestamped lyrics
- Download LRC button in song details
- Generate lyrics from style when lyrics field is empty
- Undo buttons for lyrics and style fields
- Vocal/instrumental toggle switch
- Default model: `acestep-v15-xl-turbo-bf16`
- Auto-find free port if 3001 is busy (tries up to +10)
- `install.bat` — one-click setup with GPU selection (Pascal to Blackwell)
- `update.bat` — pull and rebuild
- `download_model.bat` — model downloader via huggingface-cli
- `run.bat` / `run-dev.bat` — production and development launchers

### Fixed
- Stabilize switch-model — retry port kill, wait until free
- Simple mode param isolation from Custom mode
- Inference steps clamped to model max
- Crash when sample.caption is not a string
- Resolve PYTHON_PATH to absolute path
- Prevent polling from overriding model selection during switch
- Block Simple mode generation when LLM is unavailable
- Prevent settings reset on generation
- Non-blocking connection banner instead of fullscreen spinner
