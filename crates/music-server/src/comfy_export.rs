//! An ACE-Step adapter as one ComfyUI LoRA file.
//!
//! The studio's trainer writes a LoKr of the DiT as LyCORIS keys without the
//! module's full path (`lycoris_layers_N_cross_attn_k_proj.lokr_w1`), its
//! scale from the file's lokr_config; a PEFT LoRA comes as
//! `base_model.model.layers.N.<module>.lora_A|B` with adapter_config.json
//! beside it. ComfyUI's ACE-Step 1.5 keeps every projection apart under
//! `diffusion_model.decoder.layers.N`, so the adapter carries over as it is,
//! renamed, with its scale folded into the weights: into w1 of a LoKr, into
//! lora_B of a LoRA, so ComfyUI's own scale is one either way and nothing
//! depends on how it reads alpha.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

/// One tensor read from a safetensors file, as F32.
#[derive(Debug, Clone)]
pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl Tensor {
    fn rows(&self) -> usize {
        self.shape.first().copied().unwrap_or(1)
    }

    fn cols(&self) -> usize {
        self.shape.get(1).copied().unwrap_or(1)
    }

    fn at(&self, row: usize, col: usize) -> f32 {
        self.data[row * self.cols() + col]
    }
}

/// A safetensors file: its tensors as F32 and its metadata.
pub struct SafeTensors {
    pub tensors: BTreeMap<String, Tensor>,
    pub metadata: Map<String, Value>,
}

pub fn read(path: &Path) -> Result<SafeTensors> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let length = u64::from_le_bytes(bytes.get(..8).context("not a safetensors file")?.try_into()?) as usize;
    let header: Map<String, Value> = serde_json::from_slice(bytes.get(8..8 + length).context("a safetensors header past the end of the file")?)?;
    let data = &bytes[8 + length..];
    let mut tensors = BTreeMap::new();
    let mut metadata = Map::new();
    for (name, entry) in header {
        if name == "__metadata__" {
            metadata = entry.as_object().cloned().unwrap_or_default();
            continue;
        }
        let dtype = entry.get("dtype").and_then(Value::as_str).context("a tensor without a dtype")?;
        let shape: Vec<usize> = entry.get("shape").and_then(Value::as_array).context("a tensor without a shape")?.iter().filter_map(Value::as_u64).map(|size| size as usize).collect();
        let offsets: Vec<usize> = entry.get("data_offsets").and_then(Value::as_array).context("a tensor without offsets")?.iter().filter_map(Value::as_u64).map(|offset| offset as usize).collect();
        let raw = data.get(offsets[0]..offsets[1]).with_context(|| format!("{name} lies past the end of the file"))?;
        let values = match dtype {
            "F32" => raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
            "BF16" => raw.chunks_exact(2).map(|b| f32::from_bits((u16::from_le_bytes([b[0], b[1]]) as u32) << 16)).collect(),
            "F16" => raw.chunks_exact(2).map(|b| f16_to_f32(u16::from_le_bytes([b[0], b[1]]))).collect(),
            other => bail!("{name} is {other}; only F32, BF16 and F16 adapters are read"),
        };
        tensors.insert(name, Tensor { shape, data: values });
    }
    Ok(SafeTensors { tensors, metadata })
}

/// Writes BF16 tensors, as trainers ship LoRA, with the metadata as text.
pub fn write(path: &Path, tensors: &BTreeMap<String, Tensor>, metadata: &BTreeMap<String, String>) -> Result<()> {
    let mut header = Map::new();
    header.insert("__metadata__".into(), serde_json::to_value(metadata)?);
    let mut offset = 0usize;
    for (name, tensor) in tensors {
        let size = tensor.data.len() * 2;
        header.insert(name.clone(), serde_json::json!({ "dtype": "BF16", "shape": tensor.shape, "data_offsets": [offset, offset + size] }));
        offset += size;
    }
    let mut json = serde_json::to_vec(&header)?;
    while json.len() % 8 != 0 {
        json.push(b' ');
    }
    let mut out = Vec::with_capacity(8 + json.len() + offset);
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend_from_slice(&json);
    for tensor in tensors.values() {
        for value in &tensor.data {
            out.extend_from_slice(&f32_to_bf16(*value).to_le_bytes());
        }
    }
    let partial = path.with_extension("safetensors.part");
    std::fs::write(&partial, out).with_context(|| format!("write {}", partial.display()))?;
    std::fs::rename(&partial, path).with_context(|| format!("move into {}", path.display()))?;
    Ok(())
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) as u32) << 31;
    let exponent = ((bits >> 10) & 0x1f) as u32;
    let mantissa = (bits & 0x3ff) as u32;
    let value = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, _) => {
            let mut e = 127 - 15 + 1;
            let mut m = mantissa;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x3ff) << 13)
        }
        (0x1f, _) => sign | 0x7f80_0000 | (mantissa << 13),
        _ => sign | ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(value)
}

/// Round to nearest even, as torch casts to bfloat16.
fn f32_to_bf16(value: f32) -> u16 {
    let bits = value.to_bits();
    if value.is_nan() {
        return 0x7fc0;
    }
    let rounding = 0x7fff + ((bits >> 16) & 1);
    ((bits + rounding) >> 16) as u16
}



/// The module path of a LyCORIS name after `lycoris_layers_N_`.
const SITES: [(&str, &str); 11] = [
    ("self_attn_q_proj", "self_attn.q_proj"),
    ("self_attn_k_proj", "self_attn.k_proj"),
    ("self_attn_v_proj", "self_attn.v_proj"),
    ("self_attn_o_proj", "self_attn.o_proj"),
    ("cross_attn_q_proj", "cross_attn.q_proj"),
    ("cross_attn_k_proj", "cross_attn.k_proj"),
    ("cross_attn_v_proj", "cross_attn.v_proj"),
    ("cross_attn_o_proj", "cross_attn.o_proj"),
    ("mlp_gate_proj", "mlp.gate_proj"),
    ("mlp_up_proj", "mlp.up_proj"),
    ("mlp_down_proj", "mlp.down_proj"),
];

fn module_of_lycoris(prefix: &str) -> Result<String> {
    let rest = prefix.strip_prefix("lycoris_layers_").with_context(|| format!("{prefix} is not a DiT layer"))?;
    let (layer, site) = rest.split_once('_').with_context(|| format!("{prefix} has no module"))?;
    let layer: usize = layer.parse().with_context(|| format!("{prefix} has no layer number"))?;
    let path = SITES.iter().find(|(name, _)| *name == site).map(|(_, path)| *path).with_context(|| format!("{prefix}: ComfyUI's ACE-Step has no module {site}"))?;
    Ok(format!("decoder.layers.{layer}.{path}"))
}

/// A trained LoKr file as ComfyUI LoKr tensors, scale folded into w1.
fn lokr(file: &SafeTensors, strength: f32, out: &mut BTreeMap<String, Tensor>) -> Result<()> {
    let config: Value = file.metadata.get("lokr_config").and_then(Value::as_str).and_then(|text| serde_json::from_str(text).ok()).unwrap_or(Value::Null);
    let linear_dim = config.get("linear_dim").and_then(Value::as_f64).map(|value| value as f32);
    let mut groups: BTreeMap<String, BTreeMap<String, Tensor>> = BTreeMap::new();
    for (name, tensor) in &file.tensors {
        let (prefix, part) = name.rsplit_once('.').with_context(|| format!("{name} has no part"))?;
        groups.entry(prefix.to_string()).or_default().insert(part.to_string(), tensor.clone());
    }
    for (prefix, mut parts) in groups {
        let module = module_of_lycoris(&prefix)?;
        if parts.contains_key("dora_scale") || parts.contains_key("lokr_t2") {
            bail!("{prefix}: DoRA and Tucker LoKr are not written for ComfyUI");
        }
        let alpha = parts.remove("alpha").and_then(|tensor| tensor.data.first().copied());
        let w1 = parts.remove("lokr_w1").with_context(|| format!("{prefix} has no lokr_w1"))?;
        // LyCORIS divides alpha by the rank of the factored w2, else by the configured dim
        let dim = parts.get("lokr_w2_b").map(|b| b.rows() as f32).or(linear_dim);
        let scale = strength * match (alpha, dim) {
            (Some(alpha), Some(dim)) if dim > 0.0 => alpha / dim,
            _ => 1.0,
        };
        out.insert(format!("diffusion_model.{module}.lokr_w1"), Tensor { shape: w1.shape.clone(), data: w1.data.iter().map(|value| value * scale).collect() });
        for (part, tensor) in parts {
            if !matches!(part.as_str(), "lokr_w2" | "lokr_w2_a" | "lokr_w2_b") {
                bail!("{prefix}.{part} is not a LoKr part");
            }
            out.insert(format!("diffusion_model.{module}.{part}"), tensor);
        }
    }
    Ok(())
}

/// A PEFT LoRA as ComfyUI LoRA tensors, scale folded into lora_B.
fn peft(file: &SafeTensors, config: &Path, strength: f32, out: &mut BTreeMap<String, Tensor>) -> Result<()> {
    let json: Value = serde_json::from_slice(&std::fs::read(config).with_context(|| format!("read {}", config.display()))?)?;
    let rank = json.get("r").and_then(Value::as_f64).context("adapter_config.json has no r")? as f32;
    let alpha = json.get("lora_alpha").and_then(Value::as_f64).map_or(rank, |value| value as f32);
    let rslora = json.get("use_rslora").and_then(Value::as_bool).unwrap_or(false);
    for (name, tensor) in &file.tensors {
        let rest = name.strip_prefix("base_model.model.").unwrap_or(name);
        let (module, _) = rest.split_once(".lora_").with_context(|| format!("{name} is not a LoRA tensor"))?;
        let module = if module.starts_with("decoder.") { module.to_string() } else { format!("decoder.{module}") };
        if name.contains(".lora_A.") {
            out.insert(format!("diffusion_model.{module}.lora_A.weight"), tensor.clone());
        } else if name.contains(".lora_B.") {
            let own_rank = tensor.cols() as f32;
            let scale = strength * if rslora { alpha / own_rank.sqrt() } else { alpha / own_rank };
            out.insert(format!("diffusion_model.{module}.lora_B.weight"), Tensor { shape: tensor.shape.clone(), data: tensor.data.iter().map(|value| value * scale).collect() });
        } else {
            bail!("{name} is neither lora_A nor lora_B");
        }
    }
    Ok(())
}

/// The trained adapter in `folder` as one ComfyUI LoRA at `out`. Returns how
/// many tensors were written.
pub fn export_ace(folder: &Path, strength: f32, out: &Path, name: &str, trigger: Option<&str>) -> Result<usize> {
    let mut tensors = BTreeMap::new();
    if folder.join("lokr_weights.safetensors").is_file() {
        lokr(&read(&folder.join("lokr_weights.safetensors"))?, strength, &mut tensors)?;
    } else if folder.join("adapter_model.safetensors").is_file() && folder.join("adapter_config.json").is_file() {
        peft(&read(&folder.join("adapter_model.safetensors"))?, &folder.join("adapter_config.json"), strength, &mut tensors)?;
    } else {
        bail!("this adapter is not a LoRA of the studio's trainer, so there is nothing to convert");
    }
    let mut metadata = BTreeMap::new();
    metadata.insert("format".to_string(), "pt".to_string());
    metadata.insert("base_model".to_string(), "ACE-Step 1.5 (ComfyUI native)".to_string());
    metadata.insert("name".to_string(), name.to_string());
    metadata.insert("layout".to_string(), "diffusion_model.decoder.layers.N, projections apart; scale folded into lokr_w1 / lora_B, so ComfyUI's is one".to_string());
    if let Some(trigger) = trigger.filter(|trigger| !trigger.trim().is_empty()) {
        metadata.insert("modelspec.trigger_phrase".to_string(), trigger.to_string());
    }
    let count = tensors.len();
    write(out, &tensors, &metadata)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lycoris_name_finds_its_module() {
        assert_eq!(module_of_lycoris("lycoris_layers_0_cross_attn_k_proj").unwrap(), "decoder.layers.0.cross_attn.k_proj");
        assert_eq!(module_of_lycoris("lycoris_layers_23_mlp_gate_proj").unwrap(), "decoder.layers.23.mlp.gate_proj");
        assert!(module_of_lycoris("lycoris_layers_0_proj_in").is_err());
    }
}
