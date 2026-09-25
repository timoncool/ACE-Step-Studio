//! OpenRouter discovery and request construction.
//!
//! Model identifiers intentionally never live in this module.  The settings UI
//! must refresh `GET /api/v1/models`, let the user select a discovered model,
//! then persist that selected id in the provider profile.

use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use music_core::Capability;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const API_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const MODELS_PATH: &str = "/models";
/// Whole families are published only under their own filter; the plain listing
/// omits them. Recognisers were missing entirely, and only eleven of the
/// forty-five image models showed up.
pub const TRANSCRIPTION_MODELS_PATH: &str = "/models?output_modalities=transcription";
pub const IMAGE_MODELS_PATH: &str = "/models?output_modalities=image";
pub const CHAT_COMPLETIONS_PATH: &str = "/chat/completions";
pub const TRANSCRIPTIONS_PATH: &str = "/audio/transcriptions";
pub const IMAGES_PATH: &str = "/images";

/// A serializable request for the HTTP client owned by the server shell.
/// Keeping transport outside the adapter prevents secrets from being stored in
/// a model registry or accidentally emitted in logs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenRouterRequest {
    pub method: HttpMethod,
    pub path: &'static str,
    pub body: Value,
}

/// The credential is intentionally non-serializable and is only obtained by a
/// server handler immediately before it calls OpenRouter. It must never cross
/// the Tauri boundary or become part of a persisted provider selection.
pub struct AuthenticatedOpenRouterRequest {
    pub request: OpenRouterRequest,
    pub api_key: String,
}

pub type OpenRouterMusicStreamRequest = AuthenticatedOpenRouterRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base64AudioInput<'a> {
    /// Ask for per-segment and per-word times.
    pub timestamps: bool,
    /// Raw base64 bytes, without a data-URL prefix.
    pub data: &'a str,
    pub format: &'a str,
    pub language: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelsResponse {
    #[serde(default)]
    pub data: Vec<RemoteModel>,
}

/// Sampling a model publishes for itself, in OpenRouter's `default_parameters`.
/// Sending one hardcoded temperature to every model overrides what the model
/// asks for; 83 of them fill this in.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct ModelDefaults {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<i64>,
    pub frequency_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    pub repetition_penalty: Option<f64>,
}

impl ModelDefaults {
    /// True when the model published nothing at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub supported_parameters: Vec<String>,
    #[serde(default)]
    pub pricing: Option<ModelPricing>,
    #[serde(default)]
    pub architecture: ModelArchitecture,
    #[serde(default)]
    pub default_parameters: ModelDefaults,
    /// What the model says about thinking, in its own terms. Not part of
    /// `default_parameters`: it is a field of its own, and ignoring it is how a
    /// request asks for an effort the model does not take.
    #[serde(default)]
    pub reasoning: Option<ReasoningSupport>,
}

/// A model's own rules for reasoning, as OpenRouter publishes them.
///
/// `deepseek/deepseek-v4-pro-0813` takes max, high or low - not medium.
/// `z-ai/glm-5.3` reasons whether or not anyone asks. Sending an effort from a
/// list of our own invention is asking for something that was never offered.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct ReasoningSupport {
    /// What the model uses when nothing is asked for.
    pub default_effort: Option<String>,
    /// Whether it thinks unless told otherwise.
    pub default_enabled: Option<bool>,
    /// Whether it can be told otherwise at all.
    pub mandatory: bool,
    /// The only values it accepts.
    pub supported_efforts: Vec<String>,
}

impl ReasoningSupport {
    /// The effort to send for a wanted setting, or nothing at all.
    ///
    /// An effort the model does not list becomes the one it prefers, because a
    /// request that names an unknown effort is refused outright - and a model
    /// that must think is not told to stop.
    pub fn effort_for(&self, wanted: Option<&str>) -> Option<String> {
        let wanted = wanted.map(str::trim).filter(|value| !value.is_empty());
        if matches!(wanted, Some("off")) {
            return if self.mandatory { self.default_effort.clone() } else { None };
        }
        let wanted = wanted?;
        if self.supported_efforts.is_empty() || self.supported_efforts.iter().any(|value| value == wanted) {
            return Some(wanted.to_string());
        }
        self.default_effort.clone().or_else(|| self.supported_efforts.first().cloned())
    }
}

/// Prices are decimal strings in the upstream API. Preserve that representation
/// so the UI never introduces floating-point rounding into cost estimates.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelPricing {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub completion: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub request: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelArchitecture {
    #[serde(default)]
    pub modality: Option<String>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub output_modalities: Vec<String>,
}

/// User-facing model metadata derived exclusively from the current OpenRouter
/// catalog. `capabilities` means *eligible by declared I/O modalities*, not a
/// guarantee that the model provides a particular product feature.
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct CatalogModel {
    /// What this model says its own sampling should be.
    #[serde(default)]
    pub defaults: ModelDefaults,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub context_length: Option<u64>,
    pub modality: Option<String>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub supported_parameters: Vec<String>,
    pub pricing: Option<ModelPricing>,
    pub capabilities: Vec<Capability>,
    /// What this model will accept as thinking, in its own words.
    pub reasoning: Option<ReasoningSupport>,
}

impl CatalogModel {
    fn from_remote(model: RemoteModel) -> Self {
        let capabilities = infer_capabilities(&model);
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
            context_length: model.context_length,
            modality: model.architecture.modality,
            input_modalities: normalized(model.architecture.input_modalities),
            output_modalities: normalized(model.architecture.output_modalities),
            supported_parameters: normalized(model.supported_parameters),
            pricing: model.pricing,
            defaults: model.default_parameters,
            reasoning: model.reasoning,
            capabilities,
        }
    }

    pub fn supports(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct CapabilityCatalog {
    pub models: Vec<CatalogModel>,
}

impl CapabilityCatalog {
    pub fn from_models_response(response: ModelsResponse) -> Self {
        let mut models: Vec<_> = response
            .data
            .into_iter()
            .map(CatalogModel::from_remote)
            .filter(|model| !model.capabilities.is_empty())
            .collect();
        models.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        Self { models }
    }

    pub fn parse(body: &str) -> Result<Self> {
        let response: ModelsResponse = serde_json::from_str(body).context("invalid OpenRouter models response")?;
        Ok(Self::from_models_response(response))
    }

    /// Merges the per-modality listings into the general one. A catalog built
    /// from the plain listing alone knows of no model that can return timings,
    /// and of a quarter of the image models.
    pub fn parse_merged(general: &str, extras: &[&str]) -> Result<Self> {
        let mut catalog = Self::parse(general)?;
        let mut known: std::collections::BTreeSet<String> = catalog.models.iter().map(|model| model.id.clone()).collect();
        for body in extras {
            if body.trim().is_empty() {
                continue;
            }
            let Ok(extra) = serde_json::from_str::<ModelsResponse>(body) else { continue };
            for model in Self::from_models_response(extra).models {
                if known.insert(model.id.clone()) {
                    catalog.models.push(model);
                }
            }
        }
        catalog.models.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(catalog)
    }

    pub fn models_for(&self, capability: Capability) -> impl Iterator<Item = &CatalogModel> {
        self.models.iter().filter(move |model| model.supports(capability))
    }

    pub fn selected(&self, capability: Capability, model_id: &str) -> Result<&CatalogModel> {
        let model = self
            .models
            .iter()
            .find(|model| model.id == model_id)
            .with_context(|| format!("OpenRouter model '{model_id}' is not present in the refreshed catalog"))?;
        if !model.supports(capability) {
            bail!("OpenRouter model '{model_id}' does not declare support for {capability:?}");
        }
        Ok(model)
    }
}

pub fn models_request() -> OpenRouterRequest {
    OpenRouterRequest {
        method: HttpMethod::Get,
        path: MODELS_PATH,
        body: Value::Null,
    }
}

/// The second listing the catalog needs: dedicated transcription models.
pub fn transcription_models_request() -> OpenRouterRequest {
    OpenRouterRequest {
        method: HttpMethod::Get,
        path: TRANSCRIPTION_MODELS_PATH,
        body: Value::Null,
    }
}

/// Build the exact request shape for a catalog-backed selection. The HTTP
/// layer supplies the Authorization bearer credential from secure storage.
pub fn request_for(
    catalog: &CapabilityCatalog,
    capability: Capability,
    model_id: &str,
    prompt: &str,
) -> Result<OpenRouterRequest> {
    let model = catalog.selected(capability, model_id)?;
    let prompt = prompt.trim();
    if prompt.is_empty() {
        bail!("OpenRouter prompt must not be empty");
    }

    let (path, body) = match capability {
        Capability::PromptEnhancement => (
            CHAT_COMPLETIONS_PATH,
            json!({
                "model": model.id,
                "messages": [{ "role": "user", "content": prompt }],
                "stream": false,
            }),
        ),
        // A cover is square. Without asking, image models answer in their own
        // habit - the first covers came back 1408x768 and every card cropped
        // them to a strip.
        Capability::CoverArt => (
            IMAGES_PATH,
            json!({ "model": model.id, "prompt": prompt, "size": "1024x1024" }),
        ),
        Capability::SpeechToText => bail!(
            "use stt_request_for with base64 audio; a text prompt is not a valid OpenRouter transcription input"
        ),
        Capability::MusicGeneration => (
            CHAT_COMPLETIONS_PATH,
            json!({
                "model": model.id,
                "messages": [{ "role": "user", "content": prompt }],
                "stream": true,
                "modalities": ["text", "audio"],
                "audio": { "format": "wav" },
            }),
        ),
    };

    Ok(OpenRouterRequest {
        method: HttpMethod::Post,
        path,
        body,
    })
}

/// Builds the documented OpenRouter streaming chat request for a catalog
/// verified music model. The caller must attach this key as a Bearer token and
/// consume SSE audio chunks; this adapter never starts a paid generation.
pub fn music_stream_request_for(
    catalog: &CapabilityCatalog,
    model_id: &str,
    prompt: &str,
) -> Result<OpenRouterMusicStreamRequest> {
    authenticated_request_for(
        request_for(catalog, Capability::MusicGeneration, model_id, prompt)?,
    )
}

/// Add the locally-held credential to an already catalog-validated request.
/// The API key source is deliberately centralized so every cloud capability
/// follows the same boundary and never accepts secrets from the frontend.
pub fn authenticated_request_for(request: OpenRouterRequest) -> Result<AuthenticatedOpenRouterRequest> {
    let (api_key, _source) = crate::credentials::openrouter_api_key().context(
        "No OpenRouter API key is configured. Add one in Studio settings, or export OPENROUTER_API_KEY before starting the server.",
    )?;
    Ok(AuthenticatedOpenRouterRequest { request, api_key })
}

/// Construct the JSON request specified by OpenRouter's `/audio/transcriptions`
/// endpoint. Audio bytes are supplied by the media layer and must not be put
/// into the provider profile.

/// A sensible model for a capability, so adding a key is enough to start.
///
/// Picking a model out of four hundred is not a decision a user should have to
/// make before hearing anything. These are ordered preferences, checked
/// against the live catalog, and every one of them is a model that actually
/// does the job: Whisper for transcription because only the Whisper family
/// returns timings, an image model that takes a plain prompt, and a fast,
/// inexpensive text model for the writing assistant. Anything chosen by the
/// user always wins over this.
pub fn suggested_model(catalog: &CapabilityCatalog, capability: Capability) -> Option<String> {
    const PREFERENCES: &[(Capability, &[&str])] = &[
        (
            Capability::SpeechToText,
            &["openai/whisper-large-v3-turbo", "openai/whisper-large-v3", "openai/whisper-1"],
        ),
        (
            Capability::CoverArt,
            &["google/gemini-3.1-flash-image", "google/gemini-2.5-flash-image", "openai/gpt-5-image-mini"],
        ),
        (
            Capability::PromptEnhancement,
            &["google/gemini-3.1-flash-lite", "google/gemini-2.5-flash", "anthropic/claude-haiku-4.5", "deepseek/deepseek-chat"],
        ),
        (Capability::MusicGeneration, &["google/lyria-3-pro-preview", "google/lyria-3-clip-preview"]),
    ];

    let preferred = PREFERENCES.iter().find(|(entry, _)| *entry == capability).map(|(_, list)| *list)?;
    for id in preferred {
        if catalog.models_for(capability).any(|model| model.id == *id) {
            return Some((*id).to_string());
        }
    }
    // Nothing preferred is available: the first model that declares the
    // capability is still better than asking the user to guess.
    catalog.models_for(capability).next().map(|model| model.id.clone())
}

pub fn stt_request_for(
    catalog: &CapabilityCatalog,
    model_id: &str,
    audio: Base64AudioInput<'_>,
) -> Result<OpenRouterRequest> {
    // The catalog is a convenience for the picker, not the authority on what
    // the endpoint accepts: OpenRouter's transcription models - the Whisper
    // family, the only ones that return timings - are documented but are not
    // published in /models at all. A typed model id is therefore allowed
    // through, and OpenRouter itself rejects a wrong one.
    if model_id.trim().is_empty() {
        bail!("OpenRouter transcription needs a model id");
    }
    // A model the catalog knows must actually declare the capability; one it
    // has never heard of is allowed through, because OpenRouter publishes its
    // recognisers only behind a separate filter and a stale catalog should not
    // block a valid model. A wrong id is then rejected by OpenRouter itself.
    if catalog.models.iter().any(|model| model.id == model_id) {
        catalog.selected(Capability::SpeechToText, model_id)?;
    }
    if audio.data.trim().is_empty() || audio.format.trim().is_empty() {
        bail!("OpenRouter transcription requires base64 audio data and an audio format");
    }
    let mut body = json!({
        "model": model_id,
        "input_audio": { "data": audio.data, "format": audio.format },
    });
    if let Some(language) = audio.language.filter(|language| !language.trim().is_empty()) {
        body["language"] = Value::String(language.to_owned());
    }
    if audio.timestamps {
        // Karaoke needs times, and a plain transcription carries none. This is
        // the OpenAI-compatible way to ask for them; a model that ignores it
        // answers without segments and the caller says so plainly.
        body["response_format"] = Value::String("verbose_json".into());
        body["timestamp_granularities"] = json!(["segment", "word"]);
    }
    Ok(OpenRouterRequest {
        method: HttpMethod::Post,
        path: TRANSCRIPTIONS_PATH,
        body,
    })
}

fn infer_capabilities(model: &RemoteModel) -> Vec<Capability> {
    let input: BTreeSet<_> = normalized(model.architecture.input_modalities.clone()).into_iter().collect();
    let output: BTreeSet<_> = normalized(model.architecture.output_modalities.clone()).into_iter().collect();
    let mut capabilities = Vec::new();

    if input.contains("text") && output.contains("text") {
        capabilities.push(Capability::PromptEnhancement);
    }
    // A dedicated recogniser declares "transcription", not "text": OpenRouter
    // lists those models only under output_modalities=transcription, and they
    // are the ones that return timings at all.
    if input.contains("audio") && (output.contains("text") || output.contains("transcription")) {
        capabilities.push(Capability::SpeechToText);
    }
    if input.contains("text") && output.contains("image") {
        capabilities.push(Capability::CoverArt);
    }
    // Generic audio is insufficient: it may mean TTS. OpenRouter's models API
    // exposes the needed product evidence in title/description for models such
    // as music generators, while the streaming endpoint remains common.
    if input.contains("text") && output.contains("audio") && music_evidence(model) {
        capabilities.push(Capability::MusicGeneration);
    }
    capabilities
}

fn music_evidence(model: &RemoteModel) -> bool {
    let text = format!("{} {}", model.name, model.description.as_deref().unwrap_or_default()).to_ascii_lowercase();
    ["music generation", "music generator", "generate music", "full-length song", "full length song", "songs", "song generation"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn normalized(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {

    /// The rules are the model's, and these are real ones, copied from the
    /// live catalogue rather than imagined.
    #[test]
    fn an_effort_a_model_does_not_take_becomes_the_one_it_named() {
        // deepseek/deepseek-v4-pro-0813 as published: max, high or low.
        let deepseek = ReasoningSupport {
            default_effort: Some("high".into()),
            default_enabled: None,
            mandatory: false,
            supported_efforts: vec!["max".into(), "high".into(), "low".into()],
        };
        assert_eq!(deepseek.effort_for(Some("max")).as_deref(), Some("max"), "a listed effort is sent as asked");
        assert_eq!(deepseek.effort_for(Some("medium")).as_deref(), Some("high"), "an unlisted one becomes its own default");
        assert_eq!(deepseek.effort_for(Some("off")), None, "a model that may stay quiet is allowed to");
        assert_eq!(deepseek.effort_for(None), None);

        // z-ai/glm-5.3 thinks whether or not anyone asks.
        let glm = ReasoningSupport {
            default_effort: Some("max".into()),
            default_enabled: Some(true),
            mandatory: true,
            supported_efforts: vec!["max".into(), "high".into(), "low".into()],
        };
        assert_eq!(glm.effort_for(Some("off")).as_deref(), Some("max"), "a mandatory thinker is not told to stop");
        assert_eq!(glm.effort_for(Some("medium")).as_deref(), Some("max"));

        // A model that publishes no list takes what it is given.
        let unknown = ReasoningSupport::default();
        assert_eq!(unknown.effort_for(Some("high")).as_deref(), Some("high"));
    }
    use super::*;

    #[test]
    fn catalog_derives_only_declared_capabilities() {
        let catalog = CapabilityCatalog::parse(
            r#"{"data":[
                {"id":"catalog/text","name":"Text","architecture":{"input_modalities":["text"],"output_modalities":["text"]}},
                {"id":"catalog/stt","name":"STT","architecture":{"input_modalities":["audio"],"output_modalities":["text"]}},
                {"id":"catalog/image","name":"Image","architecture":{"input_modalities":["text"],"output_modalities":["image"]}},
                {"id":"catalog/audio","name":"Audio TTS","description":"Speech synthesis","architecture":{"input_modalities":["text"],"output_modalities":["audio"]}},
                {"id":"catalog/music","name":"Lyria Preview","description":"Generate full-length songs from text prompts.","architecture":{"input_modalities":["text"],"output_modalities":["audio"]}}
            ]}"#,
        )
        .unwrap();

        assert_eq!(catalog.models_for(Capability::PromptEnhancement).count(), 1);
        assert_eq!(catalog.models_for(Capability::SpeechToText).count(), 1);
        assert_eq!(catalog.models_for(Capability::CoverArt).count(), 1);
        assert_eq!(catalog.models_for(Capability::MusicGeneration).count(), 1);
    }

    #[test]
    fn music_request_uses_documented_streaming_chat_shape() {
        let catalog = CapabilityCatalog::parse(r#"{"data":[{"id":"catalog/music","name":"Song creator","description":"Music generation for full-length songs","architecture":{"input_modalities":["text"],"output_modalities":["audio"]}}]}"#).unwrap();
        let request = request_for(&catalog, Capability::MusicGeneration, "catalog/music", "dream pop").unwrap();
        assert_eq!(request.path, CHAT_COMPLETIONS_PATH);
        assert_eq!(request.body["stream"], true);
        assert_eq!(request.body["modalities"], json!(["text", "audio"]));
    }

    #[test]
    fn audio_only_tts_is_not_music() {
        let catalog = CapabilityCatalog::parse(r#"{"data":[{"id":"catalog/tts","name":"Voice TTS","description":"Natural speech synthesis","architecture":{"input_modalities":["text"],"output_modalities":["audio"]}}]}"#).unwrap();
        assert_eq!(catalog.models_for(Capability::MusicGeneration).count(), 0);
    }

    #[test]
    fn request_requires_a_refreshed_eligible_model() {
        let catalog = CapabilityCatalog::parse(
            r#"{"data":[{"id":"catalog/text","name":"Text","architecture":{"input_modalities":["text"],"output_modalities":["text"]}}]}"#,
        )
        .unwrap();

        let request = request_for(&catalog, Capability::PromptEnhancement, "catalog/text", "make a chorus").unwrap();
        assert_eq!(request.path, CHAT_COMPLETIONS_PATH);
        assert_eq!(request.body["model"], "catalog/text");
        assert!(request_for(&catalog, Capability::CoverArt, "catalog/text", "cover").is_err());
    }

    #[test]
    fn transcription_uses_openrouter_audio_input_shape() {
        let catalog = CapabilityCatalog::parse(
            r#"{"data":[{"id":"catalog/stt","name":"STT","architecture":{"input_modalities":["audio"],"output_modalities":["text"]}}]}"#,
        )
        .unwrap();
        let request = stt_request_for(
            &catalog,
            "catalog/stt",
            Base64AudioInput { timestamps: true, data: "UklGRg==", format: "wav", language: Some("en") },
        )
        .unwrap();
        assert_eq!(request.path, TRANSCRIPTIONS_PATH);
        assert_eq!(request.body["input_audio"]["format"], "wav");
        assert_eq!(request.body["language"], "en");
    }

    #[test]
    fn cover_uses_the_dedicated_image_api_and_a_catalog_selected_model() {
        let catalog = CapabilityCatalog::parse(
            r#"{"data":[{"id":"catalog/image","name":"Image","architecture":{"input_modalities":["text"],"output_modalities":["image"]}}]}"#,
        )
        .unwrap();

        let request = request_for(&catalog, Capability::CoverArt, "catalog/image", "retro album cover").unwrap();
        assert_eq!(request.path, IMAGES_PATH);
        // A cover is asked for square: every image model takes 1024x1024.
        assert_eq!(request.body, json!({"model":"catalog/image","prompt":"retro album cover","size":"1024x1024"}));
    }

    #[test]
    fn transcription_rejects_a_model_that_is_not_declared_for_audio_to_text() {
        let catalog = CapabilityCatalog::parse(
            r#"{"data":[{"id":"catalog/image","name":"Image","architecture":{"input_modalities":["text"],"output_modalities":["image"]}}]}"#,
        )
        .unwrap();

        assert!(stt_request_for(
            &catalog,
            "catalog/image",
            Base64AudioInput { timestamps: false, data: "UklGRg==", format: "wav", language: None },
        )
        .is_err());
    }
}
