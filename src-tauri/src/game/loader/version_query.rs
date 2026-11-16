// ── ModLoader 版本查询：maven-metadata / meta JSON 解析 + 语义化排序 ──
// 版本列表查询域（commands/loader.rs 只留命令壳）。URL 模板来自 DownloaderConfig 多源配置。

use crate::config::downloader::{DownloadSource, DownloadSourceMode, DownloaderConfig};
use crate::downloader::{endpoints, fetcher};
use crate::game::version::source;
use crate::persist::downloader_config;
use serde::Deserialize;
use std::cmp::Ordering;
use std::time::Duration;

#[derive(Deserialize)]
struct FabricEntry {
    loader: FabricLoader,
}

#[derive(Deserialize)]
struct FabricLoader {
    version: String,
}

#[derive(Deserialize)]
struct QuiltEntry {
    loader: QuiltLoader,
}

#[derive(Deserialize)]
struct QuiltLoader {
    version: String,
}

#[derive(Deserialize)]
struct QuiltAllEntry {
    version: String,
}

#[derive(Deserialize)]
struct OptiFineEntry {
    patch: String,
    filename: String,
}

fn config() -> DownloaderConfig {
    downloader_config()
}

async fn fetch_versions<T: serde::de::DeserializeOwned>(
    make_url: impl Fn(&DownloadSource) -> String,
) -> Result<Vec<T>, String> {
    let cfg = config();
    cfg.fetch_json_with_fallback::<Vec<T>>(
        cfg.game_source_mode,
        source::CacheBehaviour::DefaultTtl,
        make_url,
    )
    .await
}

/// 拉取 maven-metadata.xml 版本列表（多源回退）。
async fn fetch_maven_versions(
    make_url: impl Fn(&DownloadSource) -> String,
) -> Result<Vec<String>, String> {
    let cfg = config();
    let xml = cfg
        .fetch_with_fallback(
            cfg.game_source_mode,
            source::CacheBehaviour::DefaultTtl,
            make_url,
        )
        .await?;
    Ok(parse_maven_versions(&xml))
}

/// 从 maven-metadata.xml 提取全部 `<version>...</version>`；官方 maven 与 BMCLAPI /maven/ 代理同格式，共用一套解析。
fn parse_maven_versions(xml: &str) -> Vec<String> {
    xml.split("<version>")
        .skip(1)
        .filter_map(|s| s.split("</version>").next().map(|v| v.to_string()))
        .collect()
}

/// 分离主版本号与预发布后缀（第一个 '-' 后），如 "0.20.0-beta.9" → ("0.20.0", "-beta.9")。
fn split_pre(v: &str) -> (String, String) {
    match v.find('-') {
        Some(i) => (v[..i].to_string(), v[i..].to_string()),
        None => (v.to_string(), String::new()),
    }
}

/// 语义化版本升序比较："0.20.0" < "0.20.2" < "0.24.0"；预发布（alpha < beta < rc）< 正式。
fn cmp_version_asc(a: &str, b: &str) -> std::cmp::Ordering {
    let (ma, pa) = split_pre(a);
    let (mb, pb) = split_pre(b);
    let nums = |s: &str| -> Vec<u64> { s.split('.').filter_map(|x| x.parse().ok()).collect() };
    let (na, nb) = (nums(&ma), nums(&mb));
    for i in 0..na.len().max(nb.len()) {
        let (va, vb) = (
            na.get(i).copied().unwrap_or(0),
            nb.get(i).copied().unwrap_or(0),
        );
        if va != vb {
            return va.cmp(&vb);
        }
    }
    let rank = |p: &str| -> u8 {
        if p.is_empty() {
            4
        } else if p.contains("alpha") {
            0
        } else if p.contains("beta") {
            1
        } else if p.contains("rc") || p.contains("pre") {
            2
        } else {
            3
        }
    };
    match rank(&pa).cmp(&rank(&pb)) {
        Ordering::Equal => pa.cmp(&pb),
        o => o,
    }
}

/// 语义化版本降序排序（最新在前）："0.24.0" > "0.20.2-beta.1" > "0.20.0-beta.9"；不依赖源顺序（Quilt meta、OptiFine pre 可能乱序）。
fn sort_versions_desc(mut versions: Vec<String>) -> Vec<String> {
    versions.sort_by(|a, b| cmp_version_asc(b, a));
    versions
}

/// OptiFine patch 升序比较：preview（pre 前缀）低于正式版（字母前缀）；
/// 同级按前缀字母序 + 尾部数字数值序（"pre9" < "pre10"、"G8" < "J1"）。
fn cmp_optifine_asc(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> (String, u64) {
        let i = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
        (s[..i].to_string(), s[i..].parse().unwrap_or(0))
    };
    let (pa, na) = parse(a);
    let (pb, nb) = parse(b);
    let rank = |p: &str| -> u8 {
        if p.starts_with("pre") {
            0 // 预览版
        } else {
            1 // 正式版
        }
    };
    rank(&pa)
        .cmp(&rank(&pb))
        .then_with(|| pa.cmp(&pb))
        .then_with(|| na.cmp(&nb))
}

/// OptiFine patch 降序：正式版（J1）在前，preview 系列按数值降序（pre15 > pre9）。
fn sort_optifine_desc(mut versions: Vec<String>) -> Vec<String> {
    versions.sort_by(|a, b| cmp_optifine_asc(b, a));
    versions
}

/// Forge 版本列表：maven-metadata 版本号是 "{mc}-{ver}" 全名（如 "26.2-65.1.0"），
/// 过滤出当前 MC 并剥掉前缀，返回纯 ver；语义化降序（最新在前）。
pub async fn forge_versions(mc_version: &str) -> Result<Vec<String>, String> {
    let prefix = format!("{}-", mc_version);
    let versions: Vec<String> = fetch_maven_versions(|s| s.forge_list_url.clone())
        .await?
        .into_iter()
        .filter_map(|v| v.strip_prefix(&prefix).map(|s| s.to_string()))
        .collect();
    Ok(sort_versions_desc(versions))
}

pub async fn fabric_versions(mc_version: &str) -> Result<Vec<String>, String> {
    let data =
        fetch_versions::<FabricEntry>(|s| s.url(&s.fabric_list_url, &[("mc", mc_version)])).await?;
    Ok(sort_versions_desc(
        data.into_iter().map(|e| e.loader.version).collect(),
    ))
}

pub async fn quilt_versions(mc_version: &str) -> Result<Vec<String>, String> {
    // Quilt meta 实测乱序（0.20.0-beta.7 排在 beta.8 前、0.24.0 混在中间），必须显式排序
    match fetch_versions::<QuiltEntry>(|s| s.url(&s.quilt_list_url, &[("mc", mc_version)])).await {
        Ok(data) => Ok(sort_versions_desc(
            data.into_iter().map(|e| e.loader.version).collect(),
        )),
        Err(primary) => {
            eprintln!("[loader] Quilt 按 MC 查询失败，回退全量列表: {primary}");
            match fetch_quilt_all_versions().await {
                Ok(versions) => Ok(sort_versions_desc(versions)),
                Err(fallback) => Err(keyed_msg!(
                    "errors.loader.versionList",
                    "获取 Quilt 版本列表失败\n按 MC 查询: {}\n全量列表: {}",
                    primary,
                    fallback
                )),
            }
        }
    }
}

/// Quilt 全量 loader 版本：BMCLAPI / 官方全量列表 → Quilt maven-metadata。
/// 按 MC 查询端点失败时使用；列表不按 MC 过滤，兼容性由 loader 自身保证。
async fn fetch_quilt_all_versions() -> Result<Vec<String>, String> {
    let urls = [
        format!(
            "{}{}",
            endpoints::BMCLAPI_QUILT_META,
            endpoints::QUILT_ALL_LOADER_PATH
        ),
        format!(
            "{}{}",
            endpoints::QUILT_META,
            endpoints::QUILT_ALL_LOADER_PATH
        ),
    ];
    let mut last_err = String::new();
    for url in urls {
        let req = fetcher::http().get(&url).timeout(Duration::from_secs(8));
        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(e) => {
                last_err = format!("{url} 请求失败: {e}");
                continue;
            }
        };
        if !resp.status().is_success() {
            last_err = format!("{url} HTTP {}", resp.status());
            continue;
        }
        match resp.json::<Vec<QuiltAllEntry>>().await {
            Ok(list) => {
                let mut versions: Vec<String> = list
                    .into_iter()
                    .map(|entry| entry.version)
                    .filter(|version| !version.is_empty())
                    .collect();
                versions.sort();
                versions.dedup();
                if !versions.is_empty() {
                    return Ok(versions);
                }
                last_err = format!("{url} 返回空版本列表");
            }
            Err(e) => last_err = format!("{url} 解析失败: {e}"),
        }
    }

    match fetcher::http()
        .get(endpoints::QUILT_MAVEN_METADATA)
        .timeout(Duration::from_secs(12))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(text) => {
                let versions = parse_maven_versions(&text);
                if !versions.is_empty() {
                    return Ok(versions);
                }
                last_err = format!("Quilt maven 返回空版本列表\n{last_err}");
            }
            Err(e) => last_err = format!("Quilt maven 读取失败: {e}\n{last_err}"),
        },
        Ok(resp) => last_err = format!("Quilt maven HTTP {}\n{last_err}", resp.status()),
        Err(e) => last_err = format!("Quilt maven 请求失败: {e}\n{last_err}"),
    }
    Err(last_err)
}

/// NeoForge 版本列表：版本号内嵌 MC 版本（"26.2" → "26.2.x"）；maven-metadata 里的快照自然排除。
pub async fn neoforge_versions(mc_version: &str) -> Result<Vec<String>, String> {
    let prefix = format!("{}.", mc_version.strip_prefix("1.").unwrap_or(mc_version));
    let versions: Vec<String> = fetch_maven_versions(|s| s.neoforge_list_url.clone())
        .await?
        .into_iter()
        .filter(|v| v.starts_with(&prefix))
        .collect();
    Ok(sort_versions_desc(versions))
}

pub async fn optifine_versions(mc_version: &str) -> Result<Vec<String>, String> {
    let data =
        fetch_versions::<OptiFineEntry>(|s| s.url(&s.optifine_list_url, &[("mc", mc_version)]))
            .await?;
    Ok(sort_optifine_desc(
        data.into_iter()
            .filter(|e| !e.filename.starts_with("preview_"))
            .map(|e| e.patch)
            .collect(),
    ))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FabricApiVersion {
    pub version: String,
    pub download_url: String,
    pub filename: String,
}

/// Fabric API 版本列表（Modrinth 官方 vs MCIM 镜像，按 community_source_mode 选源；主源超 5s 降级备用源）。
/// 不复用 `fetch_json_with_fallback`（后者按 `game_source_mode` 在文件源上路由）；此处走社区源、URL 为固定域前缀拼接，故手写双源回退。
pub async fn fabric_api_versions(mc_version: &str) -> Result<Vec<FabricApiVersion>, String> {
    let path = format!(
        "/v2/project/fabric-api/version?loaders=[\"fabric\"]&game_versions=[\"{}\"]",
        mc_version
    );
    let cfg = config();
    let official = format!("{}{path}", endpoints::CommunityEndpoints::MODRINTH_API_BASE);
    let china = format!(
        "{}{path}",
        endpoints::CommunityEndpoints::MODRINTH_MIRROR_BASE
    );
    let urls = match cfg.community_source_mode {
        DownloadSourceMode::OfficialOnly => vec![official],
        DownloadSourceMode::MirrorOnly => vec![china],
        // Auto：官方优先，过慢（5s）降级镜像（自适应语义同元数据请求）
        DownloadSourceMode::Auto => vec![official, china],
    };
    let mut last_err = String::new();
    for (i, url) in urls.iter().enumerate() {
        let mut req = fetcher::http().get(url);
        // 主源过慢（5s）即降级备用源，不等待失败
        if i == 0 {
            req = req.timeout(std::time::Duration::from_secs(5));
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("请求 Fabric API 版本失败: {}", e);
                continue;
            }
        };
        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                last_err = format!("解析版本列表失败: {}", e);
                continue;
            }
        };
        let arr = match data.as_array() {
            Some(a) => a,
            None => {
                last_err = "版本列表格式错误".into();
                continue;
            }
        };
        let versions: Vec<FabricApiVersion> = arr
            .iter()
            .filter_map(|v| {
                let version = v["version_number"].as_str()?.to_string();
                let file = v["files"].as_array()?.first()?;
                let download_url = file["url"].as_str()?.to_string();
                let filename = file["filename"].as_str()?.to_string();
                Some(FabricApiVersion {
                    version,
                    download_url,
                    filename,
                })
            })
            .collect();
        return Ok(versions);
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_maven_versions_extracts_all_versions() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.minecraftforge</groupId>
  <artifactId>forge</artifactId>
  <versioning>
    <latest>1.21.11-61.2.0</latest>
    <release>1.21.11-61.2.0</release>
    <versions>
      <version>1.20.1-47.1.0</version>
      <version>1.21-51.0.0</version>
      <version>1.21.11-61.2.0</version>
    </versions>
  </versioning>
</metadata>"#;
        assert_eq!(
            parse_maven_versions(xml),
            vec![
                "1.20.1-47.1.0".to_string(),
                "1.21-51.0.0".to_string(),
                "1.21.11-61.2.0".to_string()
            ]
        );
    }

    #[test]
    fn parse_maven_versions_empty_when_no_versions() {
        assert!(parse_maven_versions("<metadata></metadata>").is_empty());
    }

    /// Quilt meta 实测乱序样例：排序后必须最新在前且数字语义正确
    /// （0.24.0 > 0.20.2-beta.1 > 0.20.0-beta.9 > 0.20.0-beta.1 > 0.17.0-beta.3）。
    #[test]
    fn sort_versions_desc_semantic_order() {
        let input = vec![
            "0.20.0-beta.1".to_string(),
            "0.20.0-beta.9".to_string(),
            "0.24.0".to_string(),
            "0.20.2-beta.1".to_string(),
            "0.17.0-beta.3".to_string(),
            "0.20.0-beta.7".to_string(),
        ];
        assert_eq!(
            sort_versions_desc(input),
            vec![
                "0.24.0",
                "0.20.2-beta.1",
                "0.20.0-beta.9",
                "0.20.0-beta.7",
                "0.20.0-beta.1",
                "0.17.0-beta.3",
            ]
        );
        // 多位数段：0.10 > 0.9
        assert_eq!(
            sort_versions_desc(vec!["0.9".into(), "0.10".into()]),
            vec!["0.10", "0.9"]
        );
    }

    /// OptiFine pre 系列按数值排序：pre15 > pre14 > pre10 > pre9，正式版（J1）在前。
    #[test]
    fn sort_optifine_desc_numeric() {
        assert_eq!(
            sort_optifine_desc(vec![
                "pre9".into(),
                "J1".into(),
                "pre15".into(),
                "pre10".into()
            ]),
            vec!["J1", "pre15", "pre10", "pre9"]
        );
    }

    #[test]
    fn mirror_source_forge_metadata_uses_bmclapi() {
        assert!(
            endpoints::mirror_source()
                .forge_list_url
                .starts_with(endpoints::BMCLAPI_MAVEN),
            "Forge 元数据必须能回退到 BMCLAPI /maven"
        );
    }

    #[tokio::test]
    #[ignore = "真实网络：Forge 版本列表"]
    async fn forge_versions_network() {
        let versions = forge_versions("1.20.1").await.expect("Forge 版本列表");
        assert!(
            versions.iter().any(|version| version.starts_with("47.")),
            "应返回 1.20.1 的 Forge 47.x 版本: {versions:?}"
        );
    }

    /// 真实网络：按 MC 查询失败时的 Quilt 全量版本兜底（BMCLAPI / 官方 / maven）。
    #[tokio::test]
    #[ignore = "真实网络：Quilt 全量版本兜底"]
    async fn quilt_all_versions_fallback_network() {
        let versions = fetch_quilt_all_versions()
            .await
            .expect("Quilt 全量版本列表");
        assert!(
            versions.len() > 10,
            "应返回可用的 Quilt loader 版本: {versions:?}"
        );
    }
}
