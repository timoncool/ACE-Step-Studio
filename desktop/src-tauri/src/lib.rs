//! Desktop shell of the studio named in `studio.json`.
//!
//! The whole studio is this one executable: the window, and the native service
//! hosted inside it. The service in turn supervises the `acestep.cpp`
//! engine, so there is exactly one owner of that process and no launcher
//! script in the release layout. If a compatible service is already listening
//! on loopback — a developer running it separately — the shell uses it instead
//! of starting a second one.

use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use music_core::studio;

fn server_port() -> u16 {
    studio().port
}

/// Mutable studio data must not be derived from the process working directory
/// or from the installed executable directory: a Start Menu shortcut is free
/// to choose either of those. Environment overrides remain authoritative for
/// portable/development installations.
fn studio_data_directory() -> PathBuf {
    // A portable copy keeps everything it owns beside itself: models, the
    // library, media, logs and settings. Nothing is written into AppData, so
    // deleting the folder deletes the studio, and carrying the folder to
    // another machine carries the whole studio with it.
    if is_portable() {
        return executable_directory().join("data");
    }

    // An installation is treated the same way whenever it can be: someone who
    // installs into F:\AI expects the twenty-five gigabytes of weights to land
    // in F:\AI, not in their profile on C:. Only when the install directory
    // cannot be written to - Program Files, a read-only share - does the studio
    // fall back to AppData, because then it has nowhere else to go.
    // On macOS the executable sits inside a signed .app bundle that must not
    // be written to, so an installation always uses Application Support.
    let beside_the_executable = executable_directory().join("data");
    if !cfg!(target_os = "macos") && directory_is_writable(&beside_the_executable) {
        return beside_the_executable;
    }

    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")) {
            return PathBuf::from(root).join(&studio().name);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library").join("Application Support").join(&studio().name);
        }
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(root).join(&studio().slug);
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(&studio().slug);
        }
    }

    std::env::temp_dir().join(&studio().slug)
}

/// A portable copy, or an installation into a folder it can write to: then the
/// data, the temporary files and the WebView2 profile all live beside the
/// executable, and deleting the folder deletes the studio.
fn keeps_everything_beside_itself() -> bool {
    studio_data_directory().starts_with(executable_directory())
}

/// Sets the same deterministic roots before the Axum bridge is spawned. Its
/// environment is inherited by the bridge and the subsequently started native
/// engine, so both always refer to one model library.
fn configure_studio_runtime_paths() {
    let data_root = studio_data_directory();

    // Temporary files count as leaving traces too: the engine, the downloader
    // and ffmpeg all write through the system temporary directory, and a
    // studio that keeps its data beside itself has no business filling the
    // system drive with them.
    if keeps_everything_beside_itself() {
        let temporary = executable_directory().join("temp");
        let _ = std::fs::create_dir_all(&temporary);
        for variable in ["TEMP", "TMP"] {
            unsafe {
                std::env::set_var(variable, &temporary);
            }
        }
    }
    if std::env::var_os("STUDIO_MODELS_ROOT").is_none() {
        unsafe {
            std::env::set_var(
                "STUDIO_MODELS_ROOT",
                data_root.join("models").join(studio().default_engine()),
            );
        }
    }
    if std::env::var_os("STUDIO_SETTINGS_PATH").is_none() {
        unsafe {
            std::env::set_var("STUDIO_SETTINGS_PATH", data_root.join("studio-settings.json"));
        }
    }
    // Without this the service falls back to `<working directory>/data` for the
    // library and media files. A Start Menu shortcut does not control the
    // working directory, so an installed build would scatter or lose the user's
    // library depending on how it was launched.
    if std::env::var_os("STUDIO_DATA_ROOT").is_none() {
        unsafe {
            std::env::set_var("STUDIO_DATA_ROOT", &data_root);
        }
    }

    // The service resolves and supervises the engine; the shell only needs the
    // loopback address they agree on.
    let host = std::env::var("STUDIO_ENGINE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("STUDIO_ENGINE_PORT").ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or_else(music_engine::server::default_port);
    if std::env::var_os("STUDIO_ENGINE_URL").is_none() {
        unsafe {
            std::env::set_var("STUDIO_ENGINE_URL", format!("http://{host}:{port}"));
        }
    }
}

fn service_is_ready() -> bool {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, server_port());
    TcpStream::connect_timeout(&address.into(), Duration::from_millis(150)).is_ok()
}

fn wait_until_ready(timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if service_is_ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    false
}

fn executable_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn is_portable() -> bool {
    executable_directory().join("portable.flag").is_file()
}

/// Whether the studio may keep its data here.
///
/// Asked by creating the directory and writing to it, because that is the only
/// answer that matters: permissions on Windows are not something to reason
/// about from a path.
fn directory_is_writable(path: &std::path::Path) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(".write-probe");
    let written = std::fs::write(&probe, b"1").is_ok();
    let _ = std::fs::remove_file(&probe);
    written
}

/// Hosts the studio service inside this process.
///
/// A release is one executable: there is no second binary to locate, no
/// launcher script, and nothing that can be left running if the window is
/// closed. The service is started on its own runtime thread and the window
/// only opens once it answers on loopback.
fn start_service() -> Result<(), String> {
    if service_is_ready() {
        return Ok(());
    }

    std::thread::Builder::new()
        .name("music-server".into())
        .spawn(|| {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("could not create the studio service runtime: {error}");
                    return;
                }
            };
            // Closing the studio and opening it again leaves the previous
            // process holding the port for a moment. Giving up on the first
            // refusal left the window on a browser error page with no way back
            // except restarting the application.
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                match runtime.block_on(music_server::serve()) {
                    Ok(()) => return,
                    Err(error) if Instant::now() < deadline => {
                        eprintln!("studio service could not start yet: {error}");
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    Err(error) => {
                        eprintln!("studio service stopped: {error}");
                        return;
                    }
                }
            }
        })
        .map_err(|error| format!("could not start the studio service thread: {error}"))?;

    if wait_until_ready(Duration::from_secs(30)) {
        Ok(())
    } else {
        Err(format!("the studio service did not become ready on 127.0.0.1:{}", server_port()))
    }
}

/// One request to the studio's own service: its body; `Err(true)` when it took
/// the request and gave nothing readable back, `Err(false)` when nothing listens.
fn service_call(method: &str, path: &str, wait: Duration) -> Result<String, bool> {
    use std::io::{Read, Write};
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, server_port());
    let Ok(mut stream) = TcpStream::connect_timeout(&address.into(), Duration::from_millis(500)) else {
        return Err(false);
    };
    let _ = stream.set_read_timeout(Some(wait));
    let request = format!("{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\n\r\n", server_port());
    let mut response = String::new();
    if stream.write_all(request.as_bytes()).is_err() || stream.read_to_string(&mut response).is_err() {
        return Err(true);
    }
    response.split("\r\n\r\n").nth(1).map(str::to_owned).ok_or(true)
}

/// The songs queued or being made, by job id; `None` when the service could not say in `wait`.
fn songs_in_progress(wait: Duration) -> Option<Vec<String>> {
    match service_call("GET", "/v1/music/jobs", wait) {
        Ok(body) => serde_json::from_str::<Vec<serde_json::Value>>(&body)
            .ok()
            .map(|jobs| jobs.iter().filter_map(|job| job.get("id").and_then(serde_json::Value::as_str).map(str::to_owned)).collect()),
        Err(false) => Some(Vec::new()),
        Err(true) => None,
    }
}

/// What the shell asks the window and is still waiting on: the question about
/// quitting, and an update found but not answered. The window draws both in the
/// studio's own language and answers through the commands below.
#[derive(Default)]
struct Prompts {
    update: std::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    quit_asked: std::sync::atomic::AtomicBool,
    quit_seen: std::sync::atomic::AtomicBool,
}

/// A portable copy is updated by hand: the window offers the download page instead.
fn update_offer(version: &str) -> serde_json::Value {
    serde_json::json!({ "version": version, "portable": is_portable(), "page": format!("{}/releases/latest", studio().repo_url()) })
}

/// Quit, asking first only when a song is being generated. The window asks;
/// a window that never showed the question (frozen, still loading) does not
/// keep the studio open when the person closes it again.
fn confirm_quit(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    use tauri::{Emitter, Manager};
    let running = songs_in_progress(Duration::from_secs(2));
    if running.as_ref().is_some_and(Vec::is_empty) {
        quit_now(app, Vec::new());
        return;
    }
    let prompts = app.state::<Prompts>();
    if prompts.quit_asked.load(Ordering::SeqCst) && !prompts.quit_seen.load(Ordering::SeqCst) {
        quit_now(app, running.unwrap_or_default());
        return;
    }
    prompts.quit_asked.store(true, Ordering::SeqCst);
    prompts.quit_seen.store(false, Ordering::SeqCst);
    if app.emit_to("main", "studio://quit-asked", ()).is_err() {
        quit_now(app, running.unwrap_or_default());
    }
}

/// A song the person agreed to stop is stopped before the studio goes, so the
/// next start does not make it again as one the studio was cut off on; the
/// day's statistics leave now rather than at the next start.
fn quit_now(app: &tauri::AppHandle, running: Vec<String>) {
    use tauri::Manager;
    // the person is done with the window; what follows can take seconds on a slow network
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let running = if running.is_empty() { songs_in_progress(Duration::from_secs(15)).unwrap_or_default() } else { running };
    for id in running {
        let _ = service_call("POST", &format!("/v1/music/jobs/{id}"), Duration::from_secs(5));
    }
    let _ = service_call("POST", "/v1/hub/flush", Duration::from_secs(10));
    app.exit(0);
}

#[tauri::command]
fn quit_prompt_shown(prompts: tauri::State<'_, Prompts>) {
    prompts.quit_seen.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
fn quit_prompt_cancelled(prompts: tauri::State<'_, Prompts>) {
    prompts.quit_asked.store(false, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
fn quit_studio(app: tauri::AppHandle) {
    std::thread::spawn(move || quit_now(&app, Vec::new()));
}

/// The update found at start, for a window that loaded after the event went out.
#[tauri::command]
fn pending_update(prompts: tauri::State<'_, Prompts>) -> Option<serde_json::Value> {
    prompts.update.lock().ok()?.as_ref().map(|update| update_offer(&update.version))
}

/// Downloads and runs the installer; the studio restarts into the new version.
/// An error comes back to the window, which shows it.
#[tauri::command]
async fn install_update(app: tauri::AppHandle, prompts: tauri::State<'_, Prompts>) -> Result<(), String> {
    let update = prompts.update.lock().map_err(|_| "the update state is poisoned".to_string())?.take().ok_or("no update is waiting")?;
    if let Err(error) = update.download_and_install(|_, _| {}, || {}).await {
        let message = error.to_string();
        if let Ok(mut slot) = prompts.update.lock() {
            *slot = Some(update);
        }
        return Err(message);
    }
    app.restart();
}

fn spawn_update_check(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    use tauri_plugin_updater::UpdaterExt;

    tauri::async_runtime::spawn(async move {
        // The installer is a child of this process, which sits in its own
        // kill-on-close job: without releasing it, the installer died with the
        // studio a moment after starting and the update never happened.
        let cleanup = app.clone();
        // Started from the studio, the installer did not find the previous
        // folder and installed a second copy into its default one. NSIS takes
        // the folder as `/D=`, which must be the last argument and unquoted.
        let install_directory = format!("/D={}", executable_directory().display());
        // The proxy chosen in Settings: GitHub, where updates come from, is
        // among the sites a proxy is set up for.
        let builder = match music_server::net::fixed() {
            music_server::net::Fixed::System => app.updater_builder(),
            music_server::net::Fixed::Direct => app.updater_builder().no_proxy(),
            music_server::net::Fixed::Through(proxy) => app.updater_builder().proxy(proxy),
        };
        let updater = match builder
            .installer_arg(install_directory)
            .on_before_exit(move || {
                cleanup.cleanup_before_exit();
                if !music_engine::process_group::release_children() {
                    eprintln!("[ERROR] could not release the update installer from the studio's job");
                }
            })
            .build()
        {
            Ok(updater) => updater,
            Err(_) => return,
        };
        let update = match updater.check().await {
            Ok(Some(update)) => update,
            _ => return,
        };
        let offer = update_offer(&update.version);
        if let Ok(mut slot) = app.state::<Prompts>().update.lock() {
            *slot = Some(update);
        }
        let _ = app.emit_to("main", "studio://update-available", offer);
    });
}

#[cfg(windows)]
fn hide_own_console_window() {
    unsafe extern "system" {
        fn GetConsoleWindow() -> isize;
        fn GetConsoleProcessList(processes: *mut u32, count: u32) -> u32;
        fn ShowWindow(window: isize, command: i32) -> i32;
    }

    unsafe {
        let mut processes = [0_u32; 4];
        if GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32) == 1 {
            let window = GetConsoleWindow();
            if window != 0 {
                ShowWindow(window, 0);
            }
        }
    }
}

/// Tauri's own WebView2 switches, followed by whatever the user set in the
/// variable WebView2 documents for exactly this.
#[cfg(windows)]
fn webview_browser_arguments() -> String {
    // The visualiser's own window hears the studio through an audio context
    // of its own, which nothing in that window has clicked on to start.
    let mut own = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required".to_owned();
    // "No proxy" for what the window loads itself; a proxy of the user's own
    // is given through Tauri's proxy_url where it takes the scheme.
    let proxy = music_server::saved_proxy();
    if proxy.mode == music_server::net::ProxyMode::Off {
        own.push_str(" --no-proxy-server");
    } else if let Some(url) = proxy.window_proxy().filter(|url| !matches!(url.scheme(), "http" | "socks5")) {
        own.push_str(&format!(" --proxy-server={}://{}:{}", url.scheme(), url.host_str().unwrap_or_default(), url.port_or_known_default().unwrap_or_default()));
    }
    match std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        Ok(extra) if !extra.trim().is_empty() => format!("{own} {}", extra.trim()),
        _ => own,
    }
}

/// The proxy a window is given through Tauri's proxy_url, which takes HTTP and
/// SOCKS5; the other schemes go in the browser arguments. Every window takes
/// the same, or WebView2 refuses the second one.
fn window_proxy_url() -> Option<reqwest::Url> {
    music_server::saved_proxy().window_proxy().filter(|url| matches!(url.scheme(), "http" | "socks5"))
}

/// The visualiser in a window of its own, or the one already open brought
/// forward. Built here rather than from the page: every window of the app
/// shares one WebView2 environment, which must be opened with the same
/// switches the main window was, or the new window fails to open. Async:
/// on Windows a window made from a synchronous command deadlocks WebView2.
#[tauri::command]
async fn open_visualizer_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager as _;
    if let Some(window) = app.get_webview_window("visualizer") {
        window.show().map_err(|error| error.to_string())?;
        return window.set_focus().map_err(|error| error.to_string());
    }
    let title = format!("{} - Visualizer", app.package_info().name);
    let window = tauri::WebviewWindowBuilder::new(&app, "visualizer", tauri::WebviewUrl::App("visualizer.html".into()))
        .title(title)
        .inner_size(960.0, 540.0)
        .min_inner_size(320.0, 200.0);
    #[cfg(windows)]
    let window = window.additional_browser_args(&webview_browser_arguments());
    let window = match window_proxy_url() {
        Some(url) => window.proxy_url(url),
        None => window,
    };
    window.build().map(|_| ()).map_err(|error| error.to_string())
}

/// The window's shape: only these rectangles, [left, top, right, bottom] in
/// physical pixels of the page, are the window, and a click anywhere else
/// goes to whatever lies below. No rectangles: the whole window again. The
/// Winamp mode spreads the window over the screen and cuts it to Winamp's
/// own windows, so they can be moved apart, docked and resized like Winamp's.
#[tauri::command]
fn set_window_region(window: tauri::WebviewWindow, rects: Vec<[i32; 4]>) -> Result<(), String> {
    #[cfg(windows)]
    {
        #[link(name = "gdi32")]
        unsafe extern "system" {
            fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> isize;
            fn CombineRgn(destination: isize, first: isize, second: isize, mode: i32) -> i32;
            fn DeleteObject(object: isize) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SetWindowRgn(window: isize, region: isize, redraw: i32) -> i32;
            fn GetWindowRect(window: isize, rect: *mut [i32; 4]) -> i32;
            fn ClientToScreen(window: isize, point: *mut [i32; 2]) -> i32;
        }
        const RGN_OR: i32 = 2;
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
        unsafe {
            let region = if rects.is_empty() {
                0
            } else {
                // a region counts from the window's outer corner, which a frameless window
                // still keeps a few invisible border pixels away from its page
                let mut outer = [0i32; 4];
                let mut page = [0i32; 2];
                if GetWindowRect(hwnd, &mut outer) == 0 || ClientToScreen(hwnd, &mut page) == 0 {
                    return Err("Windows did not tell where the window's page is".into());
                }
                let (dx, dy) = (page[0] - outer[0], page[1] - outer[1]);
                let region = CreateRectRgn(0, 0, 0, 0);
                if region == 0 {
                    return Err("Windows made no region for the window".into());
                }
                // Chromium apps take a window whose region is one plain rectangle for an opaque
                // one of its whole size (IsWindowVisibleAndFullyOpaque) and stop drawing beneath
                // it: two lone pixels in the invisible border keep the shape never that plain
                let stubs = [[-dx, -dy, 1 - dx, 1 - dy], [2 - dx, -dy, 3 - dx, 1 - dy]];
                for [left, top, right, bottom] in stubs.into_iter().chain(rects) {
                    let part = CreateRectRgn(left + dx, top + dy, right + dx, bottom + dy);
                    if part == 0 || CombineRgn(region, region, part, RGN_OR) == 0 {
                        DeleteObject(region);
                        if part != 0 {
                            DeleteObject(part);
                        }
                        return Err("Windows could not build the window's shape".into());
                    }
                    DeleteObject(part);
                }
                region
            };
            // the window owns the region from here on
            if SetWindowRgn(hwnd, region, 1) == 0 {
                if region != 0 {
                    DeleteObject(region);
                }
                return Err("Windows did not take the window's shape".into());
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (window, rects);
    Ok(())
}

pub fn run() {
    #[cfg(windows)]
    hide_own_console_window();

    if keeps_everything_beside_itself() && std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none() {
        // The process is still single-threaded here, before Tauri or its
        // worker threads are created, so updating its child WebView environment
        // cannot race with an environment read.
        unsafe {
            std::env::set_var(
                "WEBVIEW2_USER_DATA_FOLDER",
                executable_directory().join("webview-data"),
            );
        }
    }

    // The native engine is started only by the setup gate after a complete
    // verified profile is present. This keeps first launch download-free and
    // avoids starting a runtime against partial weights.
    // Whatever ends this process - the window, Task Manager, a crash - takes
    // the engine with it. Without this the engine outlived a force-killed
    // studio, holding the graphics card and the port it listens on.
    if !music_engine::process_group::bind_children_to_this_process() {
        eprintln!("this process could not create its own job object; the engine is stopped by the supervisor only");
    }

    configure_studio_runtime_paths();
    // the card chosen in the engine settings, for every CUDA process after this
    music_server::apply_saved_gpu();

    // the service names the studio's version to agents, which only the shell knows
    let context = tauri::generate_context!();
    music_server::set_studio_version(context.package_info().version.to_string());

    if let Err(error) = start_service() {
        eprintln!("failed to start the studio service: {error}");
    }

    // The updater is configured only in release builds, where the signing
    // public key and the release endpoint are injected into the config. The
    // plugin refuses to initialise without that section and would take the
    // whole window down with it, so a development build simply runs without an
    // updater instead of crashing at launch.
    let updater_configured = context
        .config()
        .plugins
        .0
        .get("updater")
        .is_some_and(|value| !value.is_null());

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::SIZE | tauri_plugin_window_state::StateFlags::POSITION | tauri_plugin_window_state::StateFlags::MAXIMIZED)
                // an absolute name: the plugin joins it to AppData, which a portable copy never writes
                .with_filename(studio_data_directory().join("window-state.json").to_string_lossy())
                .with_filter(|label| label == "main")
                .build(),
        )
        .manage(Prompts::default())
        .invoke_handler(tauri::generate_handler![
            open_visualizer_window,
            set_window_region,
            quit_prompt_shown,
            quit_prompt_cancelled,
            quit_studio,
            pending_update,
            install_update
        ]);
    if updater_configured {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .setup(move |app| {
            // the window's own files, for a browser on another computer when
            // access from the network is on
            let resolver = app.asset_resolver();
            music_server::set_asset_source(Box::new(move |path: &str| resolver.get(path.to_string()).map(|asset| (asset.bytes.to_vec(), asset.mime_type))));
            // "Save as" for what the window saves: Windows' own dialog, over
            // the studio's window, asked for by the service off the UI thread.
            let dialogs = app.handle().clone();
            music_server::set_save_dialog(Box::new(move |name: &str, start: Option<std::path::PathBuf>| {
                use tauri::Manager;
                use tauri_plugin_dialog::DialogExt;
                let mut dialog = dialogs.dialog().file().set_file_name(name);
                if let Some(extension) = std::path::Path::new(name).extension().and_then(|extension| extension.to_str()) {
                    dialog = dialog.add_filter(extension.to_uppercase(), &[extension]);
                }
                if let Some(folder) = start {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(window) = dialogs.get_webview_window("main") {
                    dialog = dialog.set_parent(&window);
                }
                dialog.blocking_save_file().and_then(|path| path.into_path().ok())
            }));
            // The window is built here rather than from the configuration so
            // the WebView2 arguments can be extended: Tauri passes its own, and
            // WebView2 then ignores WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
            // entirely, so a remote-debugging port or any other documented
            // switch set by the user would silently never arrive.
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .cloned()
                .expect("the main window is declared in tauri.conf.json");
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?;
            #[cfg(windows)]
            let window = window.additional_browser_args(&webview_browser_arguments());
            // The proxy chosen in Settings for what the window loads itself -
            // the image and video searches of the video maker; the service
            // routes its own requests.
            let window = match window_proxy_url() {
                Some(url) => window.proxy_url(url),
                None => window,
            };

            window.build()?;
            // The stock Quit item terminates the app, closing the window
            // before anything can ask. Cmd+Q is a menu item of our own that
            // asks first; the window stays up until the answer.
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{Menu, MenuItem, MenuItemKind};
                let menu = Menu::default(app.handle())?;
                for item in menu.items()? {
                    if let MenuItemKind::Submenu(submenu) = item {
                        for entry in submenu.items()? {
                            if let MenuItemKind::Predefined(predefined) = &entry {
                                if predefined.text()?.starts_with("Quit") {
                                    submenu.remove(predefined)?;
                                    submenu.append(&MenuItem::with_id(app.handle(), "quit-confirm", "Quit ACE-Step Studio", true, Some("CmdOrCtrl+Q"))?)?;
                                }
                            }
                        }
                    }
                }
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    if event.id().as_ref() != "quit-confirm" {
                        return;
                    }
                    let app = app.clone();
                    std::thread::spawn(move || confirm_quit(&app));
                });
            }
            if updater_configured {
                spawn_update_check(app.handle().clone());
            }
            // The webview starts loading the moment the window exists, and the
            // service can finish binding a fraction of a second later. That is
            // enough for the browser to keep its own connection error on screen
            // for good - checking readiness once here is not enough, because the
            // race is already lost by then. So the page is reloaded whenever it
            // is still empty, for the first half minute of the session.
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    use tauri::Manager as _;
                    if !wait_until_ready(Duration::from_secs(90)) {
                        return;
                    }
                    let started = Instant::now();
                    while started.elapsed() < Duration::from_secs(30) {
                        std::thread::sleep(Duration::from_millis(700));
                        for (_, window) in handle.webview_windows() {
                            // Reload only while nothing has rendered: reloading
                            // a working studio would throw away what the user
                            // already has on screen.
                            let _ = window.eval(
                                "if (!document.getElementById('root') || !document.getElementById('root').firstElementChild) { window.location.reload(); }",
                            );
                        }
                    }
                });
            }
            Ok(())
        })
        .build(context)
        .expect("error while building ACE-Step Studio")
        .run(|app, event| {
            // Quitting stops a song that is being generated, so ask first, but
            // only then. Closing the main window quits the studio on every
            // platform; Cmd+Q and the Dock reach it as an exit request with no
            // code. An exit the app requests itself carries a code and goes
            // through.
            match &event {
                // the service is asked off the event loop: it can take seconds to answer
                tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. } if label == "main" => {
                    api.prevent_close();
                    let app = app.clone();
                    std::thread::spawn(move || confirm_quit(&app));
                }
                #[cfg(target_os = "macos")]
                tauri::RunEvent::ExitRequested { code: None, api, .. } => {
                    api.prevent_exit();
                    let app = app.clone();
                    std::thread::spawn(move || confirm_quit(&app));
                }
                _ => {}
            }
        });
}

