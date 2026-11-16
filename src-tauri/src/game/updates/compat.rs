//! 实例升级的 Mod 兼容性检查：Modrinth 文件元数据优先，未命中时解析 jar 内声明的 MC / Loader 版本范围。

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;

use super::identify::{file_sha1, QUERY_CONCURRENCY};
use super::scan::LocalModFile;
use crate::downloader::fetcher;
use crate::game::community::{api_routes, api_urls, get_json_multi, ApiPlatform};

/// 单个 Mod 的升级兼容状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "game/")]
pub enum UpgradeModStatus {
    Compatible,
    /// 当前文件不兼容，但 Modrinth 上存在目标 MC / Loader 的版本。
    Update,
    Incompatible,
    Unknown,
}

/// 单个 Mod 的兼容性结果。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "game/")]
pub struct UpgradeModReport {
    pub file_name: String,
    pub mod_id: Option<String>,
    /// 当前文件在 Modrinth 上的版本号（未识别为 None）。
    pub current_version: Option<String>,
    /// 目标版本可用的 Modrinth 版本号（仅 status=update）。
    pub compatible_version: Option<String>,
    pub status: UpgradeModStatus,
    /// 语言无关的原因码；前端按 `settings.version.setup.upgrade.reasons.<code>` 翻译。
    pub reason: Option<String>,
    /// 原因参数（声明的版本范围 / 支持的 Loader 列表等）。
    pub reason_detail: Option<String>,
}

/// 兼容性报告。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "game/")]
pub struct UpgradeReport {
    pub mods: Vec<UpgradeModReport>,
    pub compatible: u32,
    pub update: u32,
    pub incompatible: u32,
    pub unknown: u32,
    /// Modrinth 查询失败过（结果可能降级为本地元数据）。
    pub network_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeKind {
    Predicate,
    Maven,
}

#[derive(Debug, Default)]
struct ModMetadata {
    mod_id: Option<String>,
    loaders: Vec<&'static str>,
    /// (来源 Loader, 版本范围, 范围语法)：同一 jar 可能同时有 Fabric/Forge 元数据。
    mc_ranges: Vec<(&'static str, String, RangeKind)>,
    loader_ranges: Vec<(&'static str, String, RangeKind)>,
}

impl ModMetadata {
    fn merge(&mut self, other: ModMetadata) {
        if self.mod_id.is_none() {
            self.mod_id = other.mod_id;
        }
        for loader in other.loaders {
            if !self.loaders.contains(&loader) {
                self.loaders.push(loader);
            }
        }
        for (source, range, kind) in other.mc_ranges {
            if !self.mc_ranges.iter().any(|(s, _, _)| *s == source) {
                self.mc_ranges.push((source, range, kind));
            }
        }
        for (loader, range, kind) in other.loader_ranges {
            if !self.loader_ranges.iter().any(|(l, _, _)| *l == loader) {
                self.loader_ranges.push((loader, range, kind));
            }
        }
    }
}

/// 检查实例 mods 目录对目标 MC / Loader 的兼容性。
pub async fn check_upgrade_compat(
    mods_dir: &Path,
    files: &[LocalModFile],
    target_mc: &str,
    target_loader: &str,
    target_loader_version: Option<&str>,
) -> UpgradeReport {
    let dir = mods_dir.to_path_buf();
    let mc = target_mc.to_string();
    let loader = target_loader.to_string();
    let loader_version = target_loader_version.map(str::to_string);
    if loader == "vanilla" {
        return UpgradeReport {
            mods: files
                .iter()
                .map(|file| {
                    report(
                        &file.file_name,
                        None,
                        None,
                        None,
                        UpgradeModStatus::Incompatible,
                        Some("vanillaNoMods"),
                        None,
                    )
                })
                .collect(),
            compatible: 0,
            update: 0,
            incompatible: files.len() as u32,
            unknown: 0,
            network_error: false,
        };
    }
    let inputs: Vec<(String, PathBuf)> = files
        .iter()
        .map(|f| (f.file_name.clone(), dir.clone()))
        .collect();

    let results = futures_util::stream::iter(inputs.into_iter().map(|(file_name, dir)| {
        let mc = mc.clone();
        let loader = loader.clone();
        let loader_version = loader_version.clone();
        async move { check_one(&dir, &file_name, &mc, &loader, loader_version.as_deref()).await }
    }))
    .buffered(QUERY_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    let mut report = UpgradeReport {
        mods: Vec::with_capacity(results.len()),
        compatible: 0,
        update: 0,
        incompatible: 0,
        unknown: 0,
        network_error: false,
    };
    for (item, network_error) in results {
        report.network_error |= network_error;
        match item.status {
            UpgradeModStatus::Compatible => report.compatible += 1,
            UpgradeModStatus::Update => report.update += 1,
            UpgradeModStatus::Incompatible => report.incompatible += 1,
            UpgradeModStatus::Unknown => report.unknown += 1,
        }
        report.mods.push(item);
    }
    report.mods.sort_by(|a, b| {
        severity(a.status)
            .cmp(&severity(b.status))
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    report
}

fn severity(status: UpgradeModStatus) -> u8 {
    match status {
        UpgradeModStatus::Incompatible => 0,
        UpgradeModStatus::Update => 1,
        UpgradeModStatus::Unknown => 2,
        UpgradeModStatus::Compatible => 3,
    }
}

async fn check_one(
    mods_dir: &Path,
    file_name: &str,
    target_mc: &str,
    target_loader: &str,
    target_loader_version: Option<&str>,
) -> (UpgradeModReport, bool) {
    if let Some((status, reason, detail)) = optifine_status(file_name, target_mc, target_loader) {
        return (
            report(file_name, None, None, None, status, reason, detail),
            false,
        );
    }
    let path = mods_dir.join(file_name);
    let meta = read_meta(path).await;
    let mut network_error = false;

    let current = match file_sha1(mods_dir.join(file_name)).await {
        Some(sha1) => match modrinth_current(&sha1).await {
            Ok(hit) => hit,
            Err(()) => {
                network_error = true;
                None
            }
        },
        None => None,
    };

    if let Some(current) = current {
        let current_ok = game_versions_include(&current.game_versions, target_mc)
            && loaders_include(&current.loaders, target_loader);
        if current_ok {
            if let Some((status, reason, detail)) =
                loader_range_status(meta.as_ref(), target_loader, target_loader_version)
            {
                if status != UpgradeModStatus::Compatible {
                    return (
                        report(
                            file_name,
                            meta.as_ref(),
                            Some(current.version_number),
                            None,
                            status,
                            reason,
                            detail,
                        ),
                        network_error,
                    );
                }
            }
            return (
                report(
                    file_name,
                    meta.as_ref(),
                    Some(current.version_number),
                    None,
                    UpgradeModStatus::Compatible,
                    None,
                    None,
                ),
                network_error,
            );
        }

        match modrinth_compatible_version(&current.project_id, target_mc, target_loader).await {
            Ok(Some(version)) => {
                return (
                    report(
                        file_name,
                        meta.as_ref(),
                        Some(current.version_number),
                        Some(version),
                        UpgradeModStatus::Update,
                        Some("updateAvailable"),
                        None,
                    ),
                    network_error,
                );
            }
            Ok(None) => {
                return (
                    report(
                        file_name,
                        meta.as_ref(),
                        Some(current.version_number),
                        None,
                        UpgradeModStatus::Incompatible,
                        Some("noCompatibleVersion"),
                        None,
                    ),
                    network_error,
                );
            }
            Err(()) => {
                network_error = true;
            }
        }
    }

    let local = local_status(
        meta.as_ref(),
        target_mc,
        target_loader,
        target_loader_version,
    );
    let (status, reason, detail) = local;
    (
        report(file_name, meta.as_ref(), None, None, status, reason, detail),
        network_error,
    )
}

fn report(
    file_name: &str,
    meta: Option<&ModMetadata>,
    current_version: Option<String>,
    compatible_version: Option<String>,
    status: UpgradeModStatus,
    reason: Option<&str>,
    reason_detail: Option<String>,
) -> UpgradeModReport {
    UpgradeModReport {
        file_name: file_name.to_string(),
        mod_id: meta.and_then(|m| m.mod_id.clone()),
        current_version,
        compatible_version,
        status,
        reason: reason.map(str::to_string),
        reason_detail,
    }
}

/// 本地元数据判定：返回 (状态, 原因码, 原因参数)。
fn local_status(
    meta: Option<&ModMetadata>,
    target_mc: &str,
    target_loader: &str,
    target_loader_version: Option<&str>,
) -> (UpgradeModStatus, Option<&'static str>, Option<String>) {
    if target_loader == "vanilla" {
        return (UpgradeModStatus::Incompatible, Some("vanillaNoMods"), None);
    }
    let Some(meta) = meta else {
        return (UpgradeModStatus::Unknown, Some("noMetadata"), None);
    };
    if !meta
        .loaders
        .iter()
        .any(|loader| *loader == target_loader || (target_loader == "quilt" && *loader == "fabric"))
    {
        return (
            UpgradeModStatus::Incompatible,
            Some("loaderMismatch"),
            Some(meta.loaders.join(" / ")),
        );
    }

    let range = meta
        .mc_ranges
        .iter()
        .find(|(source, _, _)| *source == target_loader)
        .or_else(|| {
            (target_loader == "quilt")
                .then(|| {
                    meta.mc_ranges
                        .iter()
                        .find(|(source, _, _)| *source == "fabric")
                })
                .flatten()
        });
    let Some((_, mc_range, kind)) = range else {
        return (UpgradeModStatus::Unknown, Some("mcRangeMissing"), None);
    };
    match version_satisfies(mc_range, target_mc, *kind) {
        Some(true) => {}
        Some(false) => {
            return (
                UpgradeModStatus::Incompatible,
                Some("mcRangeMismatch"),
                Some(mc_range.clone()),
            );
        }
        None => {
            return (
                UpgradeModStatus::Unknown,
                Some("mcRangeUnparsed"),
                Some(mc_range.clone()),
            );
        }
    }

    if let Some((status, reason, detail)) =
        loader_range_status(Some(meta), target_loader, target_loader_version)
    {
        return (status, reason, detail);
    }
    (UpgradeModStatus::Compatible, None, None)
}

fn loader_range_status(
    meta: Option<&ModMetadata>,
    target_loader: &str,
    target_loader_version: Option<&str>,
) -> Option<(UpgradeModStatus, Option<&'static str>, Option<String>)> {
    let (meta, version) = (meta?, target_loader_version?);
    let (_, range, kind) = meta
        .loader_ranges
        .iter()
        .find(|(loader, _, _)| *loader == target_loader)?;
    match version_satisfies(range, version, *kind) {
        Some(true) => Some((UpgradeModStatus::Compatible, None, None)),
        Some(false) => Some((
            UpgradeModStatus::Incompatible,
            Some("loaderRangeMismatch"),
            Some(range.clone()),
        )),
        None => Some((
            UpgradeModStatus::Unknown,
            Some("loaderRangeUnparsed"),
            Some(range.clone()),
        )),
    }
}

fn game_versions_include(game_versions: &[String], target: &str) -> bool {
    game_versions.iter().any(|version| version == target)
}

/// OptiFine jar 用文件名标注 MC（`OptiFine_<mc>_<type>_<patch>.jar`），无 mods.toml，不能走元数据判定。
fn optifine_status(
    file_name: &str,
    target_mc: &str,
    target_loader: &str,
) -> Option<(UpgradeModStatus, Option<&'static str>, Option<String>)> {
    let rest = file_name
        .strip_prefix("OptiFine_")
        .or_else(|| file_name.strip_prefix("optifine_"))?;
    let jar_mc = rest.split('_').next().filter(|value| !value.is_empty())?;
    if jar_mc != target_mc {
        return Some((
            UpgradeModStatus::Incompatible,
            Some("optifineMcMismatch"),
            Some(jar_mc.to_string()),
        ));
    }
    if target_loader == "forge" {
        return Some((
            UpgradeModStatus::Unknown,
            Some("optifineForge"),
            Some(file_name.to_string()),
        ));
    }
    Some((UpgradeModStatus::Incompatible, Some("optifineLoader"), None))
}

fn loaders_include(loaders: &[String], target: &str) -> bool {
    loaders
        .iter()
        .any(|loader| loader == target || (target == "quilt" && loader == "fabric"))
}

// ── Modrinth 查询 ──────────────────────────────────────────────

struct ModrinthCurrent {
    project_id: String,
    version_number: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
}

async fn modrinth_current(sha1: &str) -> Result<Option<ModrinthCurrent>, ()> {
    let urls = api_routes(
        ApiPlatform::Modrinth,
        &format!("/v2/version_file/{sha1}?algorithm=sha1"),
    );
    match get_json_multi(&urls, |u| fetcher::http().get(u.to_string())).await {
        Ok(value) => {
            let Some(project_id) = value["project_id"].as_str().map(str::to_string) else {
                return Ok(None);
            };
            let Some(version_number) = value["version_number"].as_str().map(str::to_string) else {
                return Ok(None);
            };
            Ok(Some(ModrinthCurrent {
                project_id,
                version_number,
                game_versions: string_list(&value["game_versions"]),
                loaders: string_list(&value["loaders"]),
            }))
        }
        Err(e) if e.contains("HTTP 404") => Ok(None),
        Err(_) => Err(()),
    }
}

async fn modrinth_compatible_version(
    project_id: &str,
    target_mc: &str,
    target_loader: &str,
) -> Result<Option<String>, ()> {
    let mut params = vec![("game_versions", serde_json::json!([target_mc]).to_string())];
    params.push(("loaders", target_loader.to_string()));
    if target_loader == "quilt" {
        params.push(("loaders", "fabric".to_string()));
    }
    let urls = api_urls(
        ApiPlatform::Modrinth,
        &format!("/v2/project/{project_id}/version"),
        &params,
    )
    .map_err(|_| ())?;
    let data = get_json_multi(&urls, |u| fetcher::http().get(u.to_string()))
        .await
        .map_err(|_| ())?;
    let mut versions: Vec<&Value> = data
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    versions.sort_by(|a, b| {
        b["date_published"]
            .as_str()
            .unwrap_or("")
            .cmp(a["date_published"].as_str().unwrap_or(""))
    });
    Ok(versions
        .into_iter()
        .find(|version| {
            version["files"]
                .as_array()
                .is_some_and(|files| !files.is_empty())
        })
        .and_then(|version| version["version_number"].as_str().map(str::to_string)))
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ── jar 元数据解析 ──────────────────────────────────────────────

async fn read_meta(path: PathBuf) -> Option<ModMetadata> {
    tokio::task::spawn_blocking(move || read_meta_blocking(&path))
        .await
        .ok()
        .flatten()
}

fn read_meta_blocking(path: &Path) -> Option<ModMetadata> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let fabric = read_zip_text(&mut archive, "fabric.mod.json");
    let quilt = read_zip_text(&mut archive, "quilt.mod.json");
    let forge = read_zip_text(&mut archive, "META-INF/mods.toml");
    let neoforge = read_zip_text(&mut archive, "META-INF/neoforge.mods.toml");

    let mut meta = ModMetadata::default();
    if let Some(text) = fabric {
        meta.merge(parse_fabric(&text));
    }
    if let Some(text) = quilt {
        meta.merge(parse_quilt(&text));
    }
    if let Some(text) = forge {
        meta.merge(parse_forge(&text, "forge"));
    }
    if let Some(text) = neoforge {
        meta.merge(parse_forge(&text, "neoforge"));
    }
    (!meta.loaders.is_empty()).then_some(meta)
}

fn read_zip_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    if entry.size() > 1_048_576 {
        return None;
    }
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    Some(text)
}

fn parse_fabric(text: &str) -> ModMetadata {
    let mut meta = ModMetadata::default();
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return meta;
    };
    meta.mod_id = value["id"].as_str().map(str::to_string);
    meta.loaders.push("fabric");
    let Some(depends) = value.get("depends") else {
        return meta;
    };
    if let Some(range) = dependency_string(depends, "minecraft") {
        meta.mc_ranges.push(("fabric", range, RangeKind::Predicate));
    }
    if let Some(range) = dependency_string(depends, "fabricloader") {
        meta.loader_ranges
            .push(("fabric", range, RangeKind::Predicate));
    }
    meta
}

fn parse_quilt(text: &str) -> ModMetadata {
    let mut meta = ModMetadata::default();
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return meta;
    };
    let Some(loader) = value.get("quilt_loader") else {
        return meta;
    };
    meta.mod_id = loader["id"].as_str().map(str::to_string);
    meta.loaders.push("quilt");
    let Some(depends) = loader.get("depends") else {
        return meta;
    };
    match depends {
        Value::Array(list) => {
            for dep in list {
                let Some(id) = dep["id"].as_str() else {
                    continue;
                };
                let id = normalize_dep_id(id);
                let Some(range) = dependency_string(dep, "versions") else {
                    continue;
                };
                match id.as_str() {
                    "minecraft" => {
                        meta.mc_ranges.push(("quilt", range, RangeKind::Predicate));
                    }
                    "quilt_loader" => {
                        meta.loader_ranges
                            .push(("quilt", range, RangeKind::Predicate));
                    }
                    _ => {}
                }
            }
        }
        Value::Object(map) => {
            for (id, value) in map {
                let id = normalize_dep_id(id);
                let Some(range) = dependency_value_string(value) else {
                    continue;
                };
                match id.as_str() {
                    "minecraft" => {
                        meta.mc_ranges.push(("quilt", range, RangeKind::Predicate));
                    }
                    "quilt_loader" => {
                        meta.loader_ranges
                            .push(("quilt", range, RangeKind::Predicate));
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    meta
}

fn parse_forge(text: &str, source: &'static str) -> ModMetadata {
    let mut meta = ModMetadata::default();
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return meta;
    };
    meta.loaders.push(source);
    meta.mod_id = value
        .get("mods")
        .and_then(toml::Value::as_array)
        .and_then(|mods| mods.first())
        .and_then(|mod_info| mod_info.get("modId"))
        .and_then(toml::Value::as_str)
        .map(str::to_string);

    let Some(dependencies) = value.get("dependencies").and_then(toml::Value::as_table) else {
        return meta;
    };
    for entries in dependencies.values() {
        let Some(entries) = entries.as_array() else {
            continue;
        };
        for entry in entries {
            if entry.get("mandatory").and_then(toml::Value::as_bool) == Some(false) {
                continue;
            }
            let Some(mod_id) = entry.get("modId").and_then(toml::Value::as_str) else {
                continue;
            };
            let Some(range) = entry.get("versionRange").and_then(toml::Value::as_str) else {
                continue;
            };
            match normalize_dep_id(mod_id).as_str() {
                "minecraft" => meta
                    .mc_ranges
                    .push((source, range.to_string(), RangeKind::Maven)),
                "forge" => meta
                    .loader_ranges
                    .push(("forge", range.to_string(), RangeKind::Maven)),
                "neoforge" => {
                    meta.loader_ranges
                        .push(("neoforge", range.to_string(), RangeKind::Maven))
                }
                _ => {}
            }
        }
    }
    meta
}

fn dependency_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(dependency_value_string)
}

fn dependency_value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(list) => {
            let joined = list
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

fn normalize_dep_id(id: &str) -> String {
    id.to_ascii_lowercase().replace('-', "_")
}

// ── 版本范围 ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ver {
    nums: Vec<u64>,
    pre: Option<String>,
}

fn parse_ver(raw: &str) -> Option<Ver> {
    let raw = raw.trim().trim_start_matches('v');
    if raw.is_empty() || raw == "*" {
        return None;
    }
    let mut nums = Vec::new();
    let mut pre = None;
    for part in raw.split(['.', '-', '+']) {
        if part.is_empty() {
            continue;
        }
        let cut = part
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(part.len());
        if cut == 0 {
            pre = Some(part.to_ascii_lowercase());
            break;
        }
        nums.push(part[..cut].parse().ok()?);
        if cut < part.len() {
            pre = Some(part[cut..].to_ascii_lowercase());
            break;
        }
    }
    (!nums.is_empty()).then_some(Ver { nums, pre })
}

fn cmp_ver(a: &Ver, b: &Ver) -> Ordering {
    let len = a.nums.len().max(b.nums.len());
    for i in 0..len {
        let x = a.nums.get(i).copied().unwrap_or(0);
        let y = b.nums.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    match (&a.pre, &b.pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => a.cmp(b),
    }
}

fn version_satisfies(range: &str, target: &str, kind: RangeKind) -> Option<bool> {
    match kind {
        RangeKind::Predicate => predicate_satisfies(range, target),
        RangeKind::Maven => maven_satisfies(range, target),
    }
}

fn predicate_satisfies(range: &str, target: &str) -> Option<bool> {
    let target = parse_ver(target)?;
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return Some(true);
    }
    let mut any_group = false;
    let mut unknown = false;
    for group in range.split("||") {
        let mut saw_token = false;
        let mut all = true;
        for token in group.split([' ', ',']) {
            let token = token.trim().trim_end_matches('-');
            if token.is_empty() {
                continue;
            }
            saw_token = true;
            match predicate_token(token, &target) {
                Some(true) => {}
                Some(false) => {
                    all = false;
                    break;
                }
                None => {
                    unknown = true;
                    all = false;
                    break;
                }
            }
        }
        if saw_token && all {
            return Some(true);
        }
        any_group |= saw_token;
    }
    if unknown {
        None
    } else if any_group {
        Some(false)
    } else {
        Some(true)
    }
}

fn predicate_token(token: &str, target: &Ver) -> Option<bool> {
    let (op, rest) = [
        (">=", ">="),
        ("<=", "<="),
        (">", ">"),
        ("<", "<"),
        ("==", "=="),
        ("=", "="),
        ("~", "~"),
        ("^", "^"),
    ]
    .into_iter()
    .find_map(|(prefix, op)| token.strip_prefix(prefix).map(|rest| (op, rest)))
    .unwrap_or(("", token));
    let rest = rest.trim().trim_end_matches('-');
    if rest.is_empty() {
        return None;
    }
    if op.is_empty() && (rest.contains('x') || rest.contains('X') || rest.contains('*')) {
        return wildcard_satisfies(rest, target);
    }
    if op.is_empty() && rest.contains(',') {
        return maven_satisfies(rest, &format_ver(target));
    }
    let rhs = parse_ver(rest)?;
    let ordering = cmp_ver(target, &rhs);
    match op {
        ">=" => Some(ordering != Ordering::Less),
        "<=" => Some(ordering != Ordering::Greater),
        ">" => Some(ordering == Ordering::Greater),
        "<" => Some(ordering == Ordering::Less),
        "=" | "==" => Some(ordering == Ordering::Equal),
        "~" => {
            let upper = bump_minor(&rhs);
            Some(
                cmp_ver(target, &rhs) != Ordering::Less
                    && cmp_ver(target, &upper) == Ordering::Less,
            )
        }
        "^" => {
            let upper = bump_major(&rhs);
            Some(
                cmp_ver(target, &rhs) != Ordering::Less
                    && cmp_ver(target, &upper) == Ordering::Less,
            )
        }
        "" => {
            if rest.contains('x') || rest.contains('X') || rest.contains('*') {
                wildcard_satisfies(rest, target)
            } else {
                Some(ordering == Ordering::Equal)
            }
        }
        _ => None,
    }
}

fn wildcard_satisfies(raw: &str, target: &Ver) -> Option<bool> {
    let parts: Vec<&str> = raw.split('.').collect();
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    let mut upper_ready = false;
    for part in parts {
        if part.eq_ignore_ascii_case("x") || part == "*" {
            if !upper_ready {
                upper = lower.clone();
                if let Some(last) = upper.last_mut() {
                    *last += 1;
                } else {
                    return Some(true);
                }
                upper_ready = true;
            }
            break;
        }
        let value: u64 = part.parse().ok()?;
        lower.push(value);
        if !upper_ready {
            upper.push(value);
        }
    }
    if !upper_ready {
        return parse_ver(raw).map(|rhs| cmp_ver(target, &rhs) == Ordering::Equal);
    }
    let lower = Ver {
        nums: lower,
        pre: None,
    };
    let upper = Ver {
        nums: upper,
        pre: None,
    };
    Some(cmp_ver(target, &lower) != Ordering::Less && cmp_ver(target, &upper) == Ordering::Less)
}

fn bump_minor(version: &Ver) -> Ver {
    let mut nums = version.nums.clone();
    if nums.len() >= 2 {
        nums[1] += 1;
        nums.truncate(2);
    } else {
        nums[0] += 1;
        nums.truncate(1);
    }
    Ver { nums, pre: None }
}

fn bump_major(version: &Ver) -> Ver {
    let mut nums = version.nums.clone();
    if nums.first().copied().unwrap_or(0) == 0 && nums.len() >= 2 {
        nums[1] += 1;
        nums.truncate(2);
    } else {
        nums[0] += 1;
        nums.truncate(1);
    }
    Ver { nums, pre: None }
}

fn maven_satisfies(range: &str, target: &str) -> Option<bool> {
    let target = parse_ver(target)?;
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return Some(true);
    }
    let lower_inclusive = range.starts_with('[');
    let upper_inclusive = range.ends_with(']');
    if (lower_inclusive || range.starts_with('(')) && (upper_inclusive || range.ends_with(')')) {
        let inner = &range[1..range.len().saturating_sub(1)];
        let Some((lower, upper)) = inner.split_once(',') else {
            let exact = parse_ver(inner.trim())?;
            return Some(cmp_ver(&target, &exact) == Ordering::Equal);
        };
        let lower = lower.trim();
        let upper = upper.trim();
        if !lower.is_empty() {
            let lower = parse_ver(lower)?;
            match cmp_ver(&target, &lower) {
                Ordering::Less => return Some(false),
                Ordering::Equal if !lower_inclusive => return Some(false),
                _ => {}
            }
        }
        if !upper.is_empty() {
            let upper = parse_ver(upper)?;
            match cmp_ver(&target, &upper) {
                Ordering::Greater => return Some(false),
                Ordering::Equal if !upper_inclusive => return Some(false),
                _ => {}
            }
        }
        return Some(true);
    }
    let lower = parse_ver(range)?;
    Some(cmp_ver(&target, &lower) != Ordering::Less)
}

fn format_ver(version: &Ver) -> String {
    let mut text = version
        .nums
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(".");
    if let Some(pre) = &version.pre {
        text.push('-');
        text.push_str(pre);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_ranges_cover_common_forms() {
        assert_eq!(predicate_satisfies(">=1.20.1 <1.21", "1.20.4"), Some(true));
        assert_eq!(predicate_satisfies(">=1.20.1 <1.21", "1.21"), Some(false));
        assert_eq!(predicate_satisfies("1.20.x", "1.20.6"), Some(true));
        assert_eq!(predicate_satisfies("1.20.x", "1.21"), Some(false));
        assert_eq!(predicate_satisfies("~1.20.1", "1.20.6"), Some(true));
        assert_eq!(predicate_satisfies("~1.20.1", "1.21"), Some(false));
        assert_eq!(predicate_satisfies("^1.20", "1.21"), Some(true));
        assert_eq!(
            predicate_satisfies(">=1.20.1 || >=1.21", "1.21.1"),
            Some(true)
        );
        assert_eq!(predicate_satisfies(">=1.20.1", "1.20.1"), Some(true));
    }

    #[test]
    fn numeric_versions_not_lexicographic() {
        assert_eq!(predicate_satisfies(">=1.21.9", "1.21.10"), Some(true));
        assert_eq!(predicate_satisfies("<1.21.10", "1.21.9"), Some(true));
    }

    #[test]
    fn maven_ranges_cover_bounds() {
        assert_eq!(maven_satisfies("[1.20.1,1.21)", "1.20.1"), Some(true));
        assert_eq!(maven_satisfies("[1.20.1,1.21)", "1.21"), Some(false));
        assert_eq!(maven_satisfies("[1.20,1.20.6]", "1.20.6"), Some(true));
        assert_eq!(maven_satisfies("[1.20,1.20.6]", "1.20.7"), Some(false));
        assert_eq!(maven_satisfies("[1.20.1,)", "1.21"), Some(true));
        assert_eq!(maven_satisfies("1.20.1", "1.20.2"), Some(true));
    }

    #[test]
    fn parses_fabric_and_forge_metadata() {
        let fabric = parse_fabric(
            r#"{"id":"example","depends":{"minecraft":">=1.20.1","fabricloader":">=0.15.0"}}"#,
        );
        assert_eq!(fabric.mod_id.as_deref(), Some("example"));
        assert!(fabric.loaders.contains(&"fabric"));
        assert_eq!(
            fabric.mc_ranges.first().map(|(_, r, _)| r.as_str()),
            Some(">=1.20.1")
        );

        let forge = parse_forge(
            r#"
[[mods]]
modId = "example"
[[dependencies.example]]
modId = "minecraft"
versionRange = "[1.20.1,1.21)"
[[dependencies.example]]
modId = "forge"
versionRange = "[47,)"
"#,
            "forge",
        );
        assert_eq!(forge.mod_id.as_deref(), Some("example"));
        assert!(forge.loaders.contains(&"forge"));
        assert_eq!(
            forge.mc_ranges.first().map(|(_, r, _)| r.as_str()),
            Some("[1.20.1,1.21)")
        );
    }

    #[tokio::test]
    async fn vanilla_target_marks_all_mods_incompatible_without_metadata() {
        let file = LocalModFile {
            file_name: "a.jar".into(),
            size: 0,
            last_modified: 0,
        };
        let report = check_upgrade_compat(
            Path::new("missing-mods-dir"),
            &[file],
            "1.21.1",
            "vanilla",
            None,
        )
        .await;
        assert_eq!(report.incompatible, 1);
        assert_eq!(report.mods[0].reason.as_deref(), Some("vanillaNoMods"));
        assert!(!report.network_error);
    }

    #[test]
    fn optifine_filename_status() {
        let name = "OptiFine_1.20.1_HD_U_I6.jar";
        assert_eq!(
            optifine_status(name, "1.21.1", "forge").unwrap().0,
            UpgradeModStatus::Incompatible
        );
        assert_eq!(
            optifine_status(name, "1.20.1", "forge").unwrap().0,
            UpgradeModStatus::Unknown
        );
        assert_eq!(
            optifine_status(name, "1.20.1", "fabric").unwrap().0,
            UpgradeModStatus::Incompatible
        );
    }

    #[test]
    fn local_status_respects_loader_and_versions() {
        let meta = ModMetadata {
            mod_id: Some("example".into()),
            loaders: vec!["fabric"],
            mc_ranges: vec![("fabric", ">=1.20.1 <1.21".into(), RangeKind::Predicate)],
            loader_ranges: vec![("fabric", ">=0.15.0".into(), RangeKind::Predicate)],
        };
        assert_eq!(
            local_status(Some(&meta), "1.20.4", "fabric", Some("0.16.9")).0,
            UpgradeModStatus::Compatible
        );
        assert_eq!(
            local_status(Some(&meta), "1.21", "fabric", Some("0.16.9")).0,
            UpgradeModStatus::Incompatible
        );
        assert_eq!(
            local_status(Some(&meta), "1.20.4", "forge", Some("47.2.0")).0,
            UpgradeModStatus::Incompatible
        );
        assert_eq!(
            local_status(Some(&meta), "1.20.4", "fabric", Some("0.14.0")).0,
            UpgradeModStatus::Incompatible
        );
    }

    #[test]
    fn reads_fabric_metadata_from_jar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("example.jar");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"{"id":"example","depends":{"minecraft":">=1.20.1 <1.21"}}"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let meta = read_meta_blocking(&path).unwrap();
        assert_eq!(meta.mod_id.as_deref(), Some("example"));
        assert!(meta.loaders.contains(&"fabric"));
        assert_eq!(
            meta.mc_ranges.first().map(|(_, r, _)| r.as_str()),
            Some(">=1.20.1 <1.21")
        );
    }
}
