//! What belongs to ACE-Step 1.5 rather than to the engine family.

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

/// What an adapter weight file holds, read from its safetensors header.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct AdapterWeights {
    /// `2b` or `xl` for a DiT adapter, `lm` for one of the planner.
    pub model: Option<String>,
    /// `lora` or `lokr`, when the file is an adapter the engine merges.
    pub format: Option<String>,
    /// Why the engine cannot use the file, when it cannot.
    pub problem: Option<String>,
}

/// Reads what a safetensors header describes. The width of the model comes
/// from an input the adapter shares with it: a query projection or the
/// feed-forward entry, never a cross-attention key, whose input is the text
/// encoder's width in every size.
pub fn describe_adapter(header: &serde_json::Map<String, Value>) -> AdapterWeights {
    let names: Vec<&String> = header.keys().filter(|name| name.as_str() != "__metadata__").collect();
    let shape = |name: &str| -> Vec<u64> {
        header.get(name).and_then(|entry| entry.get("shape")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect()
    };
    let has = |part: &str| names.iter().any(|name| name.contains(part));
    let problem = |text: &str| AdapterWeights { problem: Some(text.into()), ..AdapterWeights::default() };
    if has("lyric_encoder.") || has("transformer_blocks.") {
        return problem("ACE-Step v1");
    }
    if has("conditioners.") {
        return problem("another model");
    }
    if has("hada_w1") {
        return problem("LoHa");
    }
    if has("lora_magnitude_vector") {
        return problem("PEFT DoRA");
    }
    let entry_input = |name: &str| name.contains("q_proj") || name.contains("gate_proj") || name.contains("up_proj");
    let (format, width) = if has(".lokr_w1") {
        let width = names.iter().filter(|name| name.ends_with(".lokr_w1") && entry_input(name)).find_map(|name| {
            let prefix = &name[..name.len() - ".lokr_w1".len()];
            let first = shape(name);
            let second = [format!("{prefix}.lokr_w2"), format!("{prefix}.lokr_w2_b")].iter().map(|other| shape(other)).find(|shape| shape.len() == 2)?;
            (first.len() == 2).then(|| first[1] * second[1])
        });
        ("lokr", width)
    } else if has(".lora_A.") || has(".lora_down.") {
        let width = names
            .iter()
            .filter(|name| (name.contains(".lora_A.") || name.contains(".lora_down.")) && entry_input(name))
            .find_map(|name| shape(name).get(1).copied());
        ("lora", width)
    } else {
        return problem("not an adapter");
    };
    let dit = has("cross_attn") || has("decoder.") || has("diffusion_model.");
    let model = if dit {
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
