//! What belongs to ACE-Step 1.5 rather than to the engine family.

use music_core::adapter_weights::{adapter_format, input_width, tensor_names};
pub use music_core::adapter_weights::AdapterWeights;
use serde_json::Value;

/// A part of the model an adapter can change, with its own strength.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct AdapterSlot {
    /// The engine's name for the part, as `/props` adapter_info flags it.
    pub id: &'static str,
    /// What changing that part does to a song, for the interface to name.
    pub role: &'static str,
}

/// ACE-Step's planner LM writes the song - metadata, lyrics, the audio codes
/// that fix its structure - and the DiT renders its sound. An adapter is
/// trained for one of them.
pub const ADAPTER_SLOTS: &[AdapterSlot] =
    &[AdapterSlot { id: "lm", role: "composition" }, AdapterSlot { id: "dit", role: "sound" }];

/// The sizes of the model an adapter can be made for, by the name the
/// interface shows and the width of the DiT: the standard 2B and the XL 4B.
pub const MODEL_FAMILIES: &[(&str, u64)] = &[("2b", 2048), ("xl", 2560)];

/// The size of the DiT a model file holds, from its name: the XL files, the XL
/// merges and the XL fine-tunes say `xl`, the standard ones say nothing.
pub fn model_family(dit_file: &str) -> Option<&'static str> {
    Some(if dit_file.to_ascii_lowercase().contains("xl") { "xl" } else { "2b" })
}

/// What an adapter file is for this engine: an ACE-Step 1.5 adapter of the
/// DiT, sized 2B or XL by the width it reads, or of the planner LM; a file of
/// ACE-Step v1 or of another model is named as such rather than merged into
/// nothing.
pub fn describe_adapter(header: &serde_json::Map<String, Value>) -> AdapterWeights {
    let names = tensor_names(header);
    let has = |part: &str| names.iter().any(|name| name.contains(part));
    if has("lyric_encoder.") || has("transformer_blocks.") {
        return AdapterWeights::refused("ACE-Step v1");
    }
    if has("conditioners.") {
        return AdapterWeights::refused("another model");
    }
    let format = match adapter_format(header) {
        Ok(format) => format,
        Err(problem) => return AdapterWeights::refused(problem),
    };
    let dit = has("cross_attn") || has("decoder.") || has("diffusion_model.");
    let model = if dit {
        let width = input_width(header);
        MODEL_FAMILIES.iter().find(|(_, hidden)| Some(*hidden) == width).map(|(name, _)| *name)
    } else if has("model.layers.") || has("lycoris_layers_") {
        Some("lm")
    } else {
        None
    };
    AdapterWeights { model: model.map(str::to_owned), format: Some(format.into()), problem: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(entries: &[(&str, &[u64])]) -> serde_json::Map<String, Value> {
        entries.iter().map(|(name, shape)| (name.to_string(), serde_json::json!({ "dtype": "F32", "shape": shape, "data_offsets": [0, 0] }))).collect()
    }

    #[test]
    fn the_width_of_a_dit_adapter_comes_from_its_query_projection() {
        let small = header(&[("base_model.model.layers.0.cross_attn.k_proj.lora_A.weight", &[64, 2048]), ("base_model.model.layers.0.self_attn.q_proj.lora_A.weight", &[64, 2048])]);
        assert_eq!(describe_adapter(&small), AdapterWeights { model: Some("2b".into()), format: Some("lora".into()), problem: None });
        // the cross-attention key reads the text encoder, 2048 wide in XL too
        let large = header(&[("base_model.model.layers.0.cross_attn.k_proj.lora_A.weight", &[64, 2048]), ("base_model.model.layers.0.cross_attn.q_proj.lora_A.weight", &[64, 2560])]);
        assert_eq!(describe_adapter(&large).model.as_deref(), Some("xl"));
        let feed_forward = header(&[("diffusion_model.decoder.layers.0.mlp.up_proj.lora_down.weight", &[32, 2560])]);
        assert_eq!(describe_adapter(&feed_forward).model.as_deref(), Some("xl"));
    }

    #[test]
    fn a_lokr_is_as_wide_as_the_product_of_its_factors() {
        let lokr = header(&[("lycoris_layers_0_cross_attn_q_proj.lokr_w1", &[8, 8]), ("lycoris_layers_0_cross_attn_q_proj.lokr_w2", &[256, 256])]);
        let found = describe_adapter(&lokr);
        assert_eq!(found.format.as_deref(), Some("lokr"));
        assert_eq!(found.model.as_deref(), Some("2b"));
    }

    #[test]
    fn weights_the_engine_cannot_merge_say_why() {
        let old = header(&[("lyric_encoder.encoders.0.self_attn.linear_k.lora_A.weight", &[8, 1024])]);
        assert_eq!(describe_adapter(&old).problem.as_deref(), Some("ACE-Step v1"));
        let loha = header(&[("lycoris_layers_0_self_attn_q_proj.hada_w1_a", &[8, 8])]);
        assert_eq!(describe_adapter(&loha).problem.as_deref(), Some("LoHa"));
        let dora = header(&[("base_model.model.layers.0.self_attn.q_proj.lora_A.weight", &[8, 2048]), ("base_model.model.layers.0.self_attn.q_proj.lora_magnitude_vector", &[2048])]);
        assert_eq!(describe_adapter(&dora).problem.as_deref(), Some("PEFT DoRA"));
    }

}
