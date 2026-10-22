//! Whether this processor runs the native tools built on one ggml-cpu: the
//! MIDI transcriber, the trainer, the stem separator. The engine itself ships
//! a ggml-cpu per processor generation and needs no such check.

/// What the tools' builds take for granted on x86-64.
#[cfg(target_arch = "x86_64")]
const NEEDED: [&str; 4] = ["avx2", "fma", "f16c", "bmi2"];

/// The features this processor lacks of those the tools need.
pub fn missing_features() -> Vec<&'static str> {
    #[cfg(target_arch = "x86_64")]
    {
        NEEDED
            .iter()
            .copied()
            .filter(|feature| match *feature {
                "avx2" => !std::arch::is_x86_feature_detected!("avx2"),
                "fma" => !std::arch::is_x86_feature_detected!("fma"),
                "f16c" => !std::arch::is_x86_feature_detected!("f16c"),
                "bmi2" => !std::arch::is_x86_feature_detected!("bmi2"),
                _ => false,
            })
            .collect()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        Vec::new()
    }
}

/// Refuses to start `tool` on a processor that would stop it at its first
/// instruction, in words the person at the machine can act on.
pub fn tool_runs_here(tool: &str) -> anyhow::Result<()> {
    match refusal(tool, &missing_features()) {
        Some(reason) => anyhow::bail!(reason),
        None => Ok(()),
    }
}

fn refusal(tool: &str, missing: &[&str]) -> Option<String> {
    (!missing.is_empty()).then(|| {
        format!(
            "The {tool} needs a processor with AVX2, FMA, F16C and BMI2 (Intel since 2013, AMD since 2015); this one lacks {}, so it cannot run on this computer.",
            missing.iter().map(|feature| feature.to_uppercase()).collect::<Vec<_>>().join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_processor_without_the_features_is_named_and_refused() {
        assert_eq!(refusal("MIDI transcriber", &[]), None);
        let reason = refusal("MIDI transcriber", &["avx2", "bmi2"]).unwrap();
        assert!(reason.starts_with("The MIDI transcriber needs") && reason.contains("lacks AVX2, BMI2"), "{reason}");
    }
}
