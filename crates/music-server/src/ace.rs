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
    "audio_codes", "inference_steps", "guidance_scale", "shift",
    "dcw_scaler", "dcw_high_scaler", "dcw_mode",
    "audio_cover_strength", "cover_noise_strength", "repainting_start", "repainting_end",
    "latent_shift", "latent_rescale", "custom_timesteps", "task_type", "track",
    "solver", "stork_substeps", "jkass_beat_stability", "jkass_frequency_damping", "jkass_temporal_smoothing",
    "scheduler", "guidance", "apg_momentum", "apg_norm_threshold", "cfg_zero_init_steps", "smc_lambda", "smc_k",
    "cfg_mp_iterations", "lm_mode", "output_format", "synth_model", "lm_model", "vae",
    "adapter", "adapter_scale", "lm_adapter", "lm_adapter_scale", "adapters", "adapter_group_scales",
    "peak_clip", "mp3_bitrate", "cfg_interval_start", "cfg_interval_end", "retake_seed", "retake_variance",
];

pub const TASKS: &[&str] = &["text2music", "cover", "cover-nofsq", "repaint", "lego", "extract", "complete"];

/// Tasks that work on a source recording.
pub fn needs_source(task: &str) -> bool {
    task != "text2music"
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
    for (key, allowed) in [("solver", SOLVERS), ("guidance", GUIDANCE)] {
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
/// take; the engine counts the takes' seeds up from it.
pub fn synth_requests(planned: Vec<Value>, encode_here: bool) -> Vec<Value> {
    planned
        .into_iter()
        .map(|mut request| {
            if let Some(fields) = request.as_object_mut() {
                fields.remove("lm_batch_size");
                if encode_here {
                    fields.insert("output_format".into(), Value::from("wav32"));
                    fields.remove("mp3_bitrate");
                }
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
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(500))
            .build()
            .expect("the HTTP client builds with a connect timeout");
        Self { base_url, http }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
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

    /// `/logs` replays the log ring and then streams forever; the ring is read
    /// until it goes quiet.
    pub async fn logs_snapshot(&self, quiet_period: Duration) -> Result<Vec<String>> {
        use futures_util::StreamExt;
        let response = self.http.get(self.url("/logs")).send().await?;
        let status = response.status();
        if !status.is_success() {
            bail!("the engine returned {status} for /logs");
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(quiet_period.min(remaining), stream.next()).await {
                Ok(Some(chunk)) => buffer.extend_from_slice(&chunk?),
                Ok(None) | Err(_) => break,
            }
        }
        Ok(String::from_utf8_lossy(&buffer)
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect())
    }

    /// Starts a language-model job over one or several requests.
    pub async fn submit_lm(&self, requests: &[Value]) -> Result<String> {
        let body: Value = if requests.len() == 1 { requests[0].clone() } else { Value::Array(requests.to_vec()) };
        job_id(self.http.post(self.url("/lm")).json(&body).send().await?).await
    }

    /// Starts a synthesis job; the sources, when any, go as multipart parts.
    pub async fn submit_synth(&self, requests: &[Value], sources: Sources) -> Result<String> {
        let body = Value::Array(requests.to_vec());
        if sources.is_empty() {
            return job_id(self.http.post(self.url("/synth")).json(&body).send().await?).await;
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
        let answer: Value = json_of(self.http.get(self.url("/job")).query(&[("id", job)]).send().await?).await?;
        answer.get("status").and_then(Value::as_str).map(str::to_owned).context("the engine's job answer has no status")
    }

    pub async fn cancel(&self, job: &str) -> Result<()> {
        let response = self.http.post(self.url("/job")).query(&[("id", job), ("cancel", "1")]).send().await?;
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
        let response = self.http.get(self.url("/job")).query(&[("id", job), ("result", "1")]).send().await?;
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
