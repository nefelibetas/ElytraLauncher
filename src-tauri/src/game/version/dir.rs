//! 已安装版本目录域：定位 / 枚举 / 建目录 / 重命名 / 删除。
//! 路径基准 = downloader section 的 minecraft_dir（相对值按平台默认基准解析）。

use crate::config::downloader::DownloaderConfig;
use crate::config::game_dirs;
use crate::launcher::args::find_version_json;
use crate::persist::downloader_config;
use crate::util::{infer_loader_from_json, is_simple_name};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

/// 当前激活游戏目录（versions/ 父目录）绝对路径。
pub fn active_minecraft_dir() -> PathBuf {
    let config: DownloaderConfig = downloader_config();
    config.minecraft_dir_abs()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AutoActivate {
    pub dir_path: String,
    pub version: String,
}

/// 按配置列表顺序检测游戏目录，激活首个存在版本者，返回其字母序第一个版本名。
pub fn auto_activate_first_version() -> Result<AutoActivate, String> {
    let dirs = game_dirs::ensure_initialized();
    for d in &dirs {
        let vdir = Path::new(&d.path).join("versions");
        if !vdir.is_dir() {
            continue;
        }
        let mut names: Vec<String> = std::fs::read_dir(&vdir)
            .map_err(common_err_ctx_key!("errors.file.read", "读取目录失败"))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        if names.is_empty() {
            continue;
        }
        names.sort();
        let version = names[0].clone();
        game_dirs::set_active(&d.path)?;
        return Ok(AutoActivate {
            dir_path: d.path.clone(),
            version,
        });
    }
    Err(keyed_msg!(
        "errors.version.noneInstalled",
        "所有游戏目录中都没有已安装的版本"
    ))
}

/// 版本目录绝对路径（versions/{name}）。目录不存在时返回错误（读操作不该静默创建）。
pub fn version_dir_path(version_name: &str) -> Result<PathBuf, String> {
    let dir = active_minecraft_dir().join("versions").join(version_name);
    if !dir.exists() {
        bail_key!(
            "errors.version.missing",
            "版本目录不存在\n{}",
            dir.display()
        );
    }
    Ok(dir)
}

/// 版本级服务器直连配置的 persist section 名：`version_server_{版本目录名}`。
/// 版本名会直接成为 TOML 顶层 key，故做简单名校验（全仓唯一动态 section）。
pub fn version_server_section(version_name: &str) -> Result<String, String> {
    if !is_simple_name(version_name) {
        return Err(keyed_msg!(
            "errors.version.badName",
            "版本名非法\n仅允许不含路径分隔符与 `..` 的名称"
        ));
    }
    Ok(format!("version_server_{version_name}"))
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS, PartialEq)]
#[ts(export_to = "game/")]
pub struct InstalledVersionInfo {
    pub name: String,
    /// 从版本 json 推断的 loader；json 读取失败（非版本目录）时为空串
    pub loader: String,
}

/// 版本列表缓存指纹：目录/JSON 任一变化都会失效。
#[derive(Clone, PartialEq, Eq)]
struct VersionStamp {
    name: String,
    dir_mtime: u64,
    json: Option<(PathBuf, u64)>,
}

#[derive(Clone)]
struct VersionCache {
    stamps: Vec<VersionStamp>,
    items: Vec<InstalledVersionInfo>,
}

static VERSION_LIST_CACHE: LazyLock<Mutex<HashMap<PathBuf, VersionCache>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn mtime_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn list_installed_in(versions_dir: &Path) -> Result<Vec<InstalledVersionInfo>, String> {
    let mut dirs: Vec<(String, PathBuf)> = std::fs::read_dir(versions_dir)
        .map_err(common_err_ctx_key!(
            "errors.version.read",
            "读取版本目录失败"
        ))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            Some((
                entry.file_name().to_string_lossy().to_string(),
                entry.path(),
            ))
        })
        .collect();
    dirs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut stamps = Vec::with_capacity(dirs.len());
    for (name, path) in &dirs {
        let json = find_version_json(path, name).map(|p| (p.clone(), mtime_secs(&p)));
        stamps.push(VersionStamp {
            name: name.clone(),
            dir_mtime: mtime_secs(path),
            json,
        });
    }

    if let Some(hit) = VERSION_LIST_CACHE.lock().get(versions_dir) {
        if hit.stamps == stamps {
            return Ok(hit.items.clone());
        }
    }

    let mut cacheable = true;
    let items: Vec<InstalledVersionInfo> = dirs
        .into_iter()
        .zip(stamps.iter())
        .map(|((name, _), stamp)| {
            let loader = stamp
                .json
                .as_ref()
                .and_then(|(path, _)| match std::fs::read_to_string(path) {
                    Ok(text) => Some(infer_loader_from_json(&text)),
                    Err(_) => {
                        cacheable = false;
                        None
                    }
                })
                .unwrap_or_default();
            InstalledVersionInfo { name, loader }
        })
        .collect();

    if cacheable {
        let mut cache = VERSION_LIST_CACHE.lock();
        if cache.len() > 8 {
            cache.clear();
        }
        cache.insert(
            versions_dir.to_path_buf(),
            VersionCache {
                stamps,
                items: items.clone(),
            },
        );
    }
    Ok(items)
}

/// 枚举 versions/ 下子目录，loader 从目录内 json 推断（PCL2 实例名可能无 loader 标记），按名排序。
pub fn list_installed() -> Result<Vec<InstalledVersionInfo>, String> {
    let versions_dir = active_minecraft_dir().join("versions");
    if !versions_dir.exists() {
        return Ok(vec![]);
    }
    list_installed_in(&versions_dir)
}

/// 确保版本文件夹存在（不存在则创建）并返回绝对路径（快捷方式按钮用）。
/// 版本隔离：gameDir = versions/{name}，存档/mod 位于版本目录下。
pub fn ensure_kind_folder(kind: &str, version_name: &str) -> Result<String, String> {
    let base = active_minecraft_dir();
    let sub = match kind {
        "version" => format!("versions/{version_name}"),
        "saves" => format!("versions/{version_name}/saves"),
        "mods" => format!("versions/{version_name}/mods"),
        "resourcepacks" => format!("versions/{version_name}/resourcepacks"),
        "datapacks" => format!("versions/{version_name}/datapacks"),
        "shaderpacks" => format!("versions/{version_name}/shaderpacks"),
        _ => {
            return Err(keyed_msg!(
                "errors.version.unknownFolderKind",
                "未知文件夹类型"
            ))
        }
    };
    let dir = base.join(sub);
    std::fs::create_dir_all(&dir)
        .map_err(common_err_ctx_key!("errors.file.createDir", "创建目录失败"))?;
    Ok(dir.to_string_lossy().to_string())
}

/// 重命名版本文件夹（versions/{old} → versions/{new}）。
pub fn rename_version_dir(version_name: &str, new_name: &str) -> Result<(), String> {
    if !is_simple_name(version_name) || !is_simple_name(new_name) {
        bail_key!("errors.version.badDirName", "非法的版本目录名");
    }
    let versions_dir = active_minecraft_dir().join("versions");
    let from = versions_dir.join(version_name);
    let to = versions_dir.join(new_name);
    if !from.exists() {
        return Err(keyed_msg!(
            "errors.version.notFound?name={version_name}",
            "版本「{version_name}」不存在"
        ));
    }
    if to.exists() {
        return Err(keyed_msg!(
            "errors.version.alreadyExists?name={new_name}",
            "版本「{new_name}」已存在"
        ));
    }
    std::fs::rename(&from, &to).map_err(common_err_ctx_key!(
        "errors.version.renameFailed",
        "重命名失败"
    ))
}

/// 递归删除 versions/{name}；大目录耗时，故用 tokio::fs 避免阻塞主线程。
pub async fn delete_version_dir(version_name: &str) -> Result<(), String> {
    // 必须先校验目录名：`..`/分隔符经 `join` 会带出 versions/（`..` 即整删游戏数据），UI 输入不可信。
    if !is_simple_name(version_name) {
        bail_key!("errors.version.badDirName", "非法的版本目录名");
    }
    let dir = active_minecraft_dir().join("versions").join(version_name);
    if !dir.exists() {
        return Err(keyed_msg!(
            "errors.version.notFound?name={version_name}",
            "版本「{version_name}」不存在"
        ));
    }
    tokio::fs::remove_dir_all(&dir)
        .await
        .map_err(common_err_ctx_key!("errors.file.delete", "删除失败"))
}

/// 版本目录名：`versions/{id}` 已存在且含 client jar 时追加 `_{n}`；缺 jar（下载中断）则复用原目录。
pub fn resolve_version_dir(mc: &Path, version_id: &str) -> (String, PathBuf) {
    let base = mc.join("versions").join(version_id);
    if !base.exists() {
        return (version_id.to_string(), base);
    }
    if !base.join(format!("{version_id}.jar")).exists() {
        return (version_id.to_string(), base);
    }
    for n in 1.. {
        let name = format!("{}_{}", version_id, n);
        let dir = mc.join("versions").join(&name);
        if !dir.exists() {
            return (name, dir);
        }
    }
    // 循环必 return，此行仅为类型检查
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::{list_installed_in, version_server_section};
    use std::fs;

    #[test]
    fn version_server_section_rejects_unsafe_names() {
        // 空名 / 分隔符 / `..` 会污染 TOML 顶层 key
        assert!(version_server_section("").is_err());
        assert!(version_server_section("../evil").is_err());
        assert!(version_server_section("a/b").is_err());
        assert!(version_server_section("a\\b").is_err());
        // 真实版本目录名（含点与连字符）必须原样通过
        assert_eq!(
            version_server_section("1.20.1-fabric-0.15.0").unwrap(),
            "version_server_1.20.1-fabric-0.15.0"
        );
    }

    #[test]
    fn list_installed_cache_invalidates_on_json_change() {
        let dir = tempfile::tempdir().unwrap();
        let versions = dir.path().join("versions");
        for (name, body) in [
            ("1.20.1", r#"{"id":"1.20.1","type":"release"}"#),
            (
                "1.20.1-fabric-0.15.0",
                r#"{"id":"x","inheritsFrom":"net.fabricmc:fabric-loader"}"#,
            ),
        ] {
            let vdir = versions.join(name);
            fs::create_dir_all(&vdir).unwrap();
            fs::write(vdir.join(format!("{name}.json")), body).unwrap();
        }
        let first = list_installed_in(&versions).unwrap();
        assert_eq!(first[0].name, "1.20.1");
        assert_eq!(first[0].loader, "vanilla");
        assert_eq!(first[1].loader, "fabric");
        assert_eq!(list_installed_in(&versions).unwrap(), first);

        let json = versions.join("1.20.1").join("1.20.1.json");
        fs::write(&json, r#"{"inheritsFrom":"net.minecraftforge:forge"}"#).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&json)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
        let updated = list_installed_in(&versions).unwrap();
        assert_eq!(updated[0].loader, "forge");
        let _ = fs::remove_dir_all(dir);
    }
}
