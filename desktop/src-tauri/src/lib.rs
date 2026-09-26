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
    let beside_the_executable = executable_directory().join("data");
    if directory_is_writable(&beside_the_executable) {
        return beside_the_executable;
    }

    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")) {
            return PathBuf::from(root).join(&studio().name);
        }
    }

    #[cfg(not(windows))]
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

fn spawn_update_check(app: tauri::AppHandle, portable: bool) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
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
        let updater = match app
            .updater_builder()
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

        let version = update.version.clone();
        if portable {
            let open_release = app
                .dialog()
                .message(format!("{} {version} is available. Open the download page?", studio().name))
                .title(format!("{} update", studio().name))
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancelCustom("Open".into(), "Later".into()))
                .blocking_show();
            if open_release {
                use tauri_plugin_opener::OpenerExt;
                let _ = app.opener().open_url(format!("{}/releases/latest", studio().repo_url()), None::<&str>);
            }
            return;
        }

        let install = app
            .dialog()
            .message(format!("{} {version} is available. Install it now?", studio().name))
            .title(format!("{} update", studio().name))
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom("Install".into(), "Later".into()))
            .blocking_show();
        if !install {
            return;
        }

        if let Err(error) = update.download_and_install(|_, _| {}, || {}).await {
            app.dialog()
                .message(format!("Could not install the update: {error}"))
                .title(format!("{} update", studio().name))
                .kind(MessageDialogKind::Error)
                .blocking_show();
            return;
        }
        app.restart();
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
    let own = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";
    match std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        Ok(extra) if !extra.trim().is_empty() => format!("{own} {}", extra.trim()),
        _ => own.to_owned(),
    }
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
        .plugin(tauri_plugin_opener::init());
    if updater_configured {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .setup(move |app| {
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
            window.build()?;
            if updater_configured {
                spawn_update_check(app.handle().clone(), is_portable());
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
        .run(context)
        .expect("error while running the studio");
}

