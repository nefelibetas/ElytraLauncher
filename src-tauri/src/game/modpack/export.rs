//! 整合包导出（标准 MRPACK）与在线分发文件识别。

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::downloader::hash::{sha1_hex, sha512_hex};
use crate::game::community::modrinth_version_files;
use crate::game::version::dir::find_version_json;
use crate::util::{infer_loader_from_json, loader_from_dir, mc_version_from_dir};

// ── 导出侧（与上方导入解析对称）──

/// 导出阶段：扫描/哈希已存在文件 → 写入 zip。任务层据此切换分步进度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportPhase {
    Scan,
    Pack,
}

/// 导出进度观察者（`game` 层不依赖 task；任务层用原子计数实现并转成进度帧）。
pub(crate) trait ExportObserver: Sync {
    /// 已处理 `bytes` 字节；返回 false = 取消，调用方应尽快退出。
    fn on_bytes(&self, _bytes: u64) -> bool {
        true
    }

    /// 一个文件处理完毕（含跳过在线分发文件的引用计数）。
    fn on_file(&self) -> bool {
        true
    }

    /// 进入新阶段；返回 false = 取消。
    fn on_phase(&self, _phase: ExportPhase) -> bool {
        true
    }
}

/// 导出物理打包的内容目录（MRPACK / CurseForge 共用同一集合）。
pub(crate) const PACK_SUBDIRS: [&str; 5] = [
    "mods",
    "config",
    "shaderpacks",
    "resourcepacks",
    "datapacks",
];

const COPY_CHUNK: usize = 64 * 1024;

#[derive(serde::Serialize)]
pub(crate) struct HostedPackFile {
    path: String,
    hashes: serde_json::Value,
    downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    file_size: u64,
}

/// 扫描 mods/ 下压缩类文件 → sha1 → Modrinth version_files 匹配，返回 (files[] 条目, 需排除的文件名)；网络失败/无匹配则留在 overrides 物理打包（静默降级）。取消时返回空结果，任务层据取消令牌收口。
pub(crate) async fn resolve_hosted_mods_with(
    vdir: &Path,
    observer: &dyn ExportObserver,
) -> (Vec<HostedPackFile>, HashSet<String>) {
    let empty = || (Vec::new(), HashSet::new());
    if !observer.on_phase(ExportPhase::Scan) {
        return empty();
    }
    let mods_dir = vdir.join("mods");
    let mut candidates = mod_candidates(&mods_dir);
    if candidates.is_empty() {
        return empty();
    }
    let mut sha1s: Vec<String> = Vec::with_capacity(candidates.len());
    // 只保留哈希与大小：整个 mods 目录全部驻留内存会在大型整合包上撑爆内存。
    let mut kept: Vec<(PathBuf, String, String, u64)> = Vec::with_capacity(candidates.len());
    for path in candidates.drain(..) {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let size = data.len() as u64;
        if !observer.on_bytes(size) || !observer.on_file() {
            return empty();
        }
        let sha1 = sha1_hex(&data);
        let sha512 = sha512_hex(&data);
        kept.push((path, sha1.clone(), sha512, size));
        sha1s.push(sha1);
    }
    if kept.is_empty() {
        return empty();
    }
    let raw = match modrinth_version_files(&sha1s).await {
        Ok(v) => v,
        Err(_) => return empty(), // 离线降级
    };
    let mut hosted = Vec::new();
    let mut names = HashSet::new();
    for (path, sha1, sha512, size) in kept {
        let Some(hit) = raw.get(&sha1) else { continue };
        let Some(files) = hit.get("files").and_then(|f| f.as_array()) else {
            continue;
        };
        let Some(first) = files.first() else { continue };
        let Some(url) = first.get("url").and_then(|u| u.as_str()) else {
            continue;
        };
        let fname = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        hosted.push(HostedPackFile {
            path: format!("mods/{fname}"),
            hashes: serde_json::json!({
                "sha1": sha1,
                "sha512": sha512,
            }),
            downloads: vec![url.to_string()],
            file_size: size,
        });
        names.insert(fname);
    }
    (hosted, names)
}

/// mods/ 下参与在线匹配的压缩/资源类文件（与 PCL2 导出集合一致）。
pub(super) fn mod_candidates(mods_dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("jar" | "zip" | "rar" | "disabled" | "old")
                )
        })
        .collect();
    files.sort();
    files
}

/// 在线匹配阶段的分母：候选文件数与字节数。
pub(crate) fn mod_candidate_totals(vdir: &Path) -> (u32, u64) {
    let mut files = 0u32;
    let mut bytes = 0u64;
    for path in mod_candidates(&vdir.join("mods")) {
        let Ok(meta) = path.metadata() else {
            continue;
        };
        files += 1;
        bytes = bytes.saturating_add(meta.len());
    }
    (files, bytes)
}

/// 递归统计目录内文件数与字节数（导出进度分母）。
fn dir_totals(dir: &Path) -> (u32, u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let (mut files, mut bytes) = (0u32, 0u64);
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let (f, b) = dir_totals(&path);
            files = files.saturating_add(f);
            bytes = bytes.saturating_add(b);
        } else if path.is_file() {
            files += 1;
            if let Ok(meta) = entry.metadata() {
                bytes = bytes.saturating_add(meta.len());
            }
        }
    }
    (files, bytes)
}

/// 物理打包阶段的分母：所有内容目录的文件数与字节数（含会被在线引用跳过的文件，打包阶段照常计数）。
pub(crate) fn export_pack_totals(vdir: &Path) -> (u32, u64) {
    let (mut files, mut bytes) = (0u32, 0u64);
    for sub in PACK_SUBDIRS {
        let dir = vdir.join(sub);
        if dir.is_dir() {
            let (f, b) = dir_totals(&dir);
            files = files.saturating_add(f);
            bytes = bytes.saturating_add(b);
        }
    }
    (files, bytes)
}

/// 读取整合包安装标记里的真实 MC；旧标记无该字段时回查计划缓存。
fn pack_mc_version(vdir: &Path) -> Option<String> {
    let marker: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(vdir.join("elytra-pack.json")).ok()?).ok()?;
    if let Some(mc) = marker["mc_version"].as_str() {
        return Some(mc.to_string());
    }
    let key = marker["cache_key"].as_str()?;
    let plan: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(super::cache_dir().join(format!("{key}.plan.json"))).ok()?,
    )
    .ok()?;
    plan["mc_version"].as_str().map(str::to_string)
}

/// 权威版本元数据：真实 MC / loader kind / loader 版本号（None = 无 loader 或未知）。
/// 整合包标记优先（loader 合并后 json id 会变成目录名），其次 json `inheritsFrom`/`id`，最后目录名。
/// loader kind = json 推断 `infer_loader_from_json` → 目录名标记；版本号仅在两者同 kind 时取。
pub(crate) fn resolve_version_meta(
    vdir: &Path,
    version_name: &str,
) -> (String, String, Option<String>) {
    let dir_mc = mc_version_from_dir(version_name);
    let dir_loader = loader_from_dir(version_name);
    let pack_mc = pack_mc_version(vdir);
    let Some(text) =
        find_version_json(vdir, version_name).and_then(|p| std::fs::read_to_string(p).ok())
    else {
        // 无 json（异常目录）：目录名兜底
        let (kind, ver) = match dir_loader {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => ("vanilla".to_string(), None),
        };
        return (pack_mc.unwrap_or_else(|| dir_mc.to_string()), kind, ver);
    };
    let value: Option<serde_json::Value> = serde_json::from_str(&text).ok();
    let mc = pack_mc
        .or_else(|| {
            value
                .as_ref()
                .and_then(|v| v["inheritsFrom"].as_str())
                .map(str::to_string)
        })
        .or_else(|| {
            value
                .as_ref()
                .and_then(|v| v["id"].as_str())
                .map(|s| mc_version_from_dir(s).to_string())
        })
        .unwrap_or_else(|| dir_mc.to_string());
    let json_kind = infer_loader_from_json(&text);
    let (kind, ver) = if json_kind != "vanilla" {
        let ver = match dir_loader {
            Some((k, v)) if k == json_kind.as_str() => Some(v.to_string()),
            _ => None,
        };
        (json_kind, ver)
    } else if let Some((k, v)) = dir_loader {
        (k.to_string(), Some(v.to_string()))
    } else {
        ("vanilla".to_string(), None)
    };
    (mc, kind, ver)
}

/// 导出标准 MRPACK（v1）：内容物理打包进 `overrides/`，`modrinth.index.json` 声明版本、loader 依赖与 hosted files[]；存档（saves）不打包。取消时提前返回 Ok（任务层据取消令牌清理临时文件）。
pub(crate) fn pack_modpack_with(
    vdir: &Path,
    version_name: &str,
    save_path: &str,
    hosted: &[HostedPackFile],
    hosted_names: &HashSet<String>,
    observer: &dyn ExportObserver,
) -> Result<(), String> {
    if !observer.on_phase(ExportPhase::Pack) {
        return Ok(());
    }
    let file = std::fs::File::create(save_path)
        .map_err(common_err_ctx_key!("errors.file.write", "创建文件失败"))?;
    let mut writer = zip::ZipWriter::new(file);
    // mrpack 惯例 Deflated（config/资源包压缩收益明显）
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let (mc_version, kind, loader_version) = resolve_version_meta(vdir, version_name);
    let mut dependencies = serde_json::Map::new();
    dependencies.insert("minecraft".into(), serde_json::json!(mc_version));
    // loader 依赖键与导入侧 modrinth_loader 一致：fabric-loader / quilt-loader / forge / neoforge；版本号来自目录名约定，不明确时省略依赖而非写假值。
    if let Some(ver) = loader_version {
        let dep_key = match kind.as_str() {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            "forge" | "neoforge" => kind.as_str(),
            _ => "",
        };
        if !dep_key.is_empty() {
            dependencies.insert(dep_key.into(), serde_json::json!(ver));
        }
    }

    let manifest = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": version_name,
        "name": version_name,
        "summary": "",
        "files": hosted,
        "dependencies": dependencies,
    });
    let content = serde_json::to_vec_pretty(&manifest).map_err(common_err_ctx_key!(
        "errors.modpack.serializeFailed",
        "序列化失败"
    ))?;
    writer
        .start_file("modrinth.index.json", opts)
        .map_err(common_err_ctx_key!("errors.file.write", "写入清单失败"))?;
    writer
        .write_all(&content)
        .map_err(common_err_ctx_key!("errors.file.write", "写入清单失败"))?;

    let mut buf = vec![0u8; COPY_CHUNK];
    for sub in PACK_SUBDIRS {
        let dir = vdir.join(sub);
        if !dir.is_dir() {
            continue;
        }
        // 在线分发 mod 不物理打包进 overrides
        let skip = (sub == "mods").then_some(hosted_names);
        add_dir_to_zip(
            &mut writer,
            &dir,
            &format!("overrides/{sub}"),
            opts,
            skip,
            observer,
            &mut buf,
        )?;
    }
    writer
        .finish()
        .map_err(common_err_ctx_key!("errors.file.write", "写入 zip 失败"))?;
    Ok(())
}

/// 递归写目录到 zip（保留相对路径；目录条目本身也记录，空目录才可保留）。跳过/写入的文件都计入进度。
pub(super) fn add_dir_to_zip(
    writer: &mut zip::ZipWriter<std::fs::File>,
    dir: &Path,
    prefix: &str,
    opts: zip::write::SimpleFileOptions,
    skip_files: Option<&HashSet<String>>,
    observer: &dyn ExportObserver,
    buf: &mut [u8],
) -> Result<(), String> {
    // 空目录不会触发 on_file，进入目录时先探一次取消。
    if !observer.on_bytes(0) {
        return Ok(());
    }
    writer
        .add_directory(format!("{prefix}/"), opts)
        .map_err(common_err_ctx_key!("errors.file.write", "写入目录失败"))?;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = format!("{}/{}", prefix, e.file_name().to_string_lossy());
        // 顶层 hosted 文件跳过（在线分发，不物理打包），但按已处理计进度
        if path.is_file()
            && skip_files
                .is_some_and(|set| set.contains(&e.file_name().to_string_lossy().to_string()))
        {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if !observer.on_bytes(size) || !observer.on_file() {
                return Ok(());
            }
            continue;
        }
        if path.is_dir() {
            add_dir_to_zip(writer, &path, &name, opts, None, observer, buf)?;
        } else if path.is_file() {
            let mut f = std::fs::File::open(&path)
                .map_err(common_err_ctx_key!("errors.file.read", "打开文件失败"))?;
            writer
                .start_file(name, opts)
                .map_err(common_err_ctx_key!("errors.file.write", "写入文件失败"))?;
            loop {
                let n = f
                    .read(buf)
                    .map_err(common_err_ctx_key!("errors.file.read", "读取文件失败"))?;
                if n == 0 {
                    break;
                }
                writer
                    .write_all(&buf[..n])
                    .map_err(common_err_ctx_key!("errors.file.write", "写入文件失败"))?;
                if !observer.on_bytes(n as u64) {
                    return Ok(());
                }
            }
            if !observer.on_file() {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{resolve_version_meta, HostedPackFile};

    #[test]
    fn hosted_pack_file_serializes_spec_file_size() {
        let f = HostedPackFile {
            path: "mods/a.jar".into(),
            hashes: serde_json::json!({"sha1": "a", "sha512": "b"}),
            downloads: vec!["https://cdn.modrinth.com/a.jar".into()],
            file_size: 123,
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["fileSize"], 123);
        assert!(v.get("file_size").is_none(), "MRPACK 字段必须是 fileSize");
    }

    #[test]
    fn pack_marker_mc_wins_over_loader_merged_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("elytra-pack.json"),
            r#"{"cache_key":"k","mc_version":"1.20.1"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Pack-1.21.1.json"),
            r#"{"id":"Pack-1.21.1","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient"}"#,
        )
        .unwrap();

        let (mc, loader, _) = resolve_version_meta(dir.path(), "Pack-1.21.1");
        assert_eq!(mc, "1.20.1");
        assert_eq!(loader, "fabric");
    }
}
