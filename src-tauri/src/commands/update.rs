//! 启动器更新命令：检查更新、下载文件清单、下载目录配置；下载本身复用工具箱下载器。

use std::path::Path;

use crate::config::launcher_update::LauncherUpdateConfig;
use crate::persist;
use crate::update::{check, download_plan, LauncherUpdatePlan, UpdateInfo};

#[tauri::command]
pub async fn check_launcher_update() -> Result<Option<UpdateInfo>, String> {
    check().await
}

/// Release 文件清单 + 本机平台 + 生效下载目录（配置为空时取平台默认值）。
#[tauri::command]
pub async fn get_launcher_update_plan() -> Result<LauncherUpdatePlan, String> {
    download_plan().await
}

#[tauri::command]
pub fn save_launcher_update_config(config: LauncherUpdateConfig) -> Result<(), String> {
    let dir = config.download_dir.trim();
    if !dir.is_empty() && !Path::new(dir).is_absolute() {
        bail_key!("errors.update.dirInvalid", "下载目录必须是绝对路径");
    }
    let config = LauncherUpdateConfig {
        download_dir: dir.to_string(),
    };
    persist::save_launcher_update_config(&config)
        .map_err(common_err_ctx_key!("errors.config.write", "保存配置失败"))?;
    Ok(())
}
