//! 启动器更新检查命令：逻辑在 update/，此处仅转发。

use crate::update::{check, UpdateInfo};

#[tauri::command]
pub async fn check_launcher_update() -> Result<Option<UpdateInfo>, String> {
    check().await
}
