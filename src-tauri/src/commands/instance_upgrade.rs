//! 实例升级命令：备份 / 回滚 / 提交与 Mod 兼容性检查。

use crate::game::updates::{check_upgrade_compat, scan_mod_files, UpgradeReport};
use crate::game::version::dir::version_dir_path;
use crate::game::version::upgrade::{
    self, InstanceUpgradeInfo, UpgradeCommitResult, UpgradeRollbackResult, UpgradeTarget,
};
use crate::util::is_simple_name;

/// 当前实例的 MC / Loader / Mod 数与未完成的升级。
#[tauri::command]
pub async fn get_instance_upgrade_info(
    version_dir_name: String,
) -> Result<InstanceUpgradeInfo, String> {
    tauri::async_runtime::spawn_blocking(move || upgrade::upgrade_info(&version_dir_name))
        .await
        .map_err(common_err_ctx_key!(
            "errors.version.read",
            "读取版本信息失败"
        ))?
}

/// 检查实例 mods 对目标 MC / Loader 的兼容性（Modrinth 优先，未命中解析 jar 元数据）。
#[tauri::command]
pub async fn check_instance_upgrade(
    version_dir_name: String,
    target_mc: String,
    target_loader: String,
    target_loader_version: Option<String>,
) -> Result<UpgradeReport, String> {
    if !is_simple_name(&version_dir_name) {
        bail_key!("errors.version.badName", "版本名非法");
    }
    let mods_dir = version_dir_path(&version_dir_name)?.join("mods");
    let files = scan_mod_files(&mods_dir);
    Ok(check_upgrade_compat(
        &mods_dir,
        &files,
        &target_mc,
        &target_loader,
        target_loader_version.as_deref(),
    )
    .await)
}

/// 备份版本文件并记录本次升级目标；不启动安装任务。
#[tauri::command]
pub async fn prepare_instance_upgrade(
    version_dir_name: String,
    target_mc: String,
    target_loader: String,
    target_loader_version: Option<String>,
    disable_mods: Vec<String>,
) -> Result<InstanceUpgradeInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = UpgradeTarget {
            mc_version: target_mc,
            loader: target_loader,
            loader_version: target_loader_version,
        };
        upgrade::prepare(&version_dir_name, &target, disable_mods)
    })
    .await
    .map_err(common_err_ctx_key!(
        "errors.config.write",
        "保存升级信息失败"
    ))?
}

/// 安装任务成功结束后标记可提交。
#[tauri::command]
pub async fn mark_instance_upgrade_ready(version_dir_name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || upgrade::mark_ready(&version_dir_name))
        .await
        .map_err(common_err_ctx_key!("errors.file.write", "写入升级标记失败"))?
}

/// 回滚升级；目录已改名时一并改回旧名。
#[tauri::command]
pub async fn rollback_instance_upgrade(
    version_dir_name: String,
) -> Result<UpgradeRollbackResult, String> {
    tauri::async_runtime::spawn_blocking(move || upgrade::rollback(&version_dir_name))
        .await
        .map_err(common_err_ctx_key!("errors.file.delete", "回滚升级失败"))?
}

/// 提交升级：改目录/JSON/JAR 名、迁移快速进服配置、禁用不兼容 Mod。
#[tauri::command]
pub async fn commit_instance_upgrade(
    version_dir_name: String,
) -> Result<UpgradeCommitResult, String> {
    tauri::async_runtime::spawn_blocking(move || upgrade::commit(&version_dir_name))
        .await
        .map_err(common_err_ctx_key!("errors.file.write", "完成升级失败"))?
}
