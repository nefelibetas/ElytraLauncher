use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::downloader::{DownloadSource, DownloaderConfig};
use crate::downloader::batch::{self, BatchItem};
use crate::downloader::hash::{file_sha1_matches, sha1_hex};
use crate::downloader::progress::ProgressCounters;
use crate::downloader::reuse::{classify_local_artifact, LocalRuntimeSource};
use crate::game::version::source::CacheBehaviour;
use crate::util::sha1_sub_dir;

/// 资源下载失败止损阈值：超过即停（大量失败通常是源不可达，继续下发空耗；与 libraries 收齐错误语义相反）。
const MAX_ASSET_ERRORS: usize = 10;

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetsIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// 资源索引本地路径：内容匹配则复用旧版 `{id}.json`，否则按内容寻址 `{id}-{sha1}.json`。
/// 同一 id 可能被 Mojang 重发为不同内容（26.3 的 index 34 即如此），按 id 单槽会让多版本反复覆盖。
pub fn index_cache_path(mc_dir: &Path, id: &str, sha1: &str) -> PathBuf {
    let dir = mc_dir.join("assets").join("indexes");
    if sha1.is_empty() {
        return dir.join(format!("{id}.json"));
    }
    let legacy = dir.join(format!("{id}.json"));
    if file_sha1_matches(&legacy, sha1) {
        legacy
    } else {
        dir.join(format!("{id}-{sha1}.json"))
    }
}

/// 读取本地索引缓存（sha1 为空时只看存在）；不发起网络。
pub fn read_cached_index(mc_dir: &Path, id: &str, sha1: &str) -> Option<String> {
    let data = std::fs::read(index_cache_path(mc_dir, id, sha1)).ok()?;
    if !sha1.is_empty() && sha1_hex(&data) != sha1 {
        return None;
    }
    String::from_utf8(data).ok()
}

/// 拉取资源索引文本并按 sha1 校验（为空则跳过校验）；不读写本地缓存。
pub async fn fetch_index_text(
    config: &DownloaderConfig,
    index_url: &str,
    index_sha1: &str,
) -> Result<String, String> {
    // 主源过慢（4s）自动降级到备用源
    let text = config
        .fetch_with_fallback(config.game_source_mode, CacheBehaviour::DefaultTtl, |s| {
            s.mirror_url(index_url)
        })
        .await
        .map_err(common_err_ctx_key!(
            "errors.download.fetch",
            "获取资源索引失败"
        ))?;
    if !index_sha1.is_empty() && sha1_hex(text.as_bytes()) != index_sha1 {
        bail_key!(
            "errors.assets.indexCorrupt",
            "资源索引内容校验失败（sha1 不匹配，源返回内容已损坏）\n{index_url}"
        );
    }
    Ok(text)
}

/// 拉取并缓存资源索引（本地缓存 sha1 匹配则直接读）；`index_id` 为 None 时只拉取不读不写缓存。
pub async fn fetch_asset_index(
    config: &DownloaderConfig,
    mc_dir: &Path,
    index_url: &str,
    index_id: Option<&str>,
    index_sha1: Option<&str>,
) -> Result<String, String> {
    let sha1 = index_sha1.unwrap_or("");
    if let Some(id) = index_id {
        if let Some(text) = read_cached_index(mc_dir, id, sha1) {
            eprintln!(
                "[assets] 缓存命中: {}",
                index_cache_path(mc_dir, id, sha1).display()
            );
            return Ok(text);
        }
    }

    eprintln!("[assets] 无有效缓存，走网络 fetch_with_fallback");
    let text = fetch_index_text(config, index_url, sha1).await?;
    eprintln!("[assets] 索引文本获取完成: {}B", text.len());

    if let Some(id) = index_id {
        let path = index_cache_path(mc_dir, id, sha1);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(common_err_ctx_key!(
                "errors.file.createDir",
                "创建索引目录失败"
            ))?;
        }
        std::fs::write(&path, &text)
            .map_err(common_err_ctx_key!("errors.file.write", "写入资源索引失败"))?;
    }
    Ok(text)
}

/// 资源索引定位（URL + 版本 id + 期望 sha1）；id/sha1 为 None 表示只关心内容（缓存以 URL 为键）。
#[derive(Clone, Copy)]
pub struct AssetIndexRef<'a> {
    pub url: &'a str,
    pub id: Option<&'a str>,
    pub sha1: Option<&'a str>,
}

/// 批量下载资源文件（objects/{sub}/{hash}，逐文件 sha1 校验）。
/// `reuse`：dest 缺失且复用根有 size+sha1 匹配的同 hash 文件时本地复制（计入进度）而非网络下载。
pub async fn download_assets(
    index: AssetIndexRef<'_>,
    config: &DownloaderConfig,
    concurrency: usize,
    reuse: Option<LocalRuntimeSource>,
    pc: &ProgressCounters,
    cancel: Option<&Arc<std::sync::atomic::AtomicBool>>,
) -> Result<(), Vec<String>> {
    let AssetIndexRef {
        url: index_url,
        id: index_id,
        sha1: index_sha1,
    } = index;
    let index_id = index_id.ok_or_else(|| {
        vec![keyed_msg!(
            "errors.assets.indexIdMissing",
            "资源索引缺少版本 id"
        )]
    })?;
    let index_sha1 = index_sha1.ok_or_else(|| {
        vec![keyed_msg!(
            "errors.assets.indexSha1Missing",
            "资源索引缺少 sha1"
        )]
    })?;
    let sources: Vec<&DownloadSource> = config.file_sources_list();
    let mc_dir = config.minecraft_dir_abs();

    let index_text =
        fetch_asset_index(config, &mc_dir, index_url, Some(index_id), Some(index_sha1))
            .await
            .map_err(|e| vec![e])?;

    let index: AssetsIndex =
        serde_json::from_str(&index_text).map_err(|e| vec![format!("解析资源索引失败\n{}", e)])?;

    let objects_dir = mc_dir.join("assets").join("objects");

    // 已存在且 sha1 匹配的跳过；其余交给统一批处理（本地复用优先、网络降级）
    let mut items = Vec::new();
    for obj in index.objects.values() {
        let hash = obj.hash.clone();
        let obj_size = obj.size;
        let sub = sha1_sub_dir(&hash);
        let dest = objects_dir.join(sub).join(&hash);
        if dest.exists() && file_sha1_matches(&dest, &hash) {
            continue;
        }
        // 本地复用：dest 缺失时先问复用根（hash 即 sha1，size + 流式 sha1 校验）；命中复制，未命中走网络。
        let local = reuse.as_ref().and_then(|r| {
            classify_local_artifact(
                r,
                &format!("assets/objects/{}/{}", sub, hash),
                Some(obj_size),
                Some(&hash),
            )
        });
        let urls: Vec<String> = sources
            .iter()
            .map(|s| format!("{}/{}/{}", s.resource_url, sub, hash))
            .collect();
        items.push(BatchItem {
            urls,
            dest,
            sha1: hash.clone(),
            size: obj_size,
            local,
            label: hash,
        });
    }

    // 步骤进度分母（待下载条目数）
    pc.step_files_total
        .store(items.len() as u32, Ordering::Relaxed);
    let errors = batch::run(items, concurrency, pc, cancel, Some(MAX_ASSET_ERRORS)).await;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::downloader::DownloaderConfig;

    /// 索引按内容寻址：同一 id 的不同内容落到不同文件，损坏缓存不被命中。
    #[test]
    fn index_cache_is_content_addressed() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path();
        let body = br#"{"objects":{}}"#;
        let sha1 = sha1_hex(body);

        let path = index_cache_path(mc, "34", &sha1);
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("34-{sha1}.json")
        );
        assert_eq!(
            index_cache_path(mc, "34", "").file_name().unwrap(),
            "34.json"
        );

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        assert!(
            read_cached_index(mc, "34", &sha1).is_some(),
            "内容匹配应命中缓存"
        );

        std::fs::write(&path, b"corrupt").unwrap();
        assert!(
            read_cached_index(mc, "34", &sha1).is_none(),
            "sha1 不匹配不应命中缓存"
        );

        // 旧版按 id 单槽的缓存内容匹配时直接复用（升级后无需重下索引）
        let legacy = mc.join("assets").join("indexes").join("34.json");
        std::fs::write(&legacy, body).unwrap();
        assert_eq!(index_cache_path(mc, "34", &sha1), legacy);
        assert!(read_cached_index(mc, "34", &sha1).is_some());
    }

    /// 端到端：fetch_asset_index 经代理/直连拉官方资源索引应成功；离线/黑洞环境超时报错。
    #[tokio::test]
    async fn fetch_asset_index_end_to_end() {
        let cfg = DownloaderConfig::default();
        let mc = std::env::temp_dir().join(format!("elytra_assets_e2e_{}", std::process::id()));
        let url =
            "https://piston-meta.mojang.com/v1/packages/cf75b185cb35b32e299b0c8e674fa202d7911a3c/32.json";
        let sha1 = "cf75b185cb35b32e299b0c8e674fa202d7911a3c";
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            fetch_asset_index(&cfg, &mc, url, Some("32"), Some(sha1)),
        )
        .await;
        let text = match res {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => panic!("asset index 获取失败: {e}"),
            Err(_) => panic!("asset index 20s 超时（网络或代理不可达）"),
        };
        assert!(
            text.len() > 400_000,
            "asset index 应完整（约 586KB）: {}B",
            text.len()
        );
        let _ = std::fs::remove_dir_all(&mc);
    }
}
