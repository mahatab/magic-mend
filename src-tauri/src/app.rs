//! Tauri glue: tray, hotkey, popup, setup wizard, settings window, and the engine thread.

use crate::actions::{self, ReplaceOutcome};
use crate::channel;
use crate::checker;
use crate::claude_path;
use crate::drafts::Output;
use crate::engine::{ClaudeEngine, Engine, EngineConfig, EngineEvent, ErrorKind};
use crate::engine_manager::EngineManager;
use crate::hotkey;
use crate::logging;
use crate::platform::{imp, WindowInfo};
use crate::protocol::{parse_reply, Request};
use crate::selection::{self, Capture};
use crate::settings::{settings_path, Settings, SETUP_VERSION};
use crate::window_tracker;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const POPUP_WIDTH: f64 = 480.0;
pub const POPUP_MAX_HEIGHT: f64 = 640.0;
const LONG_TEXT: usize = 4000;
const ADMIN_MESSAGE: &str = "This app is running as administrator, so Magic Mend cannot read the selection or paste into it. Type or paste the text here, and Copy still works.";

pub struct Paths {
    pub config_dir: PathBuf,
    pub engine_dir: PathBuf,
    pub prompt_file: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct PopupSession {
    pub id: u64,
    pub target: Option<WindowInfo>,
    pub own_window: bool,
    pub target_focused: bool,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub paths: Paths,
    pub engine: Mutex<EngineManager>,
    pub session: Mutex<Option<PopupSession>>,
    busy: AtomicBool,
    session_ids: AtomicU64,
    registered: Mutex<Option<String>>,
    /// The wizard step on screen. The practice box takes the hotkey only on the Try it step.
    wizard_step: Mutex<String>,
}

impl AppState {
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
}

pub fn engine_config(s: &Settings, paths: &Paths) -> Option<EngineConfig> {
    Some(EngineConfig {
        claude_path: claude_path::resolve(s.claude_path.as_deref())?,
        model: s.model.clone(),
        system_prompt_file: paths.prompt_file.clone(),
        workdir: paths.engine_dir.clone(),
        priming: s.priming,
    })
}

/// Warm mode only starts once setup has passed, so nothing reaches Claude before then.
fn configure_engine(app: &AppHandle) {
    let st = app.state::<AppState>();
    let s = st.settings();
    let cfg = if s.required_done() { engine_config(&s, &st.paths) } else { None };
    st.engine.lock().unwrap().configure(cfg, s.warm_mode && s.required_done(), s.checker_rules());
}

fn emit_outputs(app: &AppHandle, outs: Vec<Output>) {
    for o in outs {
        let _ = app.emit_to("popup", "popup:output", &o);
    }
}

// ---------------------------------------------------------------------------------------------
// Hotkey

fn register_hotkey(app: &AppHandle, hk: &str) -> Result<(), String> {
    let parsed = hotkey::parse(hk).ok_or_else(|| format!("{hk} is not a valid hotkey."))?;
    let plugin_str = parsed.to_plugin_string();
    let st = app.state::<AppState>();
    let mut reg = st.registered.lock().unwrap();
    if reg.as_deref() == Some(plugin_str.as_str()) {
        return Ok(());
    }
    let gs = app.global_shortcut();
    if let Some(old) = reg.take() {
        let _ = gs.unregister(old.as_str());
    }
    match gs.register(plugin_str.as_str()) {
        Ok(()) => {
            *reg = Some(plugin_str);
            logging::event("hotkey_registered", &[]);
            Ok(())
        }
        Err(_) => Err(format!("Another app is already using {}. Pick a different hotkey in Settings.", parsed.display())),
    }
}

fn hotkey_pressed(app: &AppHandle) {
    let pressed_at = Instant::now();
    let st = app.state::<AppState>();
    if st.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    struct Reset<'a>(&'a AtomicBool);
    impl Drop for Reset<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let _reset = Reset(&st.busy);
    let s = st.settings();
    if s.paused {
        return;
    }
    // Never open a second popup.
    if let Some(p) = app.get_webview_window("popup") {
        if p.is_visible().unwrap_or(false) {
            let _ = p.set_focus();
            return;
        }
    }
    // Setup must pass before anything is captured or sent.
    if !s.required_done() {
        open_wizard(app, Some(s.first_unfinished_step()));
        return;
    }
    let fg = window_tracker::active();
    let mut own_window = false;
    if let Some(w) = &fg {
        if w.pid == imp::self_pid() {
            // Our own windows: only the practice box in the setup wizard takes the hotkey.
            let wizard_focused = app
                .get_webview_window("wizard")
                .map(|wz| wz.is_focused().unwrap_or(false) && wz.is_visible().unwrap_or(false))
                .unwrap_or(false);
            let on_try_step = st.wizard_step.lock().unwrap().as_str() == "try";
            if !wizard_focused || !on_try_step {
                return;
            }
            own_window = true;
        } else if channel::is_excluded(&s.excluded_apps, &w.process) {
            logging::event("hotkey_excluded", &[]);
            return;
        }
    }
    let vk = hotkey::parse(&s.hotkey).and_then(|h| h.virtual_key());
    let capture = match &fg {
        Some(w) if w.elevated => Capture::Unreadable { reason: ADMIN_MESSAGE.into() },
        _ => {
            let (c, t) = selection::capture(vk);
            logging::event(
                "capture",
                &[("release_ms", &t.release_ms.to_string()), ("copy_ms", &t.copy_ms.to_string()), ("total_ms", &t.total_ms.to_string())],
            );
            c
        }
    };
    let channel_name = match (&fg, own_window) {
        (Some(w), false) => channel::detect(&s.channel_rules, &w.process, &w.title),
        _ => "Text".to_string(),
    };
    let engine_error = st.engine.lock().unwrap().begin_session().err();
    let id = st.session_ids.fetch_add(1, Ordering::SeqCst) + 1;
    *st.session.lock().unwrap() = Some(PopupSession { id, target: fg.clone(), own_window, target_focused: false });
    let payload = popup_payload(id, &s, fg.as_ref(), own_window, capture, &channel_name, engine_error);
    show_popup(app, fg.as_ref().map(|w| w.handle), &s.popup_position, &payload);
    logging::timing("popup_visible", pressed_at.elapsed().as_millis());
}

#[derive(Serialize, Clone)]
struct TargetView {
    app_name: String,
    process: String,
    icon: Option<String>,
    elevated: bool,
    own_window: bool,
    present: bool,
}

#[derive(Serialize, Clone)]
struct PopupPayload {
    session_id: u64,
    capture: Capture,
    target: TargetView,
    channel: String,
    tone: String,
    length: String,
    toggles: Vec<String>,
    drafts: u8,
    long_text_limit: usize,
    theme: String,
    translucent: bool,
    engine_error: Option<String>,
    no_contractions: bool,
    no_em_dashes: bool,
}

fn popup_payload(
    id: u64,
    s: &Settings,
    fg: Option<&WindowInfo>,
    own_window: bool,
    capture: Capture,
    channel_name: &str,
    engine_error: Option<ErrorKind>,
) -> PopupPayload {
    let target = match fg {
        Some(w) => TargetView {
            app_name: if own_window { "Magic Mend practice box".into() } else { w.app_name.clone() },
            process: w.process.clone(),
            icon: if own_window { None } else { window_tracker::icon_data_url(&w.path) },
            elevated: w.elevated,
            own_window,
            present: true,
        },
        None => TargetView { app_name: String::new(), process: String::new(), icon: None, elevated: false, own_window, present: false },
    };
    PopupPayload {
        session_id: id,
        capture,
        target,
        channel: channel_name.to_string(),
        tone: s.default_tone.clone(),
        length: s.default_length.clone(),
        toggles: s.default_toggles.clone(),
        drafts: s.drafts,
        long_text_limit: LONG_TEXT,
        theme: s.theme.clone(),
        translucent: translucent_supported(),
        engine_error: engine_error.map(|k| k.message().to_string()),
        no_contractions: s.no_contractions,
        no_em_dashes: s.no_em_dashes,
    }
}

fn translucent_supported() -> bool {
    #[cfg(windows)]
    {
        imp::windows_build() >= 22000
    }
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

fn show_popup(app: &AppHandle, target: Option<isize>, position: &str, payload: &PopupPayload) {
    let Some(popup) = app.get_webview_window("popup") else { return };
    let _ = popup.emit("popup:open", payload);
    place_popup(&popup, target, position, 420.0);
    let _ = popup.show();
    let _ = popup.set_focus();
}

/// Centers the popup on the target window's monitor, or opens it near the cursor.
fn place_popup(popup: &WebviewWindow, target: Option<isize>, position: &str, height: f64) {
    let (mut l, mut t, mut r, mut b) = imp::work_area(target);
    if r <= l || b <= t {
        // Mac: use the monitor under the pointer, from Tauri.
        let cursor = popup.cursor_position().ok();
        let mon = cursor
            .and_then(|c| popup.monitor_from_point(c.x, c.y).ok().flatten())
            .or_else(|| popup.primary_monitor().ok().flatten());
        if let Some(m) = mon {
            let wa = m.work_area();
            (l, t) = (wa.position.x, wa.position.y);
            (r, b) = (l + wa.size.width as i32, t + wa.size.height as i32);
        } else {
            (l, t, r, b) = (0, 0, 1440, 900);
        }
    }
    let scale = popup
        .monitor_from_point(((l + r) / 2) as f64, ((t + b) / 2) as f64)
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let w = (POPUP_WIDTH * scale) as i32;
    let h = (height * scale) as i32;
    let _ = popup.set_size(tauri::LogicalSize::new(POPUP_WIDTH, height));
    let (mut x, mut y) = if position == "cursor" {
        let (cx, cy) = imp::cursor_pos();
        (cx - w / 2, cy + (16.0 * scale) as i32)
    } else {
        ((l + r - w) / 2, t + ((b - t) - (POPUP_MAX_HEIGHT * scale) as i32).max(0) / 2)
    };
    x = x.clamp(l, (r - w).max(l));
    y = y.clamp(t, (b - h).max(t));
    let _ = popup.set_position(PhysicalPosition::new(x, y));
}

// ---------------------------------------------------------------------------------------------
// Popup commands

#[tauri::command]
async fn request_drafts(app: AppHandle, req_id: u64, request: Request) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let outs = st.engine.lock().unwrap().request(req_id, request);
        emit_outputs(&app, outs);
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn retry_drafts(app: AppHandle, req_id: u64, request: Request) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        // The user may have fixed the path or signed in since: reload the engine settings first.
        let s = st.settings();
        let cfg = engine_config(&s, &st.paths);
        let mut eng = st.engine.lock().unwrap();
        if cfg.is_some() && !eng.has_config() {
            eng.configure(cfg, s.warm_mode, s.checker_rules());
        }
        let outs = eng.retry(req_id, request);
        drop(eng);
        emit_outputs(&app, outs);
    })
    .await
    .map_err(|e| e.to_string())
}

/// Never act on a draft that fails the output check.
fn passes_check(app: &AppHandle, text: &str) -> bool {
    let rules = app.state::<AppState>().settings().checker_rules();
    checker::check(text, rules).is_clean()
}

#[tauri::command]
async fn replace_draft(app: AppHandle, text: String) -> Result<ReplaceOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !passes_check(&app, &text) {
            return ReplaceOutcome::Failed { reason: "This draft did not pass the output check.".into() };
        }
        let st = app.state::<AppState>();
        let session = st.session.lock().unwrap().clone();
        let Some(target) = session.and_then(|s| s.target) else {
            actions::copy(&text);
            return ReplaceOutcome::WindowLost;
        };
        let popup = app.get_webview_window("popup");
        let hide = || {
            if let Some(p) = &popup {
                let _ = p.hide();
            }
        };
        let outcome = actions::replace(target.handle, target.elevated, &text, hide);
        logging::event("replace", &[("result", outcome_name(&outcome))]);
        if outcome == ReplaceOutcome::Pasted {
            end_session(&app);
        } else if let Some(p) = &popup {
            let _ = p.show();
            let _ = p.set_focus();
        }
        outcome
    })
    .await
    .map_err(|e| e.to_string())
}

fn outcome_name(o: &ReplaceOutcome) -> &'static str {
    match o {
        ReplaceOutcome::Pasted => "pasted",
        ReplaceOutcome::WindowLost => "window_lost",
        ReplaceOutcome::Elevated => "elevated",
        ReplaceOutcome::Failed { .. } => "failed",
    }
}

/// Copy leaves the text on the clipboard on purpose.
#[tauri::command]
async fn copy_text(app: AppHandle, text: String, check: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if check && !passes_check(&app, &text) {
            return false;
        }
        let ok = actions::copy(&text);
        logging::event("copy", &[("ok", if ok { "1" } else { "0" })]);
        ok
    })
    .await
    .map_err(|e| e.to_string())
}

fn end_session(app: &AppHandle) {
    if let Some(p) = app.get_webview_window("popup") {
        let _ = p.hide();
        let _ = p.emit("popup:closed", ());
    }
    let st = app.state::<AppState>();
    *st.session.lock().unwrap() = None;
    st.engine.lock().unwrap().end_session();
}

#[tauri::command]
async fn close_popup(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || end_session(&app)).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn resize_popup(window: WebviewWindow, height: f64) {
    let h = height.clamp(160.0, POPUP_MAX_HEIGHT);
    let Ok(size) = window.inner_size() else { return };
    let scale = window.scale_factor().unwrap_or(1.0);
    if ((size.height as f64 / scale) - h).abs() < 1.0 {
        return;
    }
    let _ = window.set_size(tauri::LogicalSize::new(POPUP_WIDTH, h));
    // Keep the bottom on screen when the popup grows.
    if let (Ok(pos), Ok(Some(mon))) = (window.outer_position(), window.current_monitor()) {
        let wa = mon.work_area();
        let bottom = wa.position.y + wa.size.height as i32;
        let new_h = (h * scale) as i32;
        if pos.y + new_h > bottom {
            let _ = window.set_position(PhysicalPosition::new(pos.x, (bottom - new_h).max(wa.position.y)));
        }
    }
}

/// The popup asks whether the original window still exists before offering Replace.
#[tauri::command]
fn target_status(app: AppHandle) -> serde_json::Value {
    let st = app.state::<AppState>();
    let s = st.session.lock().unwrap().clone();
    let alive = s.as_ref().and_then(|s| s.target.as_ref()).map(|t| imp::is_window_alive(t.handle)).unwrap_or(false);
    serde_json::json!({ "alive": alive, "focused": s.map(|s| s.target_focused).unwrap_or(false) })
}

fn popup_focus_lost(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        let st = app.state::<AppState>();
        let mut guard = st.session.lock().unwrap();
        let Some(s) = guard.as_mut() else { return };
        let Some(t) = &s.target else { return };
        let fg = imp::foreground_handle();
        if fg != 0 && imp::root_handle(fg) == imp::root_handle(t.handle) && !s.target_focused {
            s.target_focused = true;
            drop(guard);
            let _ = app.emit_to("popup", "popup:target-focused", ());
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Settings

#[derive(Serialize)]
struct AppInfo {
    settings: Settings,
    platform: &'static str,
    version: &'static str,
    hotkey_display: String,
    detected_claude: Option<String>,
    log_path: Option<String>,
    steps: Vec<&'static str>,
    required: Vec<&'static str>,
    first_unfinished: &'static str,
    translucent: bool,
}

fn platform_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    }
}

#[tauri::command]
fn app_info(app: AppHandle) -> AppInfo {
    let st = app.state::<AppState>();
    let s = st.settings();
    AppInfo {
        hotkey_display: hotkey::parse(&s.hotkey).map(|h| h.display()).unwrap_or_else(|| s.hotkey.clone()),
        detected_claude: claude_path::resolve(s.claude_path.as_deref()).map(|p| p.to_string_lossy().into_owned()),
        first_unfinished: s.first_unfinished_step(),
        settings: s,
        platform: platform_name(),
        version: env!("CARGO_PKG_VERSION"),
        log_path: logging::path().map(|p| p.to_string_lossy().into_owned()),
        steps: Settings::steps_for_platform(),
        required: Settings::required_steps(),
        translucent: translucent_supported(),
    }
}

fn persist(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let s = st.settings();
    s.save(&settings_path(&st.paths.config_dir)).map_err(|_| "Magic Mend could not save its settings file.".to_string())
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let old = st.settings();
        let mut new = settings;
        new.setup = old.setup.clone(); // the wizard owns its own progress
        if new.hotkey != old.hotkey {
            register_hotkey(&app, &new.hotkey)?;
        }
        if new.start_at_login != old.start_at_login {
            apply_autostart(&app, new.start_at_login);
        }
        *st.settings.lock().unwrap() = new;
        persist(&app)?;
        configure_engine(&app);
        refresh_tray(&app);
        let _ = app.emit("settings:changed", ());
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn apply_autostart(app: &AppHandle, on: bool) {
    let al = app.autolaunch();
    let _ = if on { al.enable() } else { al.disable() };
}

#[derive(Serialize)]
struct HotkeyCheck {
    ok: bool,
    display: String,
    warnings: Vec<String>,
    error: Option<String>,
}

#[tauri::command]
fn check_hotkey(app: AppHandle, hotkey_str: String) -> HotkeyCheck {
    let Some(parsed) = hotkey::parse(&hotkey_str) else {
        return HotkeyCheck {
            ok: false,
            display: hotkey_str,
            warnings: vec![],
            error: Some("Use Ctrl, Alt, or Cmd together with a letter, a number, Space, or F1 to F24.".into()),
        };
    };
    let warnings = hotkey::warnings(&parsed);
    let plugin_str = parsed.to_plugin_string();
    let st = app.state::<AppState>();
    let current = st.registered.lock().unwrap().clone();
    let mut error = None;
    if current.as_deref() != Some(plugin_str.as_str()) {
        let gs = app.global_shortcut();
        if gs.register(plugin_str.as_str()).is_ok() {
            let _ = gs.unregister(plugin_str.as_str());
        } else {
            error = Some(format!("Another app is already using {}.", parsed.display()));
        }
    }
    HotkeyCheck { ok: error.is_none(), display: parsed.display(), warnings, error }
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    open_settings_window(&app);
}

fn open_settings_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Magic Mend Settings")
        .inner_size(640.0, 720.0)
        .min_inner_size(520.0, 480.0)
        .center()
        .build();
}

// ---------------------------------------------------------------------------------------------
// Setup wizard

fn open_wizard(app: &AppHandle, step: Option<&str>) {
    let step = step.unwrap_or("welcome").to_string();
    if let Some(w) = app.get_webview_window("wizard") {
        let _ = w.emit("wizard:goto", &step);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let url = format!("wizard.html?step={step}");
    let built = WebviewWindowBuilder::new(app, "wizard", WebviewUrl::App(url.into()))
        .title("Magic Mend setup")
        .inner_size(560.0, 640.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .center()
        .build();
    if let Ok(w) = built {
        apply_effects(&w);
        let _ = w.set_focus();
    }
}

fn apply_effects(w: &WebviewWindow) {
    #[cfg(windows)]
    {
        if imp::windows_build() >= 22000 {
            let _ = w.set_effects(tauri::utils::config::WindowEffectsConfig {
                effects: vec![tauri::window::Effect::Acrylic],
                ..Default::default()
            });
        }
        if let Ok(h) = w.hwnd() {
            imp::round_corners(h.0 as isize);
        }
    }
    #[cfg(target_os = "macos")]
    {
        imp::apply_vibrancy(w);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = w;
}

#[derive(Serialize)]
struct ClaudeCheck {
    found: bool,
    path: Option<String>,
    version: Option<String>,
    signed_in: Option<bool>,
    message: Option<String>,
}

#[tauri::command]
async fn wizard_check_claude(app: AppHandle) -> ClaudeCheck {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<AppState>().settings();
        let Some(path) = claude_path::resolve(s.claude_path.as_deref()) else {
            return ClaudeCheck { found: false, path: None, version: None, signed_in: None, message: Some(ErrorKind::NotFound.message().into()) };
        };
        let p = path.to_string_lossy().into_owned();
        match claude_path::version(&path) {
            Ok(v) => {
                let signed_in = claude_path::signed_in(&path);
                let message = (signed_in == Some(false)).then(|| ErrorKind::NotSignedIn.message().to_string());
                ClaudeCheck { found: true, path: Some(p), version: Some(v), signed_in, message }
            }
            Err(_) => ClaudeCheck {
                found: true,
                path: Some(p),
                version: None,
                signed_in: None,
                message: Some("The claude command did not run. Check the path, or reinstall Claude Code.".into()),
            },
        }
    })
    .await
    .unwrap_or(ClaudeCheck { found: false, path: None, version: None, signed_in: None, message: Some(ErrorKind::Other.message().into()) })
}

#[derive(Serialize)]
pub struct EngineTest {
    pub ok: bool,
    pub message: String,
    pub ms: u128,
    pub sample: Option<String>,
}

/// Sends one tiny test request through the engine, exactly as the popup would.
pub fn run_engine_test(cfg: EngineConfig) -> EngineTest {
    let started = Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut eng = ClaudeEngine::new(EngineConfig { priming: false, ..cfg }, tx);
    if let Err(e) = eng.start() {
        return EngineTest { ok: false, message: e.0.message().into(), ms: 0, sample: None };
    }
    let req = Request {
        text: "thanks for helping me move last weekend".into(),
        tone: "Simple".into(),
        length: "Same".into(),
        toggles: vec![],
        channel: "Text".into(),
        followup: None,
        drafts: None,
    };
    if let Err(e) = eng.send(&req.to_json()) {
        return EngineTest { ok: false, message: e.0.message().into(), ms: 0, sample: None };
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            eng.stop();
            return EngineTest { ok: false, message: ErrorKind::Timeout.message().into(), ms: started.elapsed().as_millis(), sample: None };
        }
        match rx.recv_timeout(left) {
            Ok(t) => match t.event {
                EngineEvent::TurnDone { error: Some(k), .. } | EngineEvent::Exited { error: k } => {
                    eng.stop();
                    return EngineTest { ok: false, message: k.message().into(), ms: started.elapsed().as_millis(), sample: None };
                }
                EngineEvent::TurnDone { text, .. } => {
                    eng.stop();
                    let ms = started.elapsed().as_millis();
                    return match parse_reply(&text) {
                        Some(r) => EngineTest { ok: true, message: "Claude is connected.".into(), ms, sample: r.variants.first().map(|v| v.text.clone()) },
                        None => EngineTest { ok: false, message: ErrorKind::BadReply.message().into(), ms, sample: None },
                    };
                }
                _ => {}
            },
            Err(_) => continue,
        }
    }
}

#[tauri::command]
async fn wizard_test_engine(app: AppHandle) -> EngineTest {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let s = st.settings();
        match engine_config(&s, &st.paths) {
            Some(cfg) => {
                let r = run_engine_test(cfg);
                logging::event("wizard_engine_test", &[("ok", if r.ok { "1" } else { "0" }), ("ms", &r.ms.to_string())]);
                r
            }
            None => EngineTest { ok: false, message: ErrorKind::NotFound.message().into(), ms: 0, sample: None },
        }
    })
    .await
    .unwrap_or(EngineTest { ok: false, message: ErrorKind::Other.message().into(), ms: 0, sample: None })
}

#[tauri::command]
fn set_claude_path(app: AppHandle, path: Option<String>) -> Result<(), String> {
    let st = app.state::<AppState>();
    st.settings.lock().unwrap().claude_path = path.filter(|p| !p.trim().is_empty());
    persist(&app)
}

#[tauri::command]
async fn wizard_complete(app: AppHandle, step: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let was_done = st.settings().required_done();
        {
            let mut s = st.settings.lock().unwrap();
            if !s.setup.completed.contains(&step) {
                s.setup.completed.push(step.clone());
            }
            s.setup.pending_new.retain(|p| p != &step);
        }
        persist(&app)?;
        if !was_done && st.settings().required_done() {
            configure_engine(&app);
        }
        refresh_tray(&app);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn wizard_save_defaults(app: AppHandle, tone: String, length: String, start_at_login: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        {
            let mut s = st.settings.lock().unwrap();
            s.default_tone = tone;
            s.default_length = length;
            s.start_at_login = start_at_login;
        }
        apply_autostart(&app, start_at_login);
        persist(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_hotkey(app: AppHandle, hotkey_str: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        register_hotkey(&app, &hotkey_str)?;
        let st = app.state::<AppState>();
        let parsed = hotkey::parse(&hotkey_str).ok_or("That hotkey is not valid.")?;
        st.settings.lock().unwrap().hotkey = parsed.display().replace("Option", "Alt").replace("Cmd", "Super");
        persist(&app)?;
        refresh_tray(&app);
        Ok(parsed.display())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn wizard_finish(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    {
        let mut s = st.settings.lock().unwrap();
        s.setup.version = SETUP_VERSION;
        s.setup.pending_new.clear();
        if !s.setup.completed.iter().any(|c| c == "done") {
            s.setup.completed.push("done".into());
        }
    }
    persist(&app)?;
    configure_engine(&app);
    refresh_tray(&app);
    Ok(())
}

#[tauri::command]
fn wizard_at(app: AppHandle, step: String) {
    *app.state::<AppState>().wizard_step.lock().unwrap() = step;
}

#[tauri::command]
fn close_window(window: WebviewWindow) {
    let _ = window.close();
}

#[tauri::command]
fn access_status() -> bool {
    imp::accessibility_granted()
}

#[tauri::command]
fn open_access_settings() {
    imp::open_accessibility_settings();
}

#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart();
}

/// Only for automated tests: the shortcuts the app has sent so far.
#[tauri::command]
fn injected_keys() -> Vec<crate::platform::Letter> {
    crate::platform::injected_log()
}

// ---------------------------------------------------------------------------------------------
// Tray

fn tray_tooltip(s: &Settings) -> String {
    if s.paused {
        return "Magic Mend is paused".into();
    }
    let hk = hotkey::parse(&s.hotkey).map(|h| h.display()).unwrap_or_else(|| s.hotkey.clone());
    format!("Magic Mend, press {hk}")
}

fn build_menu(app: &AppHandle, s: &Settings) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    if !s.setup_finished() {
        menu.append(&MenuItem::with_id(app, "finish_setup", "Finish setup", true, None::<&str>)?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "setup_again", "Run setup again", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "pause", if s.paused { "Resume" } else { "Pause" }, true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit Magic Mend", true, None::<&str>)?)?;
    Ok(menu)
}

fn tray_icon(paused: bool) -> tauri::image::Image<'static> {
    let bytes: &'static [u8] = if paused { include_bytes!("../icons/tray-paused.png") } else { include_bytes!("../icons/tray.png") };
    tauri::image::Image::from_bytes(bytes).expect("tray icon")
}

fn refresh_tray(app: &AppHandle) {
    let s = app.state::<AppState>().settings();
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(m) = build_menu(app, &s) {
            let _ = tray.set_menu(Some(m));
        }
        let _ = tray.set_tooltip(Some(tray_tooltip(&s)));
        let _ = tray.set_icon(Some(tray_icon(s.paused)));
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "finish_setup" => {
            let s = app.state::<AppState>().settings();
            open_wizard(app, Some(s.first_unfinished_step()));
        }
        "settings" => open_settings_window(app),
        "setup_again" => open_wizard(app, Some("welcome")),
        "pause" => {
            let st = app.state::<AppState>();
            {
                let mut s = st.settings.lock().unwrap();
                s.paused = !s.paused;
            }
            let _ = persist(app);
            refresh_tray(app);
        }
        "quit" => {
            app.state::<AppState>().engine.lock().unwrap().shutdown();
            app.exit(0);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Startup

fn test_data_dir() -> Option<PathBuf> {
    std::env::var_os("MAGIC_MEND_DATA_DIR").map(PathBuf::from)
}

pub fn run() {
    let mut builder = tauri::Builder::default();
    if test_data_dir().is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch opens settings in the running app instead.
            open_settings_window(app);
        }));
    }
    builder
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let app = app.clone();
                        std::thread::spawn(move || hotkey_pressed(&app));
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            request_drafts,
            retry_drafts,
            replace_draft,
            copy_text,
            close_popup,
            resize_popup,
            target_status,
            app_info,
            save_settings,
            check_hotkey,
            set_hotkey,
            open_settings,
            wizard_check_claude,
            wizard_test_engine,
            set_claude_path,
            wizard_complete,
            wizard_save_defaults,
            wizard_finish,
            wizard_at,
            close_window,
            access_status,
            open_access_settings,
            restart_app,
            injected_keys,
        ])
        .on_window_event(|window, event| {
            if window.label() == "popup" {
                match event {
                    WindowEvent::Focused(false) => popup_focus_lost(window.app_handle()),
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let app = window.app_handle().clone();
                        std::thread::spawn(move || end_session(&app));
                    }
                    _ => {}
                }
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            // Automated tests point the app at a throwaway folder so real settings stay untouched.
            let (config_dir, data_dir, log_dir) = match test_data_dir() {
                Some(d) => (d.join("config"), d.join("data"), d.join("logs")),
                None => {
                    let data = app.path().app_local_data_dir()?;
                    let logs = app.path().app_log_dir().unwrap_or_else(|_| data.join("logs"));
                    (app.path().app_config_dir()?, data, logs)
                }
            };
            logging::init(log_dir);
            let prompt_dir = data_dir.join("prompt");
            std::fs::create_dir_all(&prompt_dir)?;
            let prompt_file = prompt_dir.join("system.txt");
            std::fs::write(&prompt_file, crate::SYSTEM_PROMPT)?;
            let paths = Paths { config_dir: config_dir.clone(), engine_dir: data_dir.join("engine"), prompt_file };
            std::fs::create_dir_all(&paths.engine_dir)?;
            let settings = Settings::load(&settings_path(&config_dir));
            let (engine, rx) = EngineManager::new(None, false, settings.checker_rules());
            app.manage(AppState {
                settings: Mutex::new(settings.clone()),
                paths,
                engine: Mutex::new(engine),
                session: Mutex::new(None),
                busy: AtomicBool::new(false),
                session_ids: AtomicU64::new(0),
                registered: Mutex::new(None),
                wizard_step: Mutex::new(String::new()),
            });

            // Engine events.
            {
                let app = handle.clone();
                std::thread::spawn(move || {
                    for t in rx {
                        let outs = app.state::<AppState>().engine.lock().unwrap().handle(t);
                        emit_outputs(&app, outs);
                    }
                });
            }
            // Timeouts, idle and wake recycling.
            {
                let app = handle.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let outs = app.state::<AppState>().engine.lock().unwrap().tick();
                    emit_outputs(&app, outs);
                });
            }

            let tray_menu = build_menu(&handle, &settings)?;
            TrayIconBuilder::with_id("main")
                .icon(tray_icon(settings.paused))
                .tooltip(tray_tooltip(&settings))
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        let app = tray.app_handle();
                        let s = app.state::<AppState>().settings();
                        if s.setup_finished() {
                            open_settings_window(app);
                        } else {
                            open_wizard(app, Some(s.first_unfinished_step()));
                        }
                    }
                })
                .build(app)?;

            if let Some(popup) = app.get_webview_window("popup") {
                apply_effects(&popup);
            }

            if let Err(msg) = register_hotkey(&handle, &settings.hotkey) {
                logging::event("hotkey_conflict", &[]);
                use tauri_plugin_dialog::DialogExt;
                handle.dialog().message(msg).title("Magic Mend").show(|_| {});
            }
            if settings.setup_finished() {
                apply_autostart(&handle, settings.start_at_login);
            }
            configure_engine(&handle);
            if !settings.setup_finished() {
                open_wizard(&handle, Some(settings.first_unfinished_step()));
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Magic Mend failed to start")
        .run(|app, event| match event {
            // A tray app keeps running when its windows close.
            tauri::RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
            tauri::RunEvent::Exit => app.state::<AppState>().engine.lock().unwrap().shutdown(),
            _ => {}
        });
}
