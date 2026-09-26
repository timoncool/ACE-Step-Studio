//! What an adapter weight file holds, read from its safetensors header: how it
//! is parameterised, whether the studio engines can merge that, and the width
//! of the model layer it was trained on. Which model that width belongs to is
//! the engine's to say.

use serde_json::{Map, Value};

/// An adapter file as the studio describes it before downloading it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct AdapterWeights {
    /// The size of the model it fits, in the engine's names.
    pub model: Option<String>,
    /// `lora` or `lokr`, when the file is an adapter the engine merges.
    pub format: Option<String>,
    /// Why the engine cannot use the file, when it cannot.
    pub problem: Option<String>,
}

impl AdapterWeights {
    pub fn refused(problem: &str) -> Self {
        Self { problem: Some(problem.into()), ..Self::default() }
    }
}

/// The tensor names of a header, its metadata left out.
pub fn tensor_names(header: &Map<String, Value>) -> Vec<&str> {
    header.keys().map(String::as_str).filter(|name| *name != "__metadata__").collect()
}

fn shape(header: &Map<String, Value>, name: &str) -> Vec<u64> {
    header.get(name).and_then(|entry| entry.get("shape")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect()
}

/// The parameterisation the engines merge, `lora` or `lokr`, or why they
/// cannot: every studio engine takes a low-rank or Kronecker delta and refuses
/// DoRA's magnitude vector and LoHa's Hadamard product.
pub fn adapter_format(header: &Map<String, Value>) -> Result<&'static str, &'static str> {
    let names = tensor_names(header);
    let has = |part: &str| names.iter().any(|name| name.contains(part));
    if has(".hada_w") {
        return Err("LoHa");
    }
    if has("lora_magnitude_vector") {
        return Err("PEFT DoRA");
    }
    if has(".lokr_w1") || has(".lokr_w2") {
        return Ok("lokr");
    }
    if has(".lora_A") || has(".lora_down") {
        return Ok("lora");
    }
    Err("not an adapter")
}

/// The width of the layer the adapter's weights read. It comes from an input
/// every model layer shares with its adapter: a query projection or the entry
/// of a feed-forward block - never a cross-attention key, which reads another
/// encoder's width.
pub fn input_width(header: &Map<String, Value>) -> Option<u64> {
    let names = tensor_names(header);
    let entry_input = |name: &str| name.contains("q_proj") || name.contains("gate_proj") || name.contains("up_proj");
    let lora = names
        .iter()
        .filter(|name| (name.contains(".lora_A") || name.contains(".lora_down")) && entry_input(name))
        .find_map(|name| shape(header, name).get(1).copied());
    lora.or_else(|| {
        // kron(w1, w2) reads as many inputs as the product of its factors' inputs
        names.iter().filter(|name| name.ends_with(".lokr_w1") && entry_input(name)).find_map(|name| {
            let prefix = &name[..name.len() - ".lokr_w1".len()];
            let first = shape(header, name);
            let second = [format!("{prefix}.lokr_w2"), format!("{prefix}.lokr_w2_b")].iter().map(|other| shape(header, other)).find(|shape| shape.len() == 2)?;
            (first.len() == 2).then(|| first[1] * second[1])
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(entries: &[(&str, &[u64])]) -> Map<String, Value> {
        entries.iter().map(|(name, shape)| (name.to_string(), serde_json::json!({ "dtype": "F32", "shape": shape, "data_offsets": [0, 0] }))).collect()
    }

    #[test]
    fn the_width_comes_from_a_query_or_a_feed_forward_input() {
        let attention = header(&[("layers.0.cross_attn.k_proj.lora_A.weight", &[64, 2048]), ("layers.0.cross_attn.q_proj.lora_A.weight", &[64, 2560])]);
        assert_eq!(input_width(&attention), Some(2560));
        let feed_forward = header(&[("decoder.layers.0.mlp.up_proj.lora_down.weight", &[32, 2560])]);
        assert_eq!(input_width(&feed_forward), Some(2560));
        let lokr = header(&[("lycoris_layers_0_self_attn_q_proj.lokr_w1", &[8, 8]), ("lycoris_layers_0_self_attn_q_proj.lokr_w2", &[256, 256])]);
        assert_eq!(input_width(&lokr), Some(2048));
    }

    #[test]
    fn parameterisations_no_engine_merges_are_named() {
        assert_eq!(adapter_format(&header(&[("x.hada_w1_a", &[8, 8])])), Err("LoHa"));
        assert_eq!(adapter_format(&header(&[("x.q_proj.lora_A.weight", &[8, 8]), ("x.q_proj.lora_magnitude_vector", &[8])])), Err("PEFT DoRA"));
        assert_eq!(adapter_format(&header(&[("x.q_proj.lokr_w1", &[8, 8])])), Ok("lokr"));
        assert_eq!(adapter_format(&header(&[("x.q_proj.lora_down.weight", &[8, 8])])), Ok("lora"));
        assert_eq!(adapter_format(&header(&[("decoder.condition_embedder.weight", &[8, 8])])), Err("not an adapter"));
    }
}
