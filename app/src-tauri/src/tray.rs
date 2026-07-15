use std::sync::Arc;

use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, Runtime, Wry,
};
use uuid::Uuid;

use crate::models::TokenProfile;
use crate::store::AppStore;

const TRAY_ID: &str = "grok-switch-tray";

pub struct TrayState {
    pub store: Arc<AppStore>,
}

pub fn setup_tray(app: &AppHandle<Wry>, store: Arc<AppStore>) -> Result<(), String> {
    app.manage(TrayState {
        store: store.clone(),
    });

    let menu = build_menu(app, &store)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "无法加载菜单栏图标（default_window_icon）".to_string())?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .tooltip(tray_tooltip(&store))
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn host_of(base_url: Option<&str>) -> Option<String> {
    base_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or(s)
                .to_string()
        })
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn health_for(
    store: &AppStore,
    id: Uuid,
) -> Option<crate::health::HealthResult> {
    store
        .last_health()
        .into_iter()
        .find(|h| h.profile_id == id.to_string())
}

fn tray_tooltip(store: &AppStore) -> String {
    let snap = store.list_profiles();
    match snap
        .current_id
        .and_then(|cid| snap.profiles.iter().find(|p| p.id == cid))
    {
        Some(p) => {
            if let Some(h) = health_for(store, p.id) {
                if h.ok {
                    let ms = h
                        .latency_ms
                        .map(|v| format!(" · {v}ms"))
                        .unwrap_or_default();
                    format!("Grok Switch · {}{ms}", p.name)
                } else {
                    format!("Grok Switch · {} · {}", p.name, h.title)
                }
            } else {
                format!("Grok Switch · {}", p.name)
            }
        }
        None => "Grok Switch · 未启用".into(),
    }
}

fn append_info(
    menu: &Menu<Wry>,
    app: &AppHandle<Wry>,
    id: &str,
    text: impl Into<String>,
) -> Result<(), String> {
    let item =
        MenuItem::with_id(app, id, text.into(), false, None::<&str>).map_err(|e| e.to_string())?;
    menu.append(&item).map_err(|e| e.to_string())
}

fn profile_label(profile: &TokenProfile, is_current: bool) -> String {
    let mut label = profile.name.clone();
    if !profile.token_saved.unwrap_or(false) {
        label.push_str(" · 无密钥");
    } else if is_current {
        // keep clean when already checked
    }
    label
}

fn build_menu(app: &AppHandle<Wry>, store: &AppStore) -> Result<Menu<Wry>, String> {
    let snap = store.list_profiles();
    let current = snap.current_id;
    let usage = store.usage_summary(Some(24));
    let menu = Menu::new(app).map_err(|e| e.to_string())?;

    // —— 当前：只留名称 + 模型/端点一行 + 健康一行 ——
    match current.and_then(|cid| snap.profiles.iter().find(|p| p.id == cid)) {
        Some(p) => {
            append_info(&menu, app, "tray-header", format!("当前 · {}", p.name))?;

            let mut meta = p.model_id.clone();
            if let Some(host) = host_of(p.base_url.as_deref()) {
                meta.push_str(" · ");
                meta.push_str(&host);
            }
            append_info(&menu, app, "tray-meta", meta)?;

            let health_line = match health_for(store, p.id) {
                Some(h) if h.ok => match h.latency_ms {
                    Some(ms) => format!("健康 · {ms}ms"),
                    None => "健康 · 正常".into(),
                },
                Some(h) => format!("健康 · {}", h.title),
                None => "健康 · 未检测".into(),
            };
            append_info(&menu, app, "tray-health-info", health_line)?;
        }
        None => {
            append_info(&menu, app, "tray-header", "当前 · 未启用")?;
        }
    }

    // —— 用量：一行 ——
    if usage.has_data {
        let mut line = format!(
            "24h · {} 次 · {}",
            usage.total_calls,
            format_tokens(usage.total_tokens)
        );
        if usage.rate_limit_count > 0 {
            line.push_str(&format!(" · 限流 {}", usage.rate_limit_count));
        }
        append_info(&menu, app, "tray-usage", line)?;
    }

    // —— 切换供应商 ——
    menu.append(&PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    if snap.profiles.is_empty() {
        append_info(&menu, app, "tray-empty", "暂无供应商")?;
    } else {
        for profile in &snap.profiles {
            let id = format!("switch:{}", profile.id);
            let checked = current == Some(profile.id);
            let label = profile_label(profile, checked);
            let item = CheckMenuItem::with_id(app, id, label, true, checked, None::<&str>)
                .map_err(|e| e.to_string())?;
            menu.append(&item).map_err(|e| e.to_string())?;
        }
    }

    // —— 操作：精简 ——
    menu.append(&PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    let open = MenuItem::with_id(app, "tray-open", "打开主窗口", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let health = MenuItem::with_id(app, "tray-health", "检查健康度", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)
        .map_err(|e| e.to_string())?;

    menu.append(&open).map_err(|e| e.to_string())?;
    menu.append(&health).map_err(|e| e.to_string())?;
    menu.append(&quit).map_err(|e| e.to_string())?;

    Ok(menu)
}

pub fn rebuild_tray_menu(app: &AppHandle<Wry>, store: &AppStore) -> Result<(), String> {
    let menu = build_menu(app, store)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
        let _ = tray.set_tooltip(Some(tray_tooltip(store)));
    }
    Ok(())
}

fn handle_menu_event(app: &AppHandle<Wry>, id: &str) {
    match id {
        "tray-open" => {
            let _ = show_main_window(app);
        }
        "tray-quit" => {
            app.exit(0);
        }
        "tray-health" => {
            if let Some(state) = app.try_state::<TrayState>() {
                let store = state.store.clone();
                let app = app.clone();
                // Network probe off the UI/menu thread to avoid tray freezes.
                std::thread::spawn(move || {
                    let snap = store.list_profiles();
                    if let Some(cid) = snap.current_id {
                        let _ = app.emit("app://status", "正在检查当前供应商健康度…");
                        let result = store.check_health(cid);
                        let _ = app.emit("app://health", &result);
                        let _ = app.emit("app://status", result.status_line());
                        let _ = rebuild_tray_menu(&app, &store);
                    } else {
                        let _ = app.emit("app://status", "未启用供应商，无法检查健康度。");
                    }
                });
            }
        }
        other if other.starts_with("switch:") => {
            let id_str = &other["switch:".len()..];
            if let Ok(uuid) = Uuid::parse_str(id_str) {
                if let Some(state) = app.try_state::<TrayState>() {
                    let store = state.store.clone();
                    let app = app.clone();
                    // Apply + health both off the menu thread so switching never freezes the tray.
                    let _ = app.emit("app://status", "正在切换供应商…");
                    std::thread::spawn(move || {
                        let result = store.apply_token(Some(uuid), None);
                        let _ = app.emit("app://state", &result);
                        let _ = app.emit(
                            "app://status",
                            format!("{} · 正在检查健康度…", result.status),
                        );
                        let _ = rebuild_tray_menu(&app, &store);
                        let health = store.check_health(uuid);
                        let _ = app.emit("app://health", &health);
                        let _ = app.emit(
                            "app://status",
                            format!("{} · {}", result.status, health.status_line()),
                        );
                        let _ = rebuild_tray_menu(&app, &store);
                    });
                }
            }
        }
        _ => {}
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        let _ = window.unminimize();
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn refresh_tray(app: &AppHandle<Wry>) {
    if let Some(state) = app.try_state::<TrayState>() {
        let _ = rebuild_tray_menu(app, &state.store);
    }
}
