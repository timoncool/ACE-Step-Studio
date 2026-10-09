//! ACE-Step 1.5 through acestep.cpp's `ace-server`: the request the create
//! page sends, the language-model pass, the synthesis pass and its result.
//!
//! The request keeps the engine's own field names (`AceRequest` in the
//! engine's request.h), so nothing between the page and the engine can drop a
//! control. A song is made in two engine jobs: `/lm` writes the plan (metadata,
//! lyrics, audio codes) when the page asks the model to think, then `/synth`
//! renders every planned request, several takes each. The planned request of a
//! take, seed included, is its replay record: sent to `/synth` again it
//! renders the same track without planning again.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

use crate::multipart;

/// Every field `AceRequest` reads. Anything else a page sends is refused, so a
/// misspelt control fails loudly instead of being ignored by the engine.
pub const REQUEST_FIELDS: &[&str] = &[
    "caption", "lyrics", "bpm", "duration", "keyscale", "timesignature", "vocal_language",
    "lm_batch_size", "synth_batch_size", "seed", "lm_seed",
    "lm_temperature", "lm_cfg_scale", "lm_top_p", "lm_top_k", "lm_negative_prompt", "use_cot_caption",
    "lm_rep_penalty", "lm_rep_window", "lm_rep_mode", "lm_dry_base", "lm_dry_min_len",
    "audio_codes", "inference_steps", "guidance_scale", "shift",
    "dcw_scaler", "dcw_high_scaler", "dcw_mode",
    "audio_cover_strength", "cover_noise_strength", "repainting_start", "repainting_end",
    "latent_shift", "latent_rescale", "custom_timesteps", "task_type", "track",
    "solver", "stork_substeps", "jkass_beat_stability", "jkass_frequency_damping", "jkass_temporal_smoothing",
    "scheduler", "guidance", "apg_momentum", "apg_norm_threshold", "cfg_zero_init_steps", "smc_lambda", "smc_k",
    "cfg_mp_iterations", "lm_mode", "output_format", "synth_model", "lm_model", "vae",
    "adapter", "adapter_scale", "lm_adapter", "lm_adapter_scale", "adapters", "adapter_group_scales",
    "mp3_bitrate", "cfg_interval_start", "cfg_interval_end", "retake_seed", "retake_variance",
];

pub const TASKS: &[&str] = &["text2music", "cover", "cover-nofsq", "repaint", "lego", "extract", "complete"];

/// Tasks that work on a source recording.
pub fn needs_source(task: &str) -> bool {
    task != "text2music"
}

/// The vocal language a lyric's script names, for a request that names none.
/// The engine forces a given language into its plan and guesses an unset one,
/// so Cyrillic words left to the guess have been sung in another language.
/// Latin script names no language.
pub fn script_language(lyrics: &str) -> Option<&'static str> {
    let mut inside_tag = false;
    let (mut cyrillic, mut ukrainian, mut hangul, mut kana, mut han, mut arabic, mut devanagari, mut thai, mut latin) = (0, 0, 0, 0, 0, 0, 0, 0, 0);
    for character in lyrics.chars() {
        match character {
            '[' => inside_tag = true,
            ']' => inside_tag = false,
            _ if inside_tag => {}
            'і' | 'ї' | 'є' | 'ґ' | 'І' | 'Ї' | 'Є' | 'Ґ' => { cyrillic += 1; ukrainian += 1; }
            '\u{0400}'..='\u{04FF}' => cyrillic += 1,
            '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' => hangul += 1,
            '\u{3040}'..='\u{30FF}' => kana += 1,
            '\u{4E00}'..='\u{9FFF}' => han += 1,
            '\u{0600}'..='\u{06FF}' => arabic += 1,
            '\u{0900}'..='\u{097F}' => devanagari += 1,
            '\u{0E00}'..='\u{0E7F}' => thai += 1,
            'A'..='Z' | 'a'..='z' | '\u{00C0}'..='\u{024F}' => latin += 1,
            _ => {}
        }
    }
    let cjk = if kana > 0 { ("ja", kana + han) } else { ("zh", han) };
    let cyrillic_name = if ukrainian > 0 { "uk" } else { "ru" };
    let (name, letters) = [(cyrillic_name, cyrillic), ("ko", hangul), cjk, ("ar", arabic), ("hi", devanagari), ("th", thai), ("latin", latin)]
        .into_iter()
        .max_by_key(|(_, letters)| *letters)?;
    (letters >= 8 && name != "latin").then_some(name)
}

/// Tasks only the base model (and base merges) was trained for.
pub fn base_model_only(task: &str) -> bool {
    matches!(task, "lego" | "extract" | "complete")
}

/// Stems the engine knows by name for lego, extract and complete.
pub const TRACKS: &[&str] = &[
    "vocals", "backing_vocals", "drums", "bass", "guitar", "keyboard", "percussion", "strings", "synth", "fx",
    "brass", "woodwinds",
];

pub const SOLVERS: &[&str] = &[
    "euler", "sde", "dpm2m", "dpm2m_ada", "dpm3m", "stork2", "stork4", "unipc_p", "aflops", "jkass_fast", "heun",
    "rfsolver", "unipc", "aflops2", "jkass_quality", "rk4", "rk5", "gl2s", "dopri5", "dop853",
];

pub const SCHEDULERS: &[&str] =
    &["linear", "ddim_uniform", "sgm_uniform", "bong_tangent", "linear_quadratic", "cosine", "power", "beta57"];

/// How the language model's repetition penalty counts a repeat.
pub const REP_MODES: &[&str] = &["presence", "frequency", "dry"];

pub const GUIDANCE: &[&str] = &["apg", "cfg_pp", "dynamic_cfg", "rescaled_cfg", "cfg_zero_star", "smc_cfg", "cfg_mp", "adg"];

/// The engine's largest batch: every take of one synthesis job together.
pub const MAX_TAKES: u64 = 9;

/// Checks a request from the page and completes it into what the engine gets:
/// seeds rolled here so every take can be named and replayed, the studio's
/// own MP3 encoding asked for as 32-bit float.
pub fn prepare(input: &Value) -> Result<Value> {
    let fields = input.as_object().context("the request must be a JSON object")?;
    let mut out = Map::new();
    for (key, value) in fields {
        if !REQUEST_FIELDS.contains(&key.as_str()) {
            bail!("unknown request field `{key}`");
        }
        out.insert(key.clone(), value.clone());
    }
    let task = out.get("task_type").and_then(Value::as_str).unwrap_or("text2music").to_owned();
    if !TASKS.contains(&task.as_str()) {
        bail!("task_type must be one of {}", TASKS.join(", "));
    }
    out.insert("task_type".into(), Value::from(task.clone()));
    let caption = out.get("caption").and_then(Value::as_str).unwrap_or("").trim().to_owned();
    if caption.is_empty() && !matches!(task.as_str(), "lego" | "extract" | "complete") {
        bail!("the style caption is empty");
    }
    if base_model_only(&task) {
        let track = out.get("track").and_then(Value::as_str).unwrap_or("").trim().to_owned();
        if track.is_empty() {
            bail!("{task} needs the instrument track to work on");
        }
    }
    for (key, allowed) in [("solver", SOLVERS), ("guidance", GUIDANCE), ("lm_rep_mode", REP_MODES)] {
        if let Some(value) = out.get(key).and_then(Value::as_str) {
            if !allowed.contains(&value) {
                bail!("{key} must be one of {}", allowed.join(", "));
            }
        }
    }
    if let Some(scheduler) = out.get("scheduler").and_then(Value::as_str) {
        let base = scheduler.split(':').next().unwrap_or("");
        if !SCHEDULERS.contains(&base) && !matches!(base, "beta" | "composite") {
            bail!("scheduler must be one of {}, power:<p>, beta:<a>:<b> or composite:<A>+<B>:<x>:<s>", SCHEDULERS.join(", "));
        }
    }
    let takes = out.get("synth_batch_size").and_then(Value::as_u64).unwrap_or(1).max(1);
    let plans = out.get("lm_batch_size").and_then(Value::as_u64).unwrap_or(1).max(1);
    if takes * plans > MAX_TAKES {
        bail!("{plans} songs of {takes} takes make {} tracks; the engine renders at most {MAX_TAKES} at once", takes * plans);
    }
    for key in ["seed", "lm_seed"] {
        let rolled = out.get(key).and_then(Value::as_i64).filter(|seed| *seed >= 0).unwrap_or_else(random_seed);
        out.insert(key.into(), Value::from(rolled));
    }
    Ok(Value::Object(out))
}

/// A seed in the range the engine's Philox noise reads, [0, 2^32).
pub fn random_seed() -> i64 {
    (uuid::Uuid::now_v7().as_u128() as u64 as i64 & 0xffff_ffff).abs()
}

/// The planned requests `/synth` renders: the language model's results, or
/// the request itself when the model does not plan. Each keeps one seed per
/// take; the engine counts the takes' seeds up from it. The engine always
/// hands over its unencoded float output, the model's own rate, precision and
/// level, so a track is encoded once, by the studio, and nothing changes its loudness.
pub fn synth_requests(planned: Vec<Value>) -> Vec<Value> {
    planned
        .into_iter()
        .map(|mut request| {
            if let Some(fields) = request.as_object_mut() {
                fields.remove("lm_batch_size");
                fields.insert("output_format".into(), Value::from("wav32"));
                fields.remove("mp3_bitrate");
                fields.remove("peak_clip");
            }
            request
        })
        .collect()
}

/// One rendered take: its audio, the latent it decoded from, and the request
/// that renders exactly it again.
pub struct Take {
    pub audio_type: String,
    pub audio: Vec<u8>,
    pub latent: Vec<u8>,
    pub replay: Value,
}

/// Pairs the `/synth` result's audio and latent parts with the requests they
/// came from: request i expands into its synth_batch_size takes, seeds counted
/// up from its own.
pub fn takes(requests: &[Value], content_type: &str, body: &[u8]) -> Result<Vec<Take>> {
    let parts = multipart::parse(content_type, body)?;
    if parts.len() % 2 != 0 {
        bail!("the engine returned {} parts; every take is an audio part and a latent part", parts.len());
    }
    let mut replays = Vec::new();
    for request in requests {
        let count = request.get("synth_batch_size").and_then(Value::as_u64).unwrap_or(1).max(1);
        let seed = request.get("seed").and_then(Value::as_i64).context("a planned request has no seed")?;
        for index in 0..count {
            let mut replay = request.clone();
            if let Some(fields) = replay.as_object_mut() {
                fields.insert("seed".into(), Value::from(seed + index as i64));
                fields.insert("synth_batch_size".into(), Value::from(1));
            }
            replays.push(replay);
        }
    }
    if replays.len() != parts.len() / 2 {
        bail!("the engine returned {} takes for {} requested", parts.len() / 2, replays.len());
    }
    let mut takes = Vec::with_capacity(replays.len());
    for (pair, replay) in parts.chunks(2).zip(replays) {
        let audio_type = pair[0].content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        if !audio_type.starts_with("audio/") {
            bail!("expected an audio part, got {}", pair[0].content_type);
        }
        takes.push(Take { audio_type, audio: pair[0].body.clone(), latent: pair[1].body.clone(), replay });
    }
    Ok(takes)
}

/// What `/understand` heard in a recording: the request the language model
/// wrote for it (caption, lyrics, metadata, audio codes).
pub fn understood(content_type: &str, body: &[u8]) -> Result<Value> {
    for part in multipart::parse(content_type, body)? {
        if part.content_type.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json") {
            let value: Value = serde_json::from_slice(&part.body).context("the engine's description is not JSON")?;
            return Ok(match value {
                Value::Array(mut items) if !items.is_empty() => items.remove(0),
                other => other,
            });
        }
    }
    bail!("the engine returned no description of the recording")
}

/// A source recording and a timbre reference for `/synth`, as audio or as
/// latents the engine made before (latents skip the VAE encode).
#[derive(Default)]
pub struct Sources {
    pub src_audio: Option<Vec<u8>>,
    pub src_latents: Option<Vec<u8>>,
    pub ref_audio: Option<Vec<u8>>,
    pub ref_latents: Option<Vec<u8>>,
}

impl Sources {
    fn is_empty(&self) -> bool {
        self.src_audio.is_none() && self.src_latents.is_none() && self.ref_audio.is_none() && self.ref_latents.is_none()
    }
}

#[derive(Clone)]
pub struct AceClient {
    base_url: String,
    http: reqwest::Client,
}

impl AceClient {
    pub fn from_environment() -> Self {
        let base_url = std::env::var("STUDIO_ENGINE_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{}", music_engine::server::default_port()))
            .trim_end_matches('/')
            .to_owned();
        // The engine is on loopback: a connection takes microseconds when it
        // listens. Without a limit a closed port costs Windows two seconds per
        // attempt, and a filtered one twenty, and the status polls pile up.
        // the engine's server drops a connection idle for 5 s; a pooled one can be
        // taken as it closes and the request is lost, so every request opens its own
        let http = crate::net::builder()
            .connect_timeout(Duration::from_millis(500))
            .pool_max_idle_per_host(0)
            .build()
            .expect("the HTTP client builds with a connect timeout");
        Self { base_url, http }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub async fn health(&self) -> bool {
        self.http.get(self.url("/health")).send().await.map(|response| response.status().is_success()).unwrap_or(false)
    }

    pub async fn props(&self) -> Result<Value> {
        json_of(self.http.get(self.url("/props")).send().await?).await
    }

    /// The engine's `/logs`, an endless SSE stream, followed for the service's
    /// life: its recent lines and the running job's progress.
    pub fn follow_log(&self) -> crate::engine_log::EngineLog {
        crate::engine_log::EngineLog::follow(self.http.clone(), self.url("/logs"))
    }

    /// Starts a language-model job over one or several requests.
    /// Sends a request to the engine on loopback, again when the connection
    /// drops under it: a refused connection on any method, since nothing was
    /// sent, and a reset mid-request only for a read. Three tries, a short
    /// pause between. A blip that polling rode out made a submission fail.
    async fn send(&self, request: reqwest::RequestBuilder, read: bool) -> Result<reqwest::Response> {
        let dropped = |error: &reqwest::Error| {
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
            while let Some(cause) = source {
                if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                    return matches!(io.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe);
                }
                source = cause.source();
            }
            false
        };
        let mut attempt = 0u64;
        loop {
            let Some(copy) = request.try_clone() else { return Ok(request.send().await?) };
            match copy.send().await {
                Ok(response) => return Ok(response),
                Err(error) if attempt < 2 && (error.is_connect() || (read && dropped(&error))) => {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(300 * attempt)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub async fn submit_lm(&self, requests: &[Value]) -> Result<String> {
        let body: Value = if requests.len() == 1 { requests[0].clone() } else { Value::Array(requests.to_vec()) };
        job_id(self.send(self.http.post(self.url("/lm")).json(&body), false).await?).await
    }

    /// Starts a synthesis job; the sources, when any, go as multipart parts.
    pub async fn submit_synth(&self, requests: &[Value], sources: Sources) -> Result<String> {
        let body = Value::Array(requests.to_vec());
        if sources.is_empty() {
            return job_id(self.send(self.http.post(self.url("/synth")).json(&body), false).await?).await;
        }
        let mut form = reqwest::multipart::Form::new()
            .part("request", reqwest::multipart::Part::text(body.to_string()).file_name("request.json"));
        for (name, bytes) in [
            ("audio", sources.src_audio),
            ("src_latents", sources.src_latents),
            ("ref_audio", sources.ref_audio),
            ("ref_latents", sources.ref_latents),
        ] {
            if let Some(bytes) = bytes {
                form = form.part(name, reqwest::multipart::Part::bytes(bytes).file_name(name.to_owned()));
            }
        }
        job_id(self.http.post(self.url("/synth")).multipart(form).send().await?).await
    }

    /// Starts a job that listens to a recording: its caption, metadata,
    /// lyrics and audio codes, as the engine's language model hears them.
    pub async fn submit_understand(&self, audio: Vec<u8>, request: Option<Value>) -> Result<String> {
        let mut form = reqwest::multipart::Form::new().part("audio", reqwest::multipart::Part::bytes(audio).file_name("audio"));
        if let Some(request) = request {
            form = form.part("request", reqwest::multipart::Part::text(request.to_string()).file_name("request.json"));
        }
        job_id(self.http.post(self.url("/understand")).multipart(form).send().await?).await
    }

    pub async fn status(&self, job: &str) -> Result<String> {
        let answer: Value = json_of(self.send(self.http.get(self.url("/job")).query(&[("id", job)]), true).await?).await?;
        answer.get("status").and_then(Value::as_str).map(str::to_owned).context("the engine's job answer has no status")
    }

    pub async fn cancel(&self, job: &str) -> Result<()> {
        let response = self.send(self.http.post(self.url("/job")).query(&[("id", job), ("cancel", "1")]), false).await?;
        if !response.status().is_success() {
            bail!("the engine refused to cancel job {job}: {}", response.status());
        }
        Ok(())
    }

    /// Waits for a job to end: Ok when done, an error naming how it ended
    /// otherwise.
    pub async fn wait(&self, job: &str) -> Result<()> {
        loop {
            match self.status(job).await?.as_str() {
                "done" => return Ok(()),
                "failed" => bail!("the engine failed this job; its log says why"),
                "cancelled" => bail!("cancelled"),
                _ => tokio::time::sleep(Duration::from_millis(400)).await,
            }
        }
    }

    pub async fn result(&self, job: &str) -> Result<(String, Vec<u8>)> {
        let response = self.send(self.http.get(self.url("/job")).query(&[("id", job), ("result", "1")]), true).await?;
        let status = response.status();
        if !status.is_success() {
            bail!("the engine returned {status}: {}", response.text().await.unwrap_or_default());
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        Ok((content_type, response.bytes().await?.to_vec()))
    }

    /// The planned requests of a finished `/lm` job.
    pub async fn lm_result(&self, job: &str) -> Result<Vec<Value>> {
        let (_, body) = self.result(job).await?;
        let planned: Value = serde_json::from_slice(&body).context("the language model's answer is not JSON")?;
        match planned {
            Value::Array(items) if !items.is_empty() => Ok(items),
            Value::Object(_) => Ok(vec![planned]),
            _ => bail!("the language model planned nothing"),
        }
    }
}

async fn json_of(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let reason = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| value.get("error").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or(body);
        bail!("the engine returned {status}: {reason}");
    }
    Ok(serde_json::from_str(&body)?)
}

async fn job_id(response: reqwest::Response) -> Result<String> {
    json_of(response).await?.get("id").and_then(Value::as_str).map(str::to_owned).context("the engine did not return a job id")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lyric_names_its_language_by_its_script() {
        assert_eq!(script_language("[Verse]\nПыль дороги на сапогах\nИ звезда над головой"), Some("ru"));
        assert_eq!(script_language("[Verse]\nЇжак іде додому через ліс"), Some("uk"));
        assert_eq!(script_language("[Chorus]\n사랑해요 오늘 밤 함께 노래해"), Some("ko"));
        assert_eq!(script_language("[Verse]\n夜空に光る星を見上げて"), Some("ja"));
        assert_eq!(script_language("[Verse]\n我们一起唱歌到天亮吧朋友"), Some("zh"));
        assert_eq!(script_language("[Verse]\nWalking down the river in the morning light"), None);
        assert_eq!(script_language("[Intro]\n[Chorus]"), None);
    }
    use serde_json::json;

    #[test]
    fn a_request_keeps_the_engine_fields_and_rolls_its_seeds() {
        let prepared = prepare(&json!({ "caption": "synthwave", "lyrics": "[Instrumental]", "seed": -1 })).unwrap();
        assert_eq!(prepared["task_type"], "text2music");
        assert!(prepared["seed"].as_i64().unwrap() >= 0);
        assert!(prepared["lm_seed"].as_i64().unwrap() >= 0);
        assert!(prepare(&json!({ "caption": "x", "made_up": 1 })).is_err());
        assert!(prepare(&json!({ "caption": "" })).is_err());
        assert!(prepare(&json!({ "caption": "x", "task_type": "lego" })).is_err());
        assert!(prepare(&json!({ "caption": "x", "lm_batch_size": 3, "synth_batch_size": 4 })).is_err());
        assert!(prepare(&json!({ "caption": "x", "solver": "made_up" })).is_err());
        assert!(prepare(&json!({ "caption": "x", "scheduler": "power:3" })).is_ok());
        assert!(prepare(&json!({ "caption": "x", "lm_rep_penalty": 1.1, "lm_rep_mode": "dry", "lm_rep_window": 64 })).is_ok());
        assert!(prepare(&json!({ "caption": "x", "lm_rep_mode": "made_up" })).is_err());
    }

    #[test]
    fn every_take_gets_its_own_seed_and_a_one_take_replay() {
        let requests = vec![json!({ "caption": "a", "seed": 10, "synth_batch_size": 2 }), json!({ "caption": "b", "seed": 50 })];
        let body = b"--b\r\nContent-Type: audio/wav\r\n\r\nA1\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\nL1\r\n--b\r\nContent-Type: audio/wav\r\n\r\nA2\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\nL2\r\n--b\r\nContent-Type: audio/wav\r\n\r\nA3\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\nL3\r\n--b--\r\n";
        let takes = takes(&requests, "multipart/mixed; boundary=b", body).unwrap();
        assert_eq!(takes.len(), 3);
        assert_eq!(takes[1].replay["seed"], 11);
        assert_eq!(takes[1].replay["synth_batch_size"], 1);
        assert_eq!(takes[2].replay["caption"], "b");
        assert_eq!(takes[2].audio, b"A3");
        assert_eq!(takes[2].latent, b"L3");
    }
}
