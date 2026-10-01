mod connect;
mod diagnostics;
mod models;
mod subsonic;

use connect::SplyntConnectState;
use std::sync::atomic::{AtomicBool, Ordering};
use subsonic::{DownloadState, PlaybackPrefsState, SessionState};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    Emitter, Manager, WindowEvent,
};

/// Set from the frontend so closing the window can follow the user's
/// background-playback preference instead of guessing.
#[derive(Default)]
pub(crate) struct BackgroundPlayback(AtomicBool);

/// Webview diagnostics, into the same file as everything the host records.
#[tauri::command]
fn log_message(level: String, message: String) {
    let level = match level.as_str() {
        "warn" | "error" => level.as_str(),
        _ => "info",
    };
    let mut detail = serde_json::Map::new();
    detail.insert("level".into(), serde_json::Value::from(level));
    detail.insert("message".into(), serde_json::Value::from(message));
    diagnostics::log_detail("webview", detail);
}

/// A structured event from the webview, keeping its fields as fields.
///
/// `log_message` flattens everything into one string, which is right for a
/// console warning and wrong for a measurement — Splynt Connect's drift
/// numbers are only useful if they can be read back as numbers.
#[tauri::command]
fn log_event(event: String, detail: std::collections::HashMap<String, serde_json::Value>) {
    let mut map = serde_json::Map::new();
    for (key, value) in detail {
        map.insert(key, value);
    }
    diagnostics::log_detail(&event, map);
}

/// Where the log lives, so Settings can point at it and the user can send it on.
#[tauri::command]
fn diagnostics_path() -> Option<String> {
    diagnostics::log_path().map(|path| path.to_string_lossy().to_string())
}

/// Shows the log in the platform's file manager.
#[tauri::command]
fn reveal_diagnostics() -> Result<(), String> {
    let Some(path) = diagnostics::log_path() else {
        return Err("No diagnostics log has been created yet.".to_string());
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg("-R").arg(&path);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer");
        command.arg(format!("/select,{}", path.display()));
        command
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(&path));
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("The log could not be shown: {error}"))
}

/// Opens the support page in the user's real browser.
///
/// Deliberately not a plugin. `tauri-plugin-opener` would do this too, but it
/// means a Cargo dependency, an npm dependency, plugin registration and a new
/// capability permission, for one link. This mirrors `reveal_diagnostics`
/// above, which already shells out to the same three platform openers.
///
/// The URL is a hardcoded constant rather than a parameter. A command that
/// takes an arbitrary URL from the webview and hands it to the shell is a
/// launch-anything primitive; there is exactly one page this ever needs to
/// open, so it takes no argument at all.
#[tauri::command]
fn open_support_page() -> Result<(), String> {
    const SUPPORT_URL: &str = "https://buymeacoffee.com/elijahgronda";
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(SUPPORT_URL);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        // The empty string is the window title `start` expects before the URL;
        // without it a quoted argument is taken as the title and nothing opens.
        command.args(["/C", "start", "", SUPPORT_URL]);
        command
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(SUPPORT_URL);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("The page could not be opened: {error}"))
}

#[tauri::command]
fn set_background_playback(enabled: bool, state: tauri::State<'_, BackgroundPlayback>) {
    state.0.store(enabled, Ordering::Relaxed);
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Menu items the webview acts on. Anything the shell already owns — transport,
/// search, preferences — is forwarded rather than reimplemented natively.
fn build_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let about = Submenu::with_items(
        app,
        "Splynt",
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "preferences",
                "Preferences…",
                true,
                Some("CmdOrCtrl+,"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "search", "Search", true, Some("CmdOrCtrl+K"))?,
        ],
    )?;
    let playback = Submenu::with_items(
        app,
        "Playback",
        true,
        &[
            &MenuItem::with_id(app, "playpause", "Play or Pause", true, None::<&str>)?,
            &MenuItem::with_id(app, "next", "Next Track", true, None::<&str>)?,
            &MenuItem::with_id(app, "previous", "Previous Track", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "shuffle", "Toggle Shuffle", true, None::<&str>)?,
            &MenuItem::with_id(app, "repeat", "Cycle Repeat", true, None::<&str>)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &MenuItem::with_id(app, "queue", "Queue", true, Some("CmdOrCtrl+Shift+Q"))?,
            &MenuItem::with_id(app, "lyrics", "Lyrics", true, None::<&str>)?,
            &MenuItem::with_id(app, "devices", "Splynt Connect Devices", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "fullplayer", "Full Player", true, None::<&str>)?,
        ],
    )?;
    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[&MenuItem::with_id(
            app,
            "shortcuts",
            "Keyboard Shortcuts",
            true,
            None::<&str>,
        )?],
    )?;
    Menu::with_items(app, &[&about, &edit, &playback, &view, &help])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // A second launch focuses the running player instead of starting a
            // rival instance against the same downloads and Connect identity.
            show_main_window(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(BackgroundPlayback::default())
        .manage(SessionState::default())
        .manage(DownloadState::default())
        .manage(PlaybackPrefsState::default())
        .manage(SplyntConnectState::default())
        .setup(|app| {
            // First, so everything below is recorded — including a failure here.
            diagnostics::start(&app.handle().clone());
            let proxy =
                tauri::async_runtime::block_on(subsonic::start_media_proxy(app.handle().clone()))
                    .map_err(std::io::Error::other)?;
            app.manage(proxy);

            if let Ok(data_dir) = app.path().app_data_dir() {
                connect::set_device_id_path(data_dir.join("connect-device-id"));
            }
            connect::set_app_handle(app.handle().clone());

            let handle = app.handle().clone();
            app.set_menu(build_menu(&handle)?)?;
            app.on_menu_event(|app, event| {
                if event.id() == "preferences" {
                    let _ = app.emit("menu-command", "preferences");
                } else {
                    let _ = app.emit("menu-command", event.id().0.as_str());
                }
            });

            let show = MenuItem::with_id(app, "tray-show", "Show Splynt", true, None::<&str>)?;
            let quit = PredefinedMenuItem::quit(app, Some("Quit Splynt"))?;
            let tray_menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().ok_or_else(|| {
                    std::io::Error::other("The tray icon is missing from the bundle.")
                })?)
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    if event.id() == "tray-show" {
                        show_main_window(app);
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if !matches!(event, WindowEvent::CloseRequested { .. }) {
                return;
            }
            let keep_playing = window
                .app_handle()
                .state::<BackgroundPlayback>()
                .0
                .load(Ordering::Relaxed);
            if !keep_playing {
                return;
            }
            // Hiding rather than closing keeps audio alive; Quit still stops it.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_background_playback,
            log_message,
            log_event,
            diagnostics_path,
            reveal_diagnostics,
            open_support_page,
            subsonic::connect_server,
            subsonic::restore_session,
            subsonic::list_profiles,
            subsonic::connect_profile,
            subsonic::forget_profile,
            subsonic::disconnect_server,
            subsonic::load_home,
            subsonic::load_library,
            subsonic::search_catalog,
            subsonic::get_album,
            subsonic::get_playlist,
            subsonic::get_artist,
            subsonic::get_songs_by_ids,
            subsonic::set_starred,
            subsonic::create_playlist,
            subsonic::save_play_queue,
            subsonic::get_play_queue,
            subsonic::get_lyrics,
            subsonic::get_radio,
            subsonic::scrobble,
            subsonic::add_song_to_playlist,
            subsonic::add_songs_to_playlist,
            subsonic::remove_song_from_playlist,
            subsonic::rename_playlist,
            subsonic::delete_playlist,
            subsonic::set_playlist_songs,
            subsonic::list_downloads,
            subsonic::download_song,
            subsonic::pause_download,
            subsonic::remove_download,
            subsonic::clear_downloads,
            subsonic::cache_stats,
            subsonic::clear_artwork_cache,
            subsonic::media_url,
            subsonic::set_playback_prefs,
            connect::publish_connect_playback,
            connect::connect_snapshot,
            connect::send_connect_command
        ])
        .build(tauri::generate_context!())
        .expect("error while running Splynt")
        // `_app` rather than `app`: its only use is inside the macOS-only Reopen
        // arm below, so on Linux and Windows that arm is compiled out and the
        // binding is genuinely unused. `-D warnings` in CI turns that into a hard
        // error on those two platforms only. The underscore silences it without
        // suppressing anything else, and the binding is still usable by name
        // where the arm does exist.
        .run(|_app, event| match event {
            tauri::RunEvent::ExitRequested { .. } => diagnostics::mark_clean_exit(),
            // Clicking the Dock icon, or opening the app while it is already
            // running, has to bring the window back. Without this the window
            // could be closed and never recovered: macOS `open` on a running
            // app activates it rather than starting a second process, so the
            // single-instance callback never fires either, and the only way
            // back was the tray.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                show_main_window(_app);
            }
            _ => {}
        });
}
