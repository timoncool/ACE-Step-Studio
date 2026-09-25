use std::path::Path;

fn main() {
    // Tauri embeds the built interface into the executable here. Cargo only
    // re-runs this script when something it was told to watch changes, and the
    // interface is not one of those things by default - so a rebuilt frontend
    // silently stayed out of the binary, and the application kept showing the
    // previous version. Watch the built assets so that cannot happen again.
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if let Ok(entries) = std::fs::read_dir(dist.join("assets")) {
        for entry in entries.flatten() {
            println!("cargo:rerun-if-changed={}", entry.path().display());
        }
    }
    println!("cargo:rerun-if-changed=tauri.conf.json");

    tauri_build::build()
}
