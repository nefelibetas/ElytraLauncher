//! 实例升级：版本定义文件备份 / 回滚、完成后改名为目标版本、禁用不兼容 Mod。
//! 安装本身仍走 `DownloadTask`；本模块只处理任务前后的目录安全与迁移。

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::downloader::path_lock::PathLock;
use crate::game::modpack::resolve_version_meta;
use crate::game::updates::scan_mod_files;
use crate::game::version::dir::{active_minecraft_dir, find_version_json, version_server_section};
use crate::game::version::watch;
use crate::launcher::process;
use crate::persist;
use crate::util::{is_safe_filename, is_simple_name, loader_from_dir, mc_version_from_dir};

const BACKUP_PREFIX: &str = ".elytra-upgrade-";
const BACKUP_META: &str = "backup.json";
const BACKUP_FILES: &str = "files";
const READY_MARKER: &str = "ready";
const LOCK_RETRIES: u32 = 40;
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// 串行化备份 / 回滚 / 提交，避免同一实例并发操作。
static UPGRADE_LOCK: Mutex<()> = Mutex::new(());

fn default_true() -> bool {
    true
}

/// 升级备份元数据（持久化在 `versions/.elytra-upgrade-<旧名>/backup.json`）。
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "game/")]
pub struct UpgradeBackupMeta {
    pub from_version: String,
    /// 提交时写入的最终目录名；未提交为 None。
    pub to_version: Option<String>,
    /// 旧目录名是否由 MC / Loader 自动生成；自定义实例名保持原名。
    #[serde(default = "default_true")]
    pub rename_to_target: bool,
    pub target_mc: String,
    /// `vanilla` / `fabric` / `forge` / `neoforge` / `quilt`。
    pub target_loader: String,
    pub target_loader_version: Option<String>,
    /// 升级完成后要改名为 `.disabled` 的 Mod 文件名。
    pub disable_mods: Vec<String>,
    #[ts(type = "number")]
    pub created_at: u64,
    /// 安装任务已成功结束（由 `mark_ready` 写入 ready 标记）。
    #[serde(default)]
    pub install_done: bool,
}

/// 实例升级页的当前状态。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "game/")]
pub struct InstanceUpgradeInfo {
    pub mc_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
    pub mod_count: u32,
    /// 该版本目录正被下载/升级任务占用。
    pub busy: bool,
    pub pending: Option<UpgradeBackupMeta>,
}

/// 准备升级的目标版本。
#[derive(Debug, Clone)]
pub struct UpgradeTarget {
    pub mc_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "game/")]
pub struct UpgradeCommitResult {
    pub old_version: String,
    pub new_version: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "game/")]
pub struct UpgradeRollbackResult {
    pub restored_version: String,
}

/// 该版本存在未完成的升级，或目录正被安装/升级任务占用时阻止启动。
pub fn ensure_no_pending_upgrade(mc: &Path, version_name: &str) -> Result<(), String> {
    if find_backup(mc, version_name).is_some() {
        bail_key!(
            "errors.upgrade.pendingLaunch",
            "该实例正在升级，请先在实例设置中完成或回滚升级"
        );
    }
    let vdir = versions_dir(mc).join(version_name);
    if vdir.is_dir() && PathLock::try_acquire(&vdir).is_none() {
        bail_key!(
            "errors.upgrade.busyLaunch",
            "该实例正在执行安装或升级任务，请等待任务结束"
        );
    }
    Ok(())
}

/// 读取实例当前 MC / Loader / Mod 数与未完成的升级。
pub fn upgrade_info(version_name: &str) -> Result<InstanceUpgradeInfo, String> {
    upgrade_info_in(&active_minecraft_dir(), version_name)
}

fn upgrade_info_in(mc: &Path, version_name: &str) -> Result<InstanceUpgradeInfo, String> {
    if !is_simple_name(version_name) {
        bail_key!("errors.version.badName", "版本名非法");
    }
    let versions = versions_dir(mc);
    let mut vdir = versions.join(version_name);
    if !vdir.exists() {
        // 提交已改名但前端路由仍是旧名：用备份记录的 to_version 读取新目录。
        let Some((_, meta)) = find_backup(mc, version_name) else {
            bail_key!(
                "errors.version.missing",
                "版本目录不存在\n{}",
                vdir.display()
            );
        };
        let Some(to) = meta
            .to_version
            .as_deref()
            .filter(|name| is_simple_name(name))
        else {
            bail_key!(
                "errors.version.missing",
                "版本目录不存在\n{}",
                vdir.display()
            );
        };
        vdir = versions.join(to);
        if !vdir.exists() {
            bail_key!(
                "errors.version.missing",
                "版本目录不存在\n{}",
                vdir.display()
            );
        }
    }
    let effective_name = vdir
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(version_name);
    let (mc_version, loader, loader_version) = resolve_version_meta(&vdir, effective_name);
    let mod_count = scan_mod_files(&vdir.join("mods")).len() as u32;
    let busy = PathLock::try_acquire(&vdir).is_none();
    let pending = find_backup(mc, version_name).map(|(_, meta)| meta);
    Ok(InstanceUpgradeInfo {
        mc_version,
        loader,
        loader_version,
        mod_count,
        busy,
        pending,
    })
}

/// 校验目标、备份版本文件并写入待升级元数据；不启动安装任务。
pub fn prepare(
    version_name: &str,
    target: &UpgradeTarget,
    disable_mods: Vec<String>,
) -> Result<InstanceUpgradeInfo, String> {
    prepare_in(&active_minecraft_dir(), version_name, target, disable_mods)
}

fn prepare_in(
    mc: &Path,
    version_name: &str,
    target: &UpgradeTarget,
    disable_mods: Vec<String>,
) -> Result<InstanceUpgradeInfo, String> {
    let _guard = UPGRADE_LOCK.lock();
    ensure_game_closed()?;
    if !is_simple_name(version_name) {
        bail_key!("errors.version.badName", "版本名非法");
    }
    let (target_mc, target_loader, target_loader_version) = normalize_target(target)?;

    let vdir = versions_dir(mc).join(version_name);
    if !vdir.exists() {
        bail_key!(
            "errors.version.missing",
            "版本目录不存在\n{}",
            vdir.display()
        );
    }
    let (cur_mc, cur_loader, cur_loader_version) = resolve_version_meta(&vdir, version_name);
    if cur_mc == target_mc
        && cur_loader == target_loader
        && normalize_loader_version(cur_loader_version.as_deref())
            == normalize_loader_version(target_loader_version.as_deref())
    {
        bail_key!("errors.upgrade.sameVersion", "目标版本与当前实例相同");
    }
    let dir_loader = loader_from_dir(version_name);
    let rename_to_target = mc_version_from_dir(version_name) == cur_mc.as_str()
        && match cur_loader.as_str() {
            "vanilla" => dir_loader.is_none(),
            loader => dir_loader.is_some_and(|(kind, _)| kind == loader),
        };

    if find_backup(mc, version_name).is_some() {
        bail_key!("errors.upgrade.pending", "该实例有未完成的升级，请先回滚");
    }

    let _lock = acquire_version_lock(&vdir)?;
    let backup = backup_dir(mc, version_name);
    if backup.exists() {
        if read_backup_meta(&backup).is_some() {
            bail_key!("errors.upgrade.pending", "该实例有未完成的升级，请先回滚");
        }
        // 备份元数据未写完（崩溃残留）：安装任务尚未启动，版本文件未被修改，可安全重建。
        std::fs::remove_dir_all(&backup).map_err(common_err_ctx_key!(
            "errors.file.delete",
            "清理残留升级备份失败"
        ))?;
    }

    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(backup.join(BACKUP_FILES)).map_err(common_err_ctx_key!(
            "errors.file.createDir",
            "创建升级备份目录失败"
        ))?;
        let copied = copy_version_files(&vdir, &backup.join(BACKUP_FILES))?;
        if copied == 0 {
            bail_key!(
                "errors.upgrade.versionFilesMissing",
                "版本目录缺少 JSON / JAR，无法安全升级"
            );
        }
        let mut disabled = normalize_disable_mods(disable_mods);
        disabled.sort();
        disabled.dedup();
        let meta = UpgradeBackupMeta {
            from_version: version_name.to_string(),
            to_version: None,
            rename_to_target,
            target_mc,
            target_loader,
            target_loader_version,
            disable_mods: disabled,
            created_at: now_secs(),
            install_done: false,
        };
        write_backup_meta(&backup, &meta)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&backup);
    }
    result?;
    drop(_lock);
    upgrade_info_in(mc, version_name)
}

/// 安装任务成功后标记可提交；提交前必须存在该标记，防止把半成品误判为升级完成。
pub fn mark_ready(version_name: &str) -> Result<(), String> {
    let _guard = UPGRADE_LOCK.lock();
    mark_ready_in(&active_minecraft_dir(), version_name)
}

fn mark_ready_in(mc: &Path, version_name: &str) -> Result<(), String> {
    let (backup, _) = find_backup(mc, version_name)
        .ok_or_else(|| keyed_msg!("errors.upgrade.noBackup", "没有可用的升级备份"))?;
    std::fs::write(backup.join(READY_MARKER), b"1")
        .map_err(common_err_ctx_key!("errors.file.write", "写入升级标记失败"))
}

/// 下载任务成功后自动标记（仅当备份目标与任务 MC / Loader 及版本一致）。
/// 应用重启后前端钩子丢失，仍可在升级页提交已完成的安装。
pub fn mark_ready_for_task(version_name: &str, mc_version: &str, loader: Option<(&str, &str)>) {
    mark_ready_for_task_in(&active_minecraft_dir(), version_name, mc_version, loader);
}

fn mark_ready_for_task_in(
    mc: &Path,
    version_name: &str,
    mc_version: &str,
    loader: Option<(&str, &str)>,
) {
    let Some((backup, meta)) = find_backup(mc, version_name) else {
        return;
    };
    let (task_loader, task_version) = loader.unwrap_or(("vanilla", ""));
    let target_version = meta.target_loader_version.as_deref().unwrap_or("");
    if meta.target_mc != mc_version
        || meta.target_loader != task_loader
        || target_version != task_version
    {
        return;
    }
    let _ = std::fs::write(backup.join(READY_MARKER), b"1");
}

/// 回滚到升级前的版本文件；若目录已改名到目标名，会先改回旧名。
pub fn rollback(version_name: &str) -> Result<UpgradeRollbackResult, String> {
    rollback_in(&active_minecraft_dir(), version_name)
}

fn rollback_in(mc: &Path, version_name: &str) -> Result<UpgradeRollbackResult, String> {
    let _guard = UPGRADE_LOCK.lock();
    ensure_game_closed()?;
    let (backup, meta) = find_backup(mc, version_name)
        .ok_or_else(|| keyed_msg!("errors.upgrade.noBackup", "没有可回滚的升级备份"))?;
    validate_backup_meta(&meta)?;
    let versions = versions_dir(mc);
    let from_dir = versions.join(&meta.from_version);
    let to_dir = meta.to_version.as_ref().map(|name| versions.join(name));
    let current = if from_dir.exists() {
        if to_dir
            .as_ref()
            .is_some_and(|dir| *dir != from_dir && dir.exists())
        {
            bail_key!("errors.upgrade.ambiguous", "升级目录状态异常，无法自动回滚");
        }
        from_dir.clone()
    } else if let Some(dir) = to_dir.filter(|dir| dir.exists()) {
        dir
    } else {
        bail_key!("errors.upgrade.dirMissing", "升级目录不存在，无法回滚");
    };

    let _lock = acquire_version_lock(&from_dir)?;
    // Windows 上聚焦版本目录的 watcher 句柄会阻止目录改名；先解绑，guard 结束恢复。
    let _watch = watch::suspend_focus();
    let current = if current != from_dir {
        if from_dir.exists() {
            bail_key!("errors.upgrade.ambiguous", "升级目录状态异常，无法自动回滚");
        }
        std::fs::rename(&current, &from_dir).map_err(common_err_ctx_key!(
            "errors.version.renameFailed",
            "回滚版本目录失败"
        ))?;
        from_dir
    } else {
        current
    };

    restore_version_files(&backup.join(BACKUP_FILES), &current)?;
    finish_backup(&backup);
    Ok(UpgradeRollbackResult {
        restored_version: meta.from_version,
    })
}

/// 提交升级：改 JSON id / jar 名 / 目录名，迁移快速进服配置，禁用不兼容 Mod，清理备份。
pub fn commit(version_name: &str) -> Result<UpgradeCommitResult, String> {
    commit_in(&active_minecraft_dir(), version_name)
}

fn commit_in(mc: &Path, version_name: &str) -> Result<UpgradeCommitResult, String> {
    let _guard = UPGRADE_LOCK.lock();
    ensure_game_closed()?;
    let (backup, mut meta) = find_backup(mc, version_name)
        .ok_or_else(|| keyed_msg!("errors.upgrade.noBackup", "没有可提交的升级备份"))?;
    validate_backup_meta(&meta)?;
    if !meta.install_done {
        bail_key!(
            "errors.upgrade.notReady",
            "升级安装任务尚未成功结束，不能提交"
        );
    }

    let versions = versions_dir(mc);
    let from_dir = versions.join(&meta.from_version);
    let current = if from_dir.exists() {
        from_dir.clone()
    } else if let Some(dir) = meta.to_version.as_ref().map(|name| versions.join(name)) {
        if dir.exists() {
            dir
        } else {
            bail_key!("errors.upgrade.dirMissing", "升级目录不存在，无法提交");
        }
    } else {
        bail_key!("errors.upgrade.dirMissing", "升级目录不存在，无法提交");
    };

    let _lock = acquire_version_lock(&from_dir)?;
    // Windows 上聚焦版本目录的 watcher 句柄会阻止目录改名；先解绑，guard 结束恢复。
    let _watch = watch::suspend_focus();
    let final_name = if meta.rename_to_target {
        match meta.to_version.clone() {
            Some(name) => name,
            None => unique_version_name(&versions, &final_base(&meta), &meta.from_version),
        }
    } else {
        meta.from_version.clone()
    };
    if meta.to_version.as_deref() != Some(final_name.as_str()) {
        meta.to_version = Some(final_name.clone());
        write_backup_meta(&backup, &meta)?;
    }

    let current_name = current
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| keyed_msg!("errors.upgrade.dirMissing", "升级目录名非法"))?
        .to_string();
    let final_dir = versions.join(&final_name);
    if current_name != final_name && final_dir.exists() {
        bail_key!(
            "errors.upgrade.dirConflict",
            "目标版本目录已存在\n{}",
            final_name
        );
    }
    // 自定义实例名不改目录，但 JSON id 必须记录目标 MC，否则 resolve_version_meta 会读到旧名。
    rewrite_json_id(&current, &current_name, &final_name, &meta.target_mc)?;
    if current_name != final_name {
        rename_client_jar(&current, &current_name, &final_name)?;
        std::fs::rename(&current, &final_dir).map_err(common_err_ctx_key!(
            "errors.version.renameFailed",
            "升级版本目录改名失败"
        ))?;
    }

    update_pack_marker(&final_dir, &meta.target_mc)?;
    let (installed_mc, installed_loader, installed_version) =
        resolve_version_meta(&final_dir, &final_name);
    let version_ok = if meta.target_loader == "vanilla" {
        installed_version.is_none()
    } else {
        installed_version.as_deref() == meta.target_loader_version.as_deref()
    };
    if installed_mc != meta.target_mc || installed_loader != meta.target_loader || !version_ok {
        bail_key!(
            "errors.upgrade.targetMismatch",
            "安装结果与目标版本不一致，请回滚后重试"
        );
    }
    disable_mods(&final_dir, &meta.disable_mods)?;
    // 快速进服配置迁移失败不回滚已完成升级（与重命名版本目录语义一致）。
    let _ = migrate_server_section(&meta.from_version, &final_name);
    finish_backup(&backup);
    Ok(UpgradeCommitResult {
        old_version: meta.from_version,
        new_version: final_name,
    })
}

/// 先删元数据再删目录：中途崩溃后 `find_backup` 不再命中，`prepare` 会把无元数据的残留目录当垃圾清理。
fn finish_backup(backup: &Path) {
    let _ = std::fs::remove_file(backup.join(BACKUP_META));
    let _ = std::fs::remove_dir_all(backup);
}

fn find_backup(mc: &Path, version_name: &str) -> Option<(PathBuf, UpgradeBackupMeta)> {
    let versions = versions_dir(mc);
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&versions)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(BACKUP_PREFIX))
        })
        .collect();
    entries.sort();
    entries.into_iter().find_map(|dir| {
        let meta = read_backup_meta(&dir)?;
        (meta.from_version == version_name || meta.to_version.as_deref() == Some(version_name))
            .then_some((dir, meta))
    })
}

fn validate_backup_meta(meta: &UpgradeBackupMeta) -> Result<(), String> {
    if !is_simple_name(&meta.from_version)
        || meta
            .to_version
            .as_deref()
            .is_some_and(|name| !is_simple_name(name))
        || !is_simple_name(&meta.target_mc)
        || !is_simple_name(&meta.target_loader)
        || meta
            .target_loader_version
            .as_deref()
            .is_some_and(|version| !is_simple_name(version))
        || (meta.target_loader != "vanilla" && meta.target_loader_version.is_none())
    {
        bail_key!("errors.version.badName", "升级备份数据非法");
    }
    Ok(())
}

fn read_backup_meta(dir: &Path) -> Option<UpgradeBackupMeta> {
    let text = std::fs::read_to_string(dir.join(BACKUP_META)).ok()?;
    let mut meta: UpgradeBackupMeta = serde_json::from_str(&text).ok()?;
    meta.install_done = dir.join(READY_MARKER).is_file();
    Some(meta)
}

fn write_backup_meta(dir: &Path, meta: &UpgradeBackupMeta) -> Result<(), String> {
    let text = serde_json::to_string_pretty(meta).map_err(common_err_ctx_key!(
        "errors.config.serializeFailed",
        "序列化升级备份失败"
    ))?;
    let target = dir.join(BACKUP_META);
    let tmp = dir.join(format!("{BACKUP_META}.tmp"));
    std::fs::write(&tmp, text)
        .map_err(common_err_ctx_key!("errors.file.write", "写入升级备份失败"))?;
    if target.exists() {
        std::fs::remove_file(&target).map_err(common_err_ctx_key!(
            "errors.file.delete",
            "更新升级备份失败"
        ))?;
    }
    std::fs::rename(&tmp, &target)
        .map_err(common_err_ctx_key!("errors.file.write", "写入升级备份失败"))
}

fn copy_version_files(from: &Path, to: &Path) -> Result<u32, String> {
    let mut copied = 0;
    for entry in std::fs::read_dir(from)
        .map_err(common_err_ctx_key!("errors.file.read", "读取版本目录失败"))?
    {
        let entry = entry.map_err(common_err_ctx_key!("errors.file.read", "读取版本目录失败"))?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if !is_version_file(&entry.file_name()) {
            continue;
        }
        std::fs::copy(entry.path(), to.join(entry.file_name()))
            .map_err(common_err_ctx_key!("errors.file.copy", "备份版本文件失败"))?;
        copied += 1;
    }
    Ok(copied)
}

fn restore_version_files(backup_files: &Path, vdir: &Path) -> Result<(), String> {
    let backed: HashSet<String> = std::fs::read_dir(backup_files)
        .map_err(common_err_ctx_key!("errors.file.read", "读取升级备份失败"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    for entry in std::fs::read_dir(vdir)
        .map_err(common_err_ctx_key!("errors.file.read", "读取版本目录失败"))?
    {
        let entry = entry.map_err(common_err_ctx_key!("errors.file.read", "读取版本目录失败"))?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json.tmp") {
            std::fs::remove_file(entry.path()).map_err(common_err_ctx_key!(
                "errors.file.delete",
                "清理升级临时文件失败"
            ))?;
            continue;
        }
        if is_version_file(&entry.file_name()) && !backed.contains(&name) {
            std::fs::remove_file(entry.path()).map_err(common_err_ctx_key!(
                "errors.file.delete",
                "清理升级文件失败"
            ))?;
        }
    }
    for name in backed {
        std::fs::copy(backup_files.join(&name), vdir.join(&name))
            .map_err(common_err_ctx_key!("errors.file.copy", "恢复版本文件失败"))?;
    }
    Ok(())
}

fn rewrite_json_id(
    vdir: &Path,
    current_name: &str,
    final_name: &str,
    json_id: &str,
) -> Result<(), String> {
    let json_path = find_version_json(vdir, current_name)
        .ok_or_else(|| keyed_msg!("errors.upgrade.versionJsonMissing", "未找到版本 JSON"))?;
    let text = std::fs::read_to_string(&json_path).map_err(common_err_ctx_key!(
        "errors.file.read",
        "读取版本 JSON 失败"
    ))?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(common_err_ctx_key!(
        "errors.version.parse",
        "解析版本 JSON 失败"
    ))?;
    let Some(object) = value.as_object_mut() else {
        bail_key!("errors.version.parse", "版本 JSON 不是对象");
    };
    object.insert(
        "id".to_string(),
        serde_json::Value::String(json_id.to_string()),
    );
    let text = serde_json::to_string_pretty(&value).map_err(common_err_ctx_key!(
        "errors.config.serializeFailed",
        "序列化版本 JSON 失败"
    ))?;
    let target = vdir.join(format!("{final_name}.json"));
    let tmp = vdir.join(format!("{final_name}.json.tmp"));
    std::fs::write(&tmp, text).map_err(common_err_ctx_key!(
        "errors.file.write",
        "写入版本 JSON 失败"
    ))?;
    if target.exists() {
        std::fs::remove_file(&target).map_err(common_err_ctx_key!(
            "errors.file.delete",
            "替换版本 JSON 失败"
        ))?;
    }
    std::fs::rename(&tmp, &target).map_err(common_err_ctx_key!(
        "errors.file.write",
        "替换版本 JSON 失败"
    ))?;
    if json_path != target {
        std::fs::remove_file(&json_path).map_err(common_err_ctx_key!(
            "errors.file.delete",
            "删除旧版本 JSON 失败"
        ))?;
    }
    Ok(())
}

fn rename_client_jar(vdir: &Path, current_name: &str, final_name: &str) -> Result<(), String> {
    let from = vdir.join(format!("{current_name}.jar"));
    let to = vdir.join(format!("{final_name}.jar"));
    if !from.is_file() {
        if to.is_file() {
            return Ok(());
        }
        bail_key!(
            "errors.upgrade.clientJarMissing",
            "未找到客户端 JAR，无法完成升级"
        );
    }
    if to.exists() {
        std::fs::remove_file(&to).map_err(common_err_ctx_key!(
            "errors.file.delete",
            "替换客户端 JAR 失败"
        ))?;
    }
    std::fs::rename(&from, &to).map_err(common_err_ctx_key!(
        "errors.file.write",
        "重命名客户端 JAR 失败"
    ))
}

fn disable_mods(vdir: &Path, names: &[String]) -> Result<(), String> {
    let mods = vdir.join("mods");
    for name in names {
        if !is_safe_filename(name) || !name.to_ascii_lowercase().ends_with(".jar") {
            continue;
        }
        let from = mods.join(name);
        if !from.is_file() {
            continue;
        }
        let to = mods.join(format!("{name}.disabled"));
        if to.exists() {
            continue;
        }
        std::fs::rename(&from, &to).map_err(common_err_ctx_key!(
            "errors.updates.disableFailed",
            "禁用 Mod 失败"
        ))?;
    }
    Ok(())
}

/// 快速进服配置按版本目录名存 TOML section；改名后迁移，失败不影响已完成的目录升级。
fn migrate_server_section(from: &str, to: &str) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    let (Ok(old_section), Ok(new_section)) =
        (version_server_section(from), version_server_section(to))
    else {
        return Ok(());
    };
    if let Ok(config) = persist::read::<VersionServerConfig>(&old_section) {
        persist::save(&new_section, &config).map_err(common_err_ctx_key!(
            "errors.config.write",
            "迁移快速进服配置失败"
        ))?;
        let _ = persist::remove(&old_section);
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct VersionServerConfig {
    server: Option<String>,
    port: Option<u16>,
}

fn update_pack_marker(vdir: &Path, target_mc: &str) -> Result<(), String> {
    let path = vdir.join("elytra-pack.json");
    if !path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).map_err(common_err_ctx_key!(
        "errors.file.read",
        "读取整合包标记失败"
    ))?;
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(());
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    object.insert(
        "mc_version".to_string(),
        serde_json::Value::String(target_mc.to_string()),
    );
    let text = serde_json::to_string(&value).map_err(common_err_ctx_key!(
        "errors.config.serializeFailed",
        "序列化整合包标记失败"
    ))?;
    let tmp = vdir.join("elytra-pack.json.tmp");
    std::fs::write(&tmp, text).map_err(common_err_ctx_key!(
        "errors.file.write",
        "写入整合包标记失败"
    ))?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(common_err_ctx_key!(
            "errors.file.delete",
            "更新整合包标记失败"
        ))?;
    }
    std::fs::rename(&tmp, &path).map_err(common_err_ctx_key!(
        "errors.file.write",
        "写入整合包标记失败"
    ))
}

fn final_base(meta: &UpgradeBackupMeta) -> String {
    if meta.target_loader == "vanilla" {
        meta.target_mc.clone()
    } else {
        format!(
            "{}-{}-{}",
            meta.target_mc,
            meta.target_loader,
            meta.target_loader_version.as_deref().unwrap_or("")
        )
    }
}

fn unique_version_name(versions: &Path, base: &str, current_name: &str) -> String {
    if base == current_name {
        return base.to_string();
    }
    let mut name = base.to_string();
    let mut n = 1;
    while versions.join(&name).exists() {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

fn normalize_target(target: &UpgradeTarget) -> Result<(String, String, Option<String>), String> {
    let mc = target.mc_version.trim();
    if mc.is_empty() || !is_simple_name(mc) {
        bail_key!("errors.upgrade.invalidTarget", "目标游戏版本不合法");
    }
    let loader = match target.loader.trim().to_ascii_lowercase().as_str() {
        "" | "vanilla" => "vanilla".to_string(),
        "fabric" | "forge" | "neoforge" | "quilt" => target.loader.trim().to_ascii_lowercase(),
        _ => {
            bail_key!("errors.upgrade.invalidTarget", "目标游戏版本不合法")
        }
    };
    let loader_version = if loader == "vanilla" {
        None
    } else {
        let version = target.loader_version.as_deref().unwrap_or("").trim();
        if version.is_empty() || !is_simple_name(version) {
            bail_key!("errors.upgrade.loaderVersionEmpty", "请选择 ModLoader 版本");
        }
        Some(version.to_string())
    };
    Ok((mc.to_string(), loader, loader_version))
}

fn normalize_loader_version(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_disable_mods(names: Vec<String>) -> Vec<String> {
    names
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| is_safe_filename(name) && name.to_ascii_lowercase().ends_with(".jar"))
        .collect()
}

fn ensure_game_closed() -> Result<(), String> {
    if process::is_game_running() {
        bail_key!("errors.upgrade.gameRunning", "请先关闭正在运行的游戏");
    }
    Ok(())
}

fn acquire_version_lock(vdir: &Path) -> Result<PathLock, String> {
    for _ in 0..LOCK_RETRIES {
        if let Some(lock) = PathLock::try_acquire(vdir) {
            return Ok(lock);
        }
        std::thread::sleep(LOCK_RETRY_INTERVAL);
    }
    bail_key!("errors.upgrade.busy", "该实例正在执行其他任务")
}

fn versions_dir(mc: &Path) -> PathBuf {
    mc.join("versions")
}

fn backup_dir(mc: &Path, version_name: &str) -> PathBuf {
    versions_dir(mc).join(format!("{BACKUP_PREFIX}{version_name}"))
}

fn is_version_file(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("jar"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_mc(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("elytra-upgrade-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("versions/old")).unwrap();
        dir
    }

    #[test]
    fn backup_and_restore_version_files_roundtrip() {
        let mc = temp_mc("roundtrip");
        let vdir = mc.join("versions/old");
        std::fs::write(vdir.join("old.json"), r#"{"id":"old","mainClass":"Main"}"#).unwrap();
        std::fs::write(vdir.join("old.jar"), b"old").unwrap();
        std::fs::write(vdir.join("elytra-pack.json"), r#"{"mc_version":"1.20.1"}"#).unwrap();
        let backup = backup_dir(&mc, "old");
        std::fs::create_dir_all(backup.join(BACKUP_FILES)).unwrap();
        assert_eq!(
            copy_version_files(&vdir, &backup.join(BACKUP_FILES)).unwrap(),
            3
        );

        std::fs::write(vdir.join("old.json"), r#"{"id":"new","mainClass":"Main"}"#).unwrap();
        std::fs::write(vdir.join("old.jar"), b"new").unwrap();
        std::fs::write(vdir.join("final.json"), r#"{"id":"final"}"#).unwrap();
        std::fs::write(vdir.join("final.jar"), b"final").unwrap();
        restore_version_files(&backup.join(BACKUP_FILES), &vdir).unwrap();

        assert_eq!(
            std::fs::read_to_string(vdir.join("old.json")).unwrap(),
            r#"{"id":"old","mainClass":"Main"}"#
        );
        assert_eq!(
            std::fs::read_to_string(vdir.join("old.jar")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(vdir.join("elytra-pack.json")).unwrap(),
            r#"{"mc_version":"1.20.1"}"#
        );
        assert!(!vdir.join("final.json").exists());
        assert!(!vdir.join("final.jar").exists());
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn final_name_stays_when_base_matches_current() {
        let mc = temp_mc("same");
        assert_eq!(
            unique_version_name(&mc.join("versions"), "old", "old"),
            "old"
        );
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn unique_name_skips_existing_dirs() {
        let mc = temp_mc("unique");
        std::fs::create_dir_all(mc.join("versions/1.21.1")).unwrap();
        assert_eq!(
            unique_version_name(&mc.join("versions"), "1.21.1", "old"),
            "1.21.1_1"
        );
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn commit_file_renames_are_idempotent() {
        let mc = temp_mc("rename-files");
        let vdir = mc.join("versions/old");
        std::fs::write(
            vdir.join("old.json"),
            r#"{"id":"old","mainClass":"net.minecraft.client.main.Main"}"#,
        )
        .unwrap();
        std::fs::write(vdir.join("old.jar"), b"jar").unwrap();
        std::fs::write(
            vdir.join("elytra-pack.json"),
            r#"{"cache_key":"k","mc_version":"1.20.1"}"#,
        )
        .unwrap();

        rewrite_json_id(&vdir, "old", "new", "new").unwrap();
        rename_client_jar(&vdir, "old", "new").unwrap();
        assert!(!vdir.join("old.json").exists());
        assert!(!vdir.join("old.jar").exists());
        assert!(vdir.join("new.jar").exists());
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(vdir.join("new.json")).unwrap()).unwrap();
        assert_eq!(json["id"], "new");

        // 目录改名失败后重试：JSON/JAR 已是最终名，重复操作必须成功。
        rewrite_json_id(&vdir, "old", "new", "new").unwrap();
        rename_client_jar(&vdir, "old", "new").unwrap();
        update_pack_marker(&vdir, "1.21.1").unwrap();
        let marker: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(vdir.join("elytra-pack.json")).unwrap())
                .unwrap();
        assert_eq!(marker["mc_version"], "1.21.1");
        assert_eq!(marker["cache_key"], "k");
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn prepare_then_commit_renames_and_disables_mods() {
        let mc = temp_mc("commit-flow");
        let old_name = "1.20.1-fabric-0.14.0";
        let vdir = mc.join("versions").join(old_name);
        std::fs::rename(mc.join("versions/old"), &vdir).unwrap();
        std::fs::create_dir_all(vdir.join("mods")).unwrap();
        std::fs::write(
            vdir.join(format!("{old_name}.json")),
            format!(
                r#"{{"id":"{old_name}","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{{"name":"net.fabricmc:fabric-loader:0.14.0"}}]}}"#
            ),
        )
        .unwrap();
        std::fs::write(vdir.join(format!("{old_name}.jar")), b"old").unwrap();
        std::fs::write(
            vdir.join("elytra-pack.json"),
            r#"{"cache_key":"k","mc_version":"1.20.1"}"#,
        )
        .unwrap();
        std::fs::write(vdir.join("mods/a.jar"), b"a").unwrap();
        std::fs::write(vdir.join("mods/b.jar"), b"b").unwrap();
        std::fs::write(vdir.join("version-icon.png"), b"custom-icon").unwrap();
        std::fs::write(vdir.join("pack-icon.png"), b"pack-icon").unwrap();

        let target = UpgradeTarget {
            mc_version: "1.21.1".into(),
            loader: "fabric".into(),
            loader_version: Some("0.16.9".into()),
        };
        let info = prepare_in(&mc, old_name, &target, vec!["a.jar".into()]).unwrap();
        assert!(info.pending.is_some());

        // 模拟安装任务写入目标版本 JSON / JAR（loader 安装器用目录名作为 id）。
        std::fs::write(
            vdir.join(format!("{old_name}.json")),
            format!(
                r#"{{"id":"{old_name}","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{{"name":"net.fabricmc:fabric-loader:0.16.9"}}]}}"#
            ),
        )
        .unwrap();
        std::fs::write(vdir.join(format!("{old_name}.jar")), b"new").unwrap();
        mark_ready_in(&mc, old_name).unwrap();

        let result = commit_in(&mc, old_name).unwrap();
        assert_eq!(result.old_version, old_name);
        assert_eq!(result.new_version, "1.21.1-fabric-0.16.9");
        let final_dir = mc.join("versions").join(&result.new_version);
        assert!(final_dir.is_dir());
        assert!(!vdir.exists());
        assert!(final_dir.join("mods/a.jar.disabled").exists());
        assert!(final_dir.join("mods/b.jar").exists());
        assert_eq!(
            std::fs::read(final_dir.join("version-icon.png")).unwrap(),
            b"custom-icon"
        );
        assert_eq!(
            std::fs::read(final_dir.join("pack-icon.png")).unwrap(),
            b"pack-icon"
        );
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(final_dir.join(format!("{}.json", result.new_version)))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(json["id"], "1.21.1");

        let info = upgrade_info_in(&mc, &result.new_version).unwrap();
        assert_eq!(info.mc_version, "1.21.1");
        assert_eq!(info.loader, "fabric");
        assert_eq!(info.loader_version.as_deref(), Some("0.16.9"));
        assert!(info.pending.is_none());
        let marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(final_dir.join("elytra-pack.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(marker["mc_version"], "1.21.1");
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn prepare_then_rollback_restores_old_files() {
        let mc = temp_mc("rollback-flow");
        let vdir = mc.join("versions/old");
        std::fs::write(
            vdir.join("old.json"),
            r#"{"id":"old","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{"name":"net.fabricmc:fabric-loader:0.14.0"}]}"#,
        )
        .unwrap();
        std::fs::write(vdir.join("old.jar"), b"old").unwrap();
        std::fs::write(
            vdir.join("elytra-pack.json"),
            r#"{"cache_key":"k","mc_version":"1.20.1"}"#,
        )
        .unwrap();

        let target = UpgradeTarget {
            mc_version: "1.21.1".into(),
            loader: "vanilla".into(),
            loader_version: None,
        };
        prepare_in(&mc, "old", &target, Vec::new()).unwrap();
        std::fs::write(vdir.join("old.json"), r#"{"id":"new"}"#).unwrap();
        std::fs::write(vdir.join("old.jar"), b"new").unwrap();

        let result = rollback_in(&mc, "old").unwrap();
        assert_eq!(result.restored_version, "old");
        assert!(vdir.join("old.json").is_file());
        assert_eq!(
            std::fs::read_to_string(vdir.join("old.jar")).unwrap(),
            "old"
        );
        assert!(!mc.join("versions").join(".elytra-upgrade-old").exists());
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn commit_rejects_mismatched_loader_version_and_can_rollback() {
        let mc = temp_mc("mismatch-flow");
        let old_name = "1.20.1-fabric-0.14.0";
        let vdir = mc.join("versions").join(old_name);
        std::fs::rename(mc.join("versions/old"), &vdir).unwrap();
        std::fs::write(
            vdir.join(format!("{old_name}.json")),
            format!(
                r#"{{"id":"{old_name}","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{{"name":"net.fabricmc:fabric-loader:0.14.0"}}]}}"#
            ),
        )
        .unwrap();
        std::fs::write(vdir.join(format!("{old_name}.jar")), b"old").unwrap();
        std::fs::write(
            vdir.join("elytra-pack.json"),
            r#"{"cache_key":"k","mc_version":"1.20.1"}"#,
        )
        .unwrap();

        let target = UpgradeTarget {
            mc_version: "1.21.1".into(),
            loader: "fabric".into(),
            loader_version: Some("0.16.9".into()),
        };
        prepare_in(&mc, old_name, &target, Vec::new()).unwrap();
        std::fs::write(
            vdir.join(format!("{old_name}.json")),
            format!(
                r#"{{"id":"{old_name}","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{{"name":"net.fabricmc:fabric-loader:0.16.8"}}]}}"#
            ),
        )
        .unwrap();
        std::fs::write(vdir.join(format!("{old_name}.jar")), b"new").unwrap();
        mark_ready_in(&mc, old_name).unwrap();

        let error = commit_in(&mc, old_name).unwrap_err();
        assert!(
            error.contains("errors.upgrade.targetMismatch"),
            "应拒绝与目标 loader 版本不一致的安装结果: {error}"
        );
        // 提交在改名后才校验：回滚必须能把已改名的目录与文件一起还原。
        assert!(mc.join("versions/1.21.1-fabric-0.16.9").is_dir());
        let restored = rollback_in(&mc, old_name).unwrap();
        assert_eq!(restored.restored_version, old_name);
        assert!(vdir.is_dir());
        assert!(!mc.join("versions/1.21.1-fabric-0.16.9").exists());
        assert_eq!(
            std::fs::read_to_string(vdir.join(format!("{old_name}.jar"))).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn custom_instance_name_is_preserved_on_upgrade() {
        let mc = temp_mc("custom-name");
        let old_name = "我的整合包";
        let vdir = mc.join("versions").join(old_name);
        std::fs::rename(mc.join("versions/old"), &vdir).unwrap();
        std::fs::write(
            vdir.join(format!("{old_name}.json")),
            r#"{"id":"1.20.1","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{"name":"net.fabricmc:fabric-loader:0.14.0"}]}"#,
        )
        .unwrap();
        std::fs::write(vdir.join(format!("{old_name}.jar")), b"old").unwrap();
        std::fs::write(vdir.join("version-icon.png"), b"custom-icon").unwrap();

        let target = UpgradeTarget {
            mc_version: "1.21.1".into(),
            loader: "fabric".into(),
            loader_version: Some("0.16.9".into()),
        };
        prepare_in(&mc, old_name, &target, Vec::new()).unwrap();
        std::fs::write(
            vdir.join(format!("{old_name}.json")),
            format!(
                r#"{{"id":"{old_name}","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{{"name":"net.fabricmc:fabric-loader:0.16.9"}}]}}"#
            ),
        )
        .unwrap();
        std::fs::write(vdir.join(format!("{old_name}.jar")), b"new").unwrap();
        mark_ready_in(&mc, old_name).unwrap();

        let result = commit_in(&mc, old_name).unwrap();
        assert_eq!(result.new_version, old_name);
        assert!(vdir.is_dir());
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(vdir.join(format!("{old_name}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            json["id"], "1.21.1",
            "自定义实例名时 JSON id 必须记录目标 MC"
        );
        assert_eq!(
            std::fs::read(vdir.join("version-icon.png")).unwrap(),
            b"custom-icon"
        );
        let info = upgrade_info_in(&mc, old_name).unwrap();
        assert_eq!(info.mc_version, "1.21.1");
        assert_eq!(info.loader, "fabric");
        assert_eq!(info.loader_version.as_deref(), Some("0.16.9"));
        let _ = std::fs::remove_dir_all(&mc);
    }

    #[test]
    fn disable_mods_renames_jar_only() {
        let mc = temp_mc("disable");
        let vdir = mc.join("versions/old");
        std::fs::create_dir_all(vdir.join("mods")).unwrap();
        std::fs::write(vdir.join("mods/a.jar"), b"a").unwrap();
        std::fs::write(vdir.join("mods/b.jar"), b"b").unwrap();
        disable_mods(
            &vdir,
            &["a.jar".into(), "missing.jar".into(), "../b.jar".into()],
        )
        .unwrap();
        assert!(!vdir.join("mods/a.jar").exists());
        assert!(vdir.join("mods/a.jar.disabled").exists());
        assert!(vdir.join("mods/b.jar").exists());
        let _ = std::fs::remove_dir_all(&mc);
    }
}
