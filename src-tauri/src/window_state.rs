//! 主窗口几何状态：尺寸/位置/最大化，统一落在 Elytra 配置目录（window-state.json）。
//! 不引入 standalone 插件，避免其把状态写进 `%APPDATA%\xyz.bridgefish.elytra` 造成双目录。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::util::config_dir;

const WINDOW_LABEL: &str = "main";
const FILE_NAME: &str = "window-state.json";
const LEGACY_FILE_NAME: &str = ".window-state.json";
/// 与 tauri.conf.json 的 minWidth / minHeight 保持一致。
const MIN_WIDTH: u32 = 900;
const MIN_HEIGHT: u32 = 560;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct WindowState {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    maximized: bool,
}

/// 最大化时 outer_size 是屏幕尺寸；这里缓存最大化前最后一次普通窗口几何。
static LAST_NORMAL: LazyLock<Mutex<Option<WindowState>>> = LazyLock::new(|| Mutex::new(None));

fn state_path() -> PathBuf {
    config_dir().join(FILE_NAME)
}

fn current_state(
    size: tauri::PhysicalSize<u32>,
    pos: tauri::PhysicalPosition<i32>,
    maximized: bool,
) -> WindowState {
    WindowState {
        width: size.width.max(MIN_WIDTH),
        height: size.height.max(MIN_HEIGHT),
        x: pos.x,
        y: pos.y,
        maximized,
    }
}

fn read_file(path: &Path) -> Option<WindowState> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn write_file(state: &WindowState) {
    let path = state_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let Ok(json) = serde_json::to_vec_pretty(state) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, json).is_err() {
        return;
    }
    if fs::rename(&tmp, &path).is_err() {
        // Windows 目标已存在时部分文件系统会拒绝 rename：删旧再试。
        let _ = fs::remove_file(&path);
        let _ = fs::rename(&tmp, &path);
    }
}

/// 读取旧插件写在 identifier 配置目录的状态，迁移后删除旧文件/空目录。
fn migrate_legacy(app: &tauri::AppHandle) -> Option<WindowState> {
    let dir = app.path().app_config_dir().ok()?;
    let text = fs::read_to_string(dir.join(LEGACY_FILE_NAME)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let main = value.get(WINDOW_LABEL)?;
    let state = WindowState {
        width: main.get("width")?.as_u64()? as u32,
        height: main.get("height")?.as_u64()? as u32,
        x: main.get("x")?.as_i64()? as i32,
        y: main.get("y")?.as_i64()? as i32,
        maximized: main
            .get("maximized")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    };
    write_file(&state);
    let _ = fs::remove_file(dir.join(LEGACY_FILE_NAME));
    let _ = fs::remove_dir(dir);
    Some(state)
}

/// 旧插件目录只承载 .window-state.json；迁移后清掉文件与空目录。
fn cleanup_legacy(app: &tauri::AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let _ = fs::remove_file(dir.join(LEGACY_FILE_NAME));
    let _ = fs::remove_dir(dir);
}

fn position_visible(win: &tauri::WebviewWindow, state: &WindowState) -> bool {
    let Ok(monitors) = win.available_monitors() else {
        return true;
    };
    if monitors.is_empty() {
        return true;
    }
    let x = i64::from(state.x);
    let y = i64::from(state.y);
    let w = i64::from(state.width);
    let h = i64::from(state.height);
    monitors.iter().any(|m| {
        let mp = m.position();
        let ms = m.size();
        x < i64::from(mp.x) + i64::from(ms.width)
            && x + w > i64::from(mp.x)
            && y < i64::from(mp.y) + i64::from(ms.height)
            && y + h > i64::from(mp.y)
    })
}

/// 启动时恢复窗口几何；状态缺失或损坏时保持 tauri.conf.json 的默认/居中。
pub fn restore(app: &tauri::AppHandle) {
    let Some(win) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    let loaded = read_file(&state_path());
    let state = loaded.or_else(|| migrate_legacy(app));
    // 新文件已存在时也清理旧插件的残留目录。
    if loaded.is_some() {
        cleanup_legacy(app);
    }
    let Some(state) = state else {
        // 首次运行/状态损坏：记录默认窗口几何，供首次最大化后关闭时复用。
        if let (Ok(size), Ok(pos)) = (win.outer_size(), win.outer_position()) {
            *LAST_NORMAL.lock() = Some(current_state(size, pos, false));
        }
        return;
    };
    let state = WindowState {
        width: state.width.max(MIN_WIDTH),
        height: state.height.max(MIN_HEIGHT),
        ..state
    };
    let _ = win.set_size(tauri::PhysicalSize::new(state.width, state.height));
    if position_visible(&win, &state) {
        let _ = win.set_position(tauri::PhysicalPosition::new(state.x, state.y));
    }
    if state.maximized {
        let _ = win.maximize();
    }
    // 保存的是普通窗口几何；最大化时 close 会复用它。
    *LAST_NORMAL.lock() = Some(WindowState {
        maximized: false,
        ..state
    });
}

/// 普通（非最大化）窗口的移动/缩放只更新内存缓存，不写盘。
pub fn record_normal(win: &tauri::Window) {
    if !win.is_visible().unwrap_or(true) || win.is_maximized().unwrap_or(false) {
        return;
    }
    let (Ok(size), Ok(pos)) = (win.outer_size(), win.outer_position()) else {
        return;
    };
    *LAST_NORMAL.lock() = Some(current_state(size, pos, false));
}

/// 关闭/退出时落盘；隐藏到托盘后由托盘退出时不覆盖已保存状态。
pub fn save(app: &tauri::AppHandle) {
    let Some(win) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    if !win.is_visible().unwrap_or(true) {
        return;
    }
    let maximized = win.is_maximized().unwrap_or(false);
    let state = if maximized {
        let cached = LAST_NORMAL.lock().as_ref().copied();
        cached
            .map(|state| WindowState {
                maximized: true,
                ..state
            })
            .or_else(|| {
                let (Ok(size), Ok(pos)) = (win.outer_size(), win.outer_position()) else {
                    return None;
                };
                Some(current_state(size, pos, true))
            })
    } else {
        let (Ok(size), Ok(pos)) = (win.outer_size(), win.outer_position()) else {
            return;
        };
        let state = current_state(size, pos, false);
        *LAST_NORMAL.lock() = Some(state);
        Some(state)
    };
    if let Some(state) = state {
        write_file(&state);
    }
}
