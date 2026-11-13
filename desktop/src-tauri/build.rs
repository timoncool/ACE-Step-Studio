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
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");

    // Tauri's own manifest with the console allocation policy beside it: started from Explorer on Windows 11 24H2
    // and later, the studio gets no console of its own (no window, no Terminal tab) and makes a windowless one itself.
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app-manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("the Tauri build step");
}
