//! Runs one processing stage on WAV files, for checking against reference
//! implementations and listening.
//!
//!   process master <target.wav> <reference.wav> <out.wav>
//!   process denoise|lifter|naturalize <in.wav> <out.wav>
//!   process quality <in.wav>

use audio_post::{denoise, lifter, mastering, naturalize, quality, Stereo};

fn read(path: &str) -> Stereo {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.unwrap() as f32 / scale).collect()
        }
    };
    let channels = spec.channels as usize;
    let left: Vec<f32> = samples.chunks(channels).map(|f| f[0]).collect();
    let right: Vec<f32> = samples.chunks(channels).map(|f| f[channels.min(2) - 1]).collect();
    Stereo::new(left, right, spec.sample_rate)
}

fn write(path: &str, audio: &Stereo) {
    let spec = hound::WavSpec { channels: 2, sample_rate: audio.rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for (l, r) in audio.left.iter().zip(&audio.right) {
        writer.write_sample(*l).unwrap();
        writer.write_sample(*r).unwrap();
    }
    writer.finalize().unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let started = std::time::Instant::now();
    match args[1].as_str() {
        "master" => {
            let out = mastering::master(&read(&args[2]), &read(&args[3]), &mastering::MasteringConfig::default()).expect("master");
            write(&args[4], &out);
        }
        "denoise" => write(&args[3], &denoise::denoise(&read(&args[2]), &denoise::DenoiseSettings::default())),
        "lifter" => write(&args[3], &lifter::lift(&read(&args[2]), &lifter::LifterSettings::default())),
        "naturalize" => write(&args[3], &naturalize::naturalize(&read(&args[2]), &naturalize::NaturalizeSettings::default())),
        "quality" => println!("{}", serde_json::to_string(&quality::evaluate(&read(&args[2]))).unwrap()),
        other => panic!("unknown stage {other}"),
    }
    eprintln!("{} in {:.2} s", args[1], started.elapsed().as_secs_f64());
}
