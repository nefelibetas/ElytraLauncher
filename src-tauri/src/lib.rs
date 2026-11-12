#[macro_use]
mod macros;

mod auth;
mod commands;
pub mod config;
pub mod constants;
pub mod downloader;
mod events;
pub mod game;
mod jdk;
pub mod launcher;
mod map;
pub mod online;
mod persist;
mod seedmap;
mod server_status;
pub mod task;
pub mod tooldl;
pub mod ts_export;
mod update;
pub mod util;
mod window_state;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

use commands::account::{
    cancel_ms_login, disable_cape, ensure_ms_account, external_login, fetch_account_skin,
    fetch_account_skin_profile, fetch_capes, fetch_default_skin, fetch_external_skin,
    fetch_external_skin_profile, get_external_accounts, get_external_config, get_ms_account,
    get_ms_accounts, refresh_external_account, refresh_ms_account, remove_external_account,
    remove_ms_account, save_external_config, save_external_skin, save_skin, set_active_cape,
    start_ms_login, switch_external_account, switch_external_role, switch_ms_account,
    upload_skin_file,
};
use commands::auto_tasks::{
    get_instance_auto_tasks, run_auto_tasks_now, save_global_auto_tasks, save_instance_auto_tasks,
};
use commands::community::{
    clear_curseforge_key, get_community_details, get_community_versions, get_curseforge_key_source,
    resolve_mod_dependencies, search_community, set_curseforge_key,
};
use commands::config::take_config_recovery_notice;
use commands::downloader::{
    get_downloader_config, get_installed_versions, get_version_list, get_version_state,
    resolve_version_name, save_downloader_config,
};
use commands::gamedirs::{
    add_game_dir, auto_activate_version, delete_version_dir, ensure_game_folder,
    get_active_game_dir, get_game_dirs, open_folder, remove_game_dir, rename_game_dir,
    rename_version_dir, set_active_game_dir, set_watched_version,
};
use commands::jdk::{
    auto_scan_jdk, get_discovered_jdks, get_jdk_options, save_jdk_options, scan_custom_jdk,
};
use commands::launch::{
    begin_launch, cancel_launch, check_launch_env, get_game_log, get_launcher_log,
    open_launcher_log_dir, spawn_game,
};
use commands::launch_options::{
    get_instance_launch_options, get_launch_options, get_memory_info, save_instance_launch_options,
    save_launch_options,
};
use commands::launch_script::export_launch_script;
use commands::loader::{
    get_fabric_api_versions, get_fabric_versions, get_forge_versions, get_neoforge_versions,
    get_optifine_versions, get_quilt_versions,
};
use commands::map::{
    find_map_biome, find_map_folder_biome, find_map_slime_chunks, get_map_block,
    get_map_folder_block, get_map_folder_info, get_map_folder_tile, get_map_slime_chunks,
    get_map_tile, get_map_world_info, list_map_biomes, list_map_folder_biomes,
    list_map_folder_regions, list_map_regions, resolve_map_folders,
};
use commands::offline_account::{
    get_last_offline_username, get_offline_accounts, remove_offline_account,
    save_last_offline_username, save_offline_account,
};
use commands::online::{
    check_online_nodes, get_online_config, online_join, online_start_host, online_status,
    online_stop, save_online_config,
};
use commands::seedmap::{
    find_seed_map_slime_chunks, find_seed_map_structures, get_seed_map_block,
    get_seed_map_slime_chunks, get_seed_map_structures, get_seed_map_tile, get_seed_map_versions,
    resolve_seed_map_version,
};
use commands::server_status::ping_server;
use commands::skins::{
    export_skin_png, import_skin_png, list_local_skins, read_local_skin, remove_local_skin,
    save_local_skin,
};
use commands::task::{
    cancel_task, decide_waiting_task, dismiss_interrupted_task, download_files,
    download_single_file, install_modpack, list_interrupted_tasks, pause_task, resume_task,
    scan_missing_files, start_download, submit_install_jdk, submit_missing_download, task_exists,
};
use commands::tooldl::{
    tooldl_cancel, tooldl_clear_finished, tooldl_enqueue, tooldl_get_config, tooldl_list,
    tooldl_pause, tooldl_remove, tooldl_resume, tooldl_save_config,
};
use commands::update::check_launcher_update;
use commands::version::{
    check_mod_updates, delete_mod_file, delete_resource_file, delete_world, export_modpack,
    get_instance_mod_target, get_version_server, get_world_icon, identify_mod_file,
    list_mods_detailed, list_resource_detailed, list_resource_files, list_worlds,
    save_version_server, toggle_mod_file, update_mod_file,
};
use commands::version_icon::{
    get_version_icon, get_version_icon_source, reset_version_icon, set_version_icon,
};

/// 主窗口标签（tauri.conf.json 唯一窗口）。
const MAIN_WINDOW: &str = "main";
/// 托盘图标 id：更新多语言菜单时按 id 查找。
const TRAY_ID: &str = "main";
/// 托盘是否创建成功：Linux 缺少 appindicator 等环境时前端需回退为直接退出。
static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn tray_menu(app: &tauri::AppHandle, show: &str, quit: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let show_item = MenuItem::with_id(app, "show", show, true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", quit, true, None::<&str>)?;
    Menu::with_items(app, &[&show_item, &quit_item])
}

/// 创建系统托盘：菜单含「显示主窗口 / 退出」；双击图标恢复窗口。
fn init_tray(app: &tauri::App) -> Result<(), String> {
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| keyed_msg!("errors.tray.noIcon", "应用缺少窗口图标"))?;
    let menu = tray_menu(app.handle(), "显示主窗口", "退出").map_err(|e| e.to_string())?;
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        // macOS 菜单栏图标按模板渲染（单色），避免彩色图标在系统栏里突兀。
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        // Windows/Linux 左键恢复窗口、右键菜单；macOS 保持系统惯例（左键菜单）。
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => focus_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => focus_main_window(tray.app_handle()),
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    // TrayIcon 在最后一个实例 drop 时移除：交给 App 状态持有。
    app.manage(tray);
    TRAY_AVAILABLE.store(true, Ordering::Relaxed);
    Ok(())
}

/// 显示主窗口：恢复几何后显示；不铺原生底色，避免圆角外毛边与透明首帧。
fn reveal_main_window(app: &tauri::AppHandle) {
    let Some(win) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    if win.is_visible().unwrap_or(false) {
        return;
    }
    window_state::restore(app);
    focus_main_window(app);
}

#[tauri::command]
fn has_tray() -> bool {
    TRAY_AVAILABLE.load(Ordering::Relaxed)
}

/// 前端语言切换后同步托盘菜单文案（托盘由 Rust 持有，无法直接读前端 i18n）。
#[tauri::command]
fn set_tray_labels(app: tauri::AppHandle, show: String, quit: String) -> Result<(), String> {
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| keyed_msg!("errors.tray.notReady", "托盘未初始化"))?;
    let menu = tray_menu(&app, &show, &quit).map_err(|e| e.to_string())?;
    tray.set_menu(Some(menu)).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 子进程不得弹控制台：GUI 进程先持有一个隐藏控制台，第三方库内部 spawn
    // （easytier 的 cmd/ipconfig 等）才会继承它而非新建窗口
    util::ensure_hidden_console();
    // 启动器日志落盘（stdout/stderr → Elytra/launcher.log）
    util::init_file_logging();
    // 任务数据库恢复与工具箱配置均为首次访问惰性初始化；窗口创建前不碰 SQLite/TOML。

    let app = tauri::Builder::default()
        // 单实例须最先注册（其窗口过程在插件初始化时挂钩）
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 第二实例的启动请求：把已有窗口带前（可能处于最小化或失焦）
            for win in app.webview_windows().values() {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        // 自管窗口几何：统一写入 Elytra 配置目录，避免插件另建 identifier 目录。
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    window_state::save(window.app_handle());
                }
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                    window_state::record_normal(window);
                }
                _ => {}
            }
        })
        // 页面加载完成后再显示：骨架已可绘制，避免透明窗口先露出桌面。
        .on_page_load(|webview, payload| {
            if webview.label() != "main"
                || payload.event() != tauri::webview::PageLoadEvent::Finished
            {
                return;
            }
            reveal_main_window(webview.app_handle());
        })
        .setup(|app| {
            // 配置损坏恢复：运行期经事件推送（启动期由前端 take_config_recovery_notice 拉取兜底）
            let handle = app.handle().clone();
            persist::set_recovery_hook(move |notice| {
                let _ = handle.emit(events::CONFIG_RECOVERY, notice);
            });
            // 系统托盘：失败时仅记录，前端 has_tray 会回退为直接退出。
            if let Err(e) = init_tray(app) {
                eprintln!("托盘初始化失败: {e}");
            }
            // 游戏目录变化监听（激活目录的 versions/ 与聚焦版本子目录 → 事件推送）
            game::version::watch::init(app.handle().clone());
            // JDK 首扫放到窗口创建后：首次启动的递归扫盘 + java -version 子进程不与 WebView 争资源。
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(500));
                jdk::init();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auto_scan_jdk,
            scan_custom_jdk,
            get_jdk_options,
            save_jdk_options,
            get_discovered_jdks,
            submit_install_jdk,
            get_version_list,
            get_game_dirs,
            add_game_dir,
            rename_game_dir,
            remove_game_dir,
            set_active_game_dir,
            set_watched_version,
            get_active_game_dir,
            rename_version_dir,
            delete_version_dir,
            ensure_game_folder,
            open_folder,
            auto_activate_version,
            list_worlds,
            get_world_icon,
            get_map_world_info,
            list_map_regions,
            get_map_tile,
            get_map_block,
            resolve_map_folders,
            get_map_folder_info,
            list_map_folder_regions,
            get_map_folder_tile,
            get_map_folder_block,
            get_map_slime_chunks,
            find_map_slime_chunks,
            list_map_biomes,
            list_map_folder_biomes,
            find_map_biome,
            find_map_folder_biome,
            get_seed_map_versions,
            resolve_seed_map_version,
            get_seed_map_tile,
            get_seed_map_block,
            get_seed_map_structures,
            find_seed_map_structures,
            find_seed_map_slime_chunks,
            get_seed_map_slime_chunks,
            list_resource_files,
            list_resource_detailed,
            delete_resource_file,
            delete_world,
            identify_mod_file,
            list_mods_detailed,
            check_mod_updates,
            update_mod_file,
            delete_mod_file,
            get_instance_mod_target,
            get_version_server,
            save_version_server,
            toggle_mod_file,
            export_modpack,
            get_version_icon,
            get_version_icon_source,
            set_version_icon,
            reset_version_icon,
            export_launch_script,
            scan_missing_files,
            submit_missing_download,
            get_downloader_config,
            save_downloader_config,
            get_installed_versions,
            get_version_state,
            resolve_version_name,
            get_forge_versions,
            get_fabric_versions,
            get_quilt_versions,
            get_optifine_versions,
            get_neoforge_versions,
            get_fabric_api_versions,
            search_community,
            get_community_versions,
            get_community_details,
            resolve_mod_dependencies,
            get_curseforge_key_source,
            set_curseforge_key,
            clear_curseforge_key,
            start_download,
            download_files,
            download_single_file,
            install_modpack,
            cancel_task,
            pause_task,
            decide_waiting_task,
            task_exists,
            list_interrupted_tasks,
            resume_task,
            dismiss_interrupted_task,
            spawn_game,
            begin_launch,
            cancel_launch,
            get_game_log,
            get_launcher_log,
            open_launcher_log_dir,
            check_launch_env,
            get_memory_info,
            get_launch_options,
            save_launch_options,
            get_instance_launch_options,
            save_instance_launch_options,
            get_instance_auto_tasks,
            save_instance_auto_tasks,
            save_global_auto_tasks,
            run_auto_tasks_now,
            get_last_offline_username,
            save_last_offline_username,
            get_offline_accounts,
            save_offline_account,
            remove_offline_account,
            start_ms_login,
            external_login,
            get_external_accounts,
            switch_external_role,
            refresh_external_account,
            switch_external_account,
            remove_external_account,
            get_external_config,
            save_external_config,
            cancel_ms_login,
            get_ms_account,
            get_ms_accounts,
            remove_ms_account,
            switch_ms_account,
            ensure_ms_account,
            refresh_ms_account,
            fetch_account_skin,
            fetch_account_skin_profile,
            fetch_external_skin,
            fetch_external_skin_profile,
            save_external_skin,
            fetch_default_skin,
            save_skin,
            upload_skin_file,
            fetch_capes,
            set_active_cape,
            disable_cape,
            list_local_skins,
            save_local_skin,
            remove_local_skin,
            read_local_skin,
            export_skin_png,
            import_skin_png,
            online_start_host,
            online_join,
            online_stop,
            online_status,
            get_online_config,
            save_online_config,
            check_online_nodes,
            ping_server,
            tooldl_enqueue,
            tooldl_pause,
            tooldl_resume,
            tooldl_cancel,
            tooldl_remove,
            tooldl_clear_finished,
            tooldl_list,
            tooldl_get_config,
            tooldl_save_config,
            check_launcher_update,
            take_config_recovery_notice,
            has_tray,
            set_tray_labels
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| match event {
        // Ready 在窗口创建/居中完成后触发；几何恢复由页面加载完成后的 reveal 处理。
        tauri::RunEvent::Ready => {}
        tauri::RunEvent::ExitRequested { .. } => window_state::save(app_handle),
        _ => {}
    });
}
