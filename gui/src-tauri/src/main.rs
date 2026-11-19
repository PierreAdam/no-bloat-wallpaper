// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use no_bloat_wallpaper::config::{self, Config};
use no_bloat_wallpaper::engine::{self, Current};
use no_bloat_wallpaper::{cache, error, platform, task};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Id of the optional tray icon. While it exists, closing the windows keeps the app running.
const TRAY_ID: &str = "nbw-tray";
/// Argument the login startup entry passes: start with the tray icon only, no window.
const TRAY_ARG: &str = "--tray";
/// Size of the panel that opens next to the tray icon, in logical pixels.
const FLYOUT_W: f64 = 360.0;
const FLYOUT_H: f64 = 500.0;

/// When the flyout last closed. A click on the tray icon first takes the focus away from the
/// open flyout (which closes it), then arrives as a click: that click must not reopen it.
struct FlyoutClosedAt(Mutex<Option<Instant>>);

/// What a failed command sends to the UI: a translation key when the error is a known one,
/// and the English message otherwise.
#[derive(serde::Serialize)]
struct CmdError {
    code: Option<&'static str>,
    message: String,
}

impl From<anyhow::Error> for CmdError {
    fn from(e: anyhow::Error) -> Self {
        CmdError { code: error::code_of(&e), message: format!("{e:#}") }
    }
}

type CmdResult<T> = Result<T, CmdError>;

/// Run blocking engine work (network, disk) off the UI thread.
async fn blocking<T, F>(f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CmdError { code: None, message: e.to_string() })?
        .map_err(CmdError::from)
}

/// The page may only display files from our data folder: previews, and the copies sized for
/// the screen that are made there from local pictures, downloads and favorites. Nothing else
/// on disk, whatever the page asks for.
fn allow_assets(app: &AppHandle) {
    if let Ok(dir) = config::data_dir() {
        let _ = app.asset_protocol_scope().allow_directory(dir, true);
    }
}

/// A picture as the interface shows it: its details, and a copy sized for the screen.
#[derive(serde::Serialize)]
struct Shown {
    #[serde(flatten)]
    cur: Current,
    /// The copy to display (in the data folder), or None when it could not be made.
    preview: Option<PathBuf>,
}

fn shown(cur: Current, size: u32) -> Shown {
    let preview = engine::preview(&cur.file, size);
    Shown { cur, preview }
}

#[tauri::command]
async fn get_config() -> CmdResult<Config> {
    blocking(config::load).await
}

#[tauri::command]
async fn save_config(app: AppHandle, cfg: Config) -> CmdResult<()> {
    let saved = cfg.clone();
    blocking(move || config::save(&saved)).await?;
    sync_tray(&app, &cfg).map_err(CmdError::from)
}

/// Replace an unusable config file with the defaults (the old one is kept as a `.bak`).
#[tauri::command]
async fn reset_config() -> CmdResult<String> {
    blocking(|| Ok(config::reset()?.display().to_string())).await
}

/// Open the main window (from the flyout's settings button).
///
/// Must stay `async`: a synchronous command runs on the main thread while the calling web view
/// waits, and creating a window there deadlocks on Windows (the app stops responding).
#[tauri::command]
async fn open_main_window(app: AppHandle) {
    open_main(&app);
}

#[tauri::command]
async fn get_status() -> CmdResult<engine::Status> {
    blocking(engine::status).await
}

#[tauri::command]
async fn list_offers(force: Option<bool>) -> CmdResult<Vec<engine::SourceOffers>> {
    blocking(move || engine::offers(force.unwrap_or(false))).await
}

#[tauri::command]
async fn cache_size() -> CmdResult<u64> {
    blocking(|| Ok(engine::cache_size())).await
}

#[tauri::command]
async fn clear_cache() -> CmdResult<()> {
    blocking(engine::clear_cache).await
}

#[tauri::command]
async fn apply_offer(source_id: String, offer_id: String) -> CmdResult<Current> {
    blocking(move || engine::apply_offer(&source_id, &offer_id)).await
}

#[tauri::command]
async fn get_current() -> CmdResult<Option<Shown>> {
    blocking(|| Ok(engine::current().map(|c| shown(c, cache::DISPLAY_SIZE)))).await
}

/// Result of "Change now": the wallpaper changed, or it was already the newest image.
#[derive(serde::Serialize)]
struct RunResult {
    changed: bool,
    reason: Option<String>,
    current: Option<Current>,
}

#[tauri::command]
async fn run_now() -> CmdResult<RunResult> {
    blocking(|| {
        Ok(match engine::refresh_now()? {
            engine::Outcome::Changed(c) => RunResult { changed: true, reason: None, current: Some(c) },
            engine::Outcome::Skipped(why) => RunResult { changed: false, reason: Some(why), current: None },
        })
    })
    .await
}

#[tauri::command]
async fn favorite_current() -> CmdResult<Current> {
    blocking(engine::favorite_current).await
}

#[tauri::command]
async fn list_favorites() -> CmdResult<Vec<Shown>> {
    blocking(|| Ok(engine::list_favorites()?.into_iter().map(|f| shown(f, cache::THUMB_SIZE)).collect())).await
}

#[tauri::command]
async fn apply_favorite(id: String) -> CmdResult<Current> {
    blocking(move || engine::apply_favorite(&id)).await
}

#[tauri::command]
async fn delete_favorite(id: String) -> CmdResult<()> {
    blocking(move || engine::delete_favorite(&id)).await
}

#[tauri::command]
async fn install_schedule() -> CmdResult<u32> {
    blocking(engine::apply_schedule).await
}

#[tauri::command]
async fn remove_schedule() -> CmdResult<()> {
    blocking(platform::remove_schedule).await
}

// ---- Windows and tray icon ----

/// Show the main window, creating it when needed.
fn open_main(app: &AppHandle) {
    if let Some(f) = app.get_webview_window("flyout") {
        let _ = f.destroy();
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("No Bloat Wallpaper")
        .decorations(false)
        .inner_size(460.0, 720.0)
        .min_inner_size(400.0, 560.0)
        .build();
}

/// Open the flyout next to the tray icon, or close it when it is open. It is destroyed (not
/// hidden) when it closes, so the web view's memory is given back while the tray idles.
fn toggle_flyout(app: &AppHandle, icon: tauri::Rect) {
    if let Some(w) = app.get_webview_window("flyout") {
        let _ = w.destroy();
        return;
    }
    let just_closed =
        app.state::<FlyoutClosedAt>().0.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if just_closed {
        return;
    }
    let Ok(win) = WebviewWindowBuilder::new(app, "flyout", WebviewUrl::App("index.html#flyout".into()))
        .title("No Bloat Wallpaper")
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .inner_size(FLYOUT_W, FLYOUT_H)
        .build()
    else {
        return;
    };
    if let Some(pos) = flyout_position(app, icon) {
        let _ = win.set_position(pos);
    }
    let _ = win.show();
    let _ = win.set_focus();
}

/// Place the flyout against the taskbar, next to the icon, whichever edge the taskbar is on.
fn flyout_position(app: &AppHandle, icon: tauri::Rect) -> Option<PhysicalPosition<i32>> {
    let p = icon.position.to_physical::<f64>(1.0);
    let s = icon.size.to_physical::<f64>(1.0);
    let (cx, cy) = (p.x + s.width / 2.0, p.y + s.height / 2.0);
    let monitor = app.monitor_from_point(cx, cy).ok().flatten().or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let wa = monitor.work_area();
    let (l, t) = (wa.position.x, wa.position.y);
    let (r, b) = (l + wa.size.width as i32, t + wa.size.height as i32);
    let (w, h) = ((FLYOUT_W * scale) as i32, (FLYOUT_H * scale) as i32);
    let gap = (12.0 * scale) as i32;
    let (cx, cy) = (cx as i32, cy as i32);
    let (x, y) = if cy >= b {
        (cx - w / 2, b - h - gap) // taskbar at the bottom
    } else if cy <= t {
        (cx - w / 2, t + gap) // at the top
    } else if cx <= l {
        (l + gap, cy - h / 2) // on the left
    } else if cx >= r {
        (r - w - gap, cy - h / 2) // on the right
    } else {
        (r - w - gap, b - h - gap) // the icon sits in the hidden-icons popup
    };
    Some(PhysicalPosition::new(
        x.clamp(l + gap, (r - w - gap).max(l + gap)),
        y.clamp(t + gap, (b - h - gap).max(t + gap)),
    ))
}

/// The tray menu is drawn by Windows, outside the page, so its two labels are translated here.
fn tray_labels(cfg: &Config) -> (&'static str, &'static str) {
    let lang =
        if cfg.language.is_empty() || cfg.language == "auto" { platform::ui_language() } else { cfg.language.as_str() };
    match lang {
        "fr" => ("Ouvrir No Bloat Wallpaper", "Quitter"),
        "de" => ("No Bloat Wallpaper öffnen", "Beenden"),
        "es" => ("Abrir No Bloat Wallpaper", "Salir"),
        "it" => ("Apri No Bloat Wallpaper", "Esci"),
        "cs" => ("Otevřít No Bloat Wallpaper", "Ukončit"),
        "pt" => ("Abrir No Bloat Wallpaper", "Sair"),
        "nl" => ("No Bloat Wallpaper openen", "Afsluiten"),
        "ja" => ("No Bloat Wallpaper を開く", "終了"),
        _ => ("Open No Bloat Wallpaper", "Quit"),
    }
}

fn tray_menu(app: &AppHandle, cfg: &Config) -> tauri::Result<Menu<tauri::Wry>> {
    let (open, quit) = tray_labels(cfg);
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", open, true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", quit, true, None::<&str>)?,
        ],
    )
}

/// What the login startup entry runs.
fn autostart_command() -> anyhow::Result<String> {
    Ok(format!("\"{}\" {TRAY_ARG}", std::env::current_exe()?.display()))
}

/// Make the tray icon (and its login startup entry) match the settings.
fn sync_tray(app: &AppHandle, cfg: &Config) -> anyhow::Result<()> {
    match (cfg.tray, app.tray_by_id(TRAY_ID)) {
        (true, Some(tray)) => tray.set_menu(Some(tray_menu(app, cfg)?))?, // the language may have changed
        (true, None) => {
            create_tray(app, cfg)?;
            platform::set_login_autostart(Some(&autostart_command()?))?;
        }
        (false, Some(_)) => {
            app.remove_tray_by_id(TRAY_ID);
            if let Some(f) = app.get_webview_window("flyout") {
                let _ = f.destroy();
            }
            platform::set_login_autostart(None)?;
        }
        (false, None) => {}
    }
    Ok(())
}

fn create_tray(app: &AppHandle, cfg: &Config) -> tauri::Result<()> {
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("No Bloat Wallpaper")
        .menu(&tray_menu(app, cfg)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "open" => open_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left, button_state: MouseButtonState::Up, rect, ..
            } = ev
            {
                toggle_flyout(tray.app_handle(), rect);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    // A scheduled run: change the wallpaper and exit, before any window or web view exists.
    if std::env::args().any(|a| a == task::TASK_ARG) {
        task::run(std::env::args().any(|a| a == task::IF_DUE_ARG));
        return;
    }

    let tray_start = std::env::args().any(|a| a == TRAY_ARG);
    // An unreadable config must not look like "tray off": started at login, keep the icon so
    // the settings (which offer to repair the file) stay one click away.
    let loaded = config::load();
    let tray_on = loaded.as_ref().map(|c| c.tray).unwrap_or(tray_start);
    let cfg = loaded.unwrap_or_default();
    // Started at login, but the tray icon was turned off since: clean up and leave.
    if tray_start && !tray_on {
        let _ = platform::set_login_autostart(None);
        return;
    }

    let app = tauri::Builder::default()
        // One process at a time: opening the app again shows the window of the running one.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == TRAY_ARG) {
                open_main(app);
            }
        }))
        .manage(FlyoutClosedAt(Mutex::new(None)))
        .setup(move |app| {
            let handle = app.handle();
            allow_assets(handle);
            if tray_on {
                create_tray(handle, &cfg)?;
                // Keeps the entry pointing at this program, also after it moved.
                let _ = autostart_command().and_then(|c| platform::set_login_autostart(Some(&c)));
            }
            if !tray_start {
                open_main(handle);
            }
            Ok(())
        })
        .on_window_event(|window, ev| {
            // Close the flyout once it really lost the focus. When it opens, the focus moves from the
            // window into its web view, which reports "lost" then "gained" within a millisecond:
            // wait a moment and look again instead of closing on that first event.
            if window.label() == "flyout" && matches!(ev, WindowEvent::Focused(false)) {
                let window = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(150));
                    if !window.is_focused().unwrap_or(false) {
                        *window.state::<FlyoutClosedAt>().0.lock().unwrap() = Some(Instant::now());
                        let _ = window.destroy();
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            reset_config,
            open_main_window,
            get_status,
            list_offers,
            cache_size,
            clear_cache,
            apply_offer,
            get_current,
            run_now,
            favorite_current,
            list_favorites,
            apply_favorite,
            delete_favorite,
            install_schedule,
            remove_schedule
        ])
        .build(tauri::generate_context!())
        .expect("error while starting the GUI");

    app.run(|app, ev| {
        // Closing the last window keeps the app running while the tray icon is on ("Quit" exits).
        if let RunEvent::ExitRequested { code: None, api, .. } = ev {
            if app.tray_by_id(TRAY_ID).is_some() {
                api.prevent_exit();
            }
        }
    });
}
