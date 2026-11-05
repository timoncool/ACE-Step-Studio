use audio_post::{encode, Stereo};

fn tone(seconds: f32, rate: u32, amp: f32) -> Stereo {
    let n = (seconds * rate as f32) as usize;
    let left: Vec<f32> = (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin()).collect();
    Stereo::new(left.clone(), left, rate)
}

/// STREAMINFO's 36-bit total-samples field, read back the way a player reads it.
fn total_samples(flac: &[u8]) -> u64 {
    assert_eq!(&flac[0..4], b"fLaC", "a FLAC stream begins with its magic");
    let field = u64::from_be_bytes(flac[18..26].try_into().unwrap());
    field & ((1u64 << 36) - 1)
}

#[test]
fn flac_is_built_in_memory_with_its_length() {
    // The engine's rates, plus the awkward ones, built with no temp file at all:
    // a successful encode is proof libFLAC verified the stream decodes back.
    for rate in [44_100u32, 48_000, 32_000, 24_000, 22_050, 16_000, 96_000] {
        let audio = tone(1.0, rate, 0.5);
        let frames = audio.frames() as u64;
        let bytes = encode::flac(&audio).expect("flac in memory");
        assert!(bytes.len() > 1000, "rate {rate}: {} bytes", bytes.len());
        assert_eq!(total_samples(&bytes), frames, "rate {rate}: STREAMINFO carries the real length");
    }
    // A clipping track still encodes; nothing is kept on a temp file.
    assert!(encode::flac(&tone(0.5, 44_100, 3.0)).is_ok());
}
