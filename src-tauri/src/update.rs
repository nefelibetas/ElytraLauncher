//! 启动器自身更新：读取 GitHub Releases 的 latest，返回版本、说明与可下载文件清单；
//! 下载由前端交给工具箱下载器（`tooldl`）执行，本模块不落盘、不运行安装包。

use elytra_domain::error::CoreError;
use std::time::Duration;

use serde::Serialize;

use crate::constants::APP_VERSION;
use elytra_domain::downloader::fetcher::http;
use elytra_domain::persist;
use elytra_domain::util::downloads_dir;

const REPO: &str = "nefelibetas/ElytraLauncher";
const RELEASE_MANIFEST_URL: &str =
    "https://github.com/nefelibetas/ElytraLauncher/releases/latest/download/release.json";

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    pub current_version: &'static str,
    pub latest_version: String,
    pub update_available: bool,
    pub url: String,
    pub notes: String,
    pub published_at: Option<String>,
}

/// Release 中的一个可下载文件。
#[derive(Debug, Clone, Serialize)]
pub struct LauncherUpdateAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// windows / macos / linux / other
    pub platform: String,
    /// x64 / arm64 / universal / x86 / other
    pub arch: String,
    /// setup / msi / portable / dmg / appimage / deb / rpm / archive / other
    pub kind: String,
    /// 当前平台+架构下的首选文件
    pub recommended: bool,
}

/// 下载更新对话框的完整数据：平台信息、文件清单与下载目录。
#[derive(Debug, Clone, Serialize)]
pub struct LauncherUpdatePlan {
    pub version: String,
    pub release_url: String,
    pub platform: String,
    pub arch: String,
    /// 生效目录（配置值或平台默认值）
    pub download_dir: String,
    /// 平台默认目录（用于「恢复默认」）
    pub default_download_dir: String,
    pub assets: Vec<LauncherUpdateAsset>,
}

#[derive(serde::Deserialize)]
struct ReleaseManifest {
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    assets: Vec<ManifestAsset>,
}

#[derive(serde::Deserialize)]
struct ManifestAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    size: u64,
}

/// 查询最新正式 Release：优先取发布产物中的静态 release.json；缺失/损坏时回退 GitHub API。
pub async fn check() -> Result<Option<UpdateInfo>, CoreError> {
    if let Ok(Some(info)) = check_static_manifest().await {
        return Ok(Some(info));
    }
    check_github_api().await
}

async fn check_static_manifest() -> Result<Option<UpdateInfo>, CoreError> {
    let Some(manifest) = fetch_static_manifest().await? else {
        return Ok(None);
    };
    Ok(Some(manifest_to_info(manifest)?))
}

async fn fetch_static_manifest() -> Result<Option<ReleaseManifest>, CoreError> {
    let response = http()
        .get(RELEASE_MANIFEST_URL)
        .send()
        .await
        .map_err(common_err_ctx_key!("errors.update.failed", "检查更新失败"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let manifest: ReleaseManifest = response
        .error_for_status()
        .map_err(common_err_ctx_key!("errors.update.failed", "检查更新失败"))?
        .json()
        .await
        .map_err(common_err_ctx_key!("errors.update.failed", "检查更新失败"))?;
    Ok(Some(manifest))
}

fn manifest_to_info(manifest: ReleaseManifest) -> Result<UpdateInfo, CoreError> {
    let latest_version = normalize_version(&manifest.version);
    if latest_version.is_empty() {
        return Err(err_keyed!(
            "errors.update.failed",
            "检查更新失败\n发布信息缺少版本号"
        ));
    }
    let url = if manifest.url.is_empty() {
        format!("https://github.com/{REPO}/releases/tag/v{latest_version}")
    } else {
        manifest.url
    };
    Ok(UpdateInfo {
        current_version: APP_VERSION,
        update_available: is_newer(&latest_version, APP_VERSION),
        latest_version,
        url,
        notes: manifest.notes,
        published_at: manifest.published_at,
    })
}

/// 下载更新对话框数据：release.json 文件清单（主）+ GitHub API（清单缺失时兜底）+ 产物命名兜底。
pub async fn download_plan() -> Result<LauncherUpdatePlan, CoreError> {
    let (version, release_url, mut assets) =
        if let Some(manifest) = fetch_static_manifest().await.ok().flatten() {
            let ReleaseManifest {
                version: raw_version,
                url,
                assets,
                ..
            } = manifest;
            let version = normalize_version(&raw_version);
            if version.is_empty() {
                bail_key!("errors.update.failed", "检查更新失败\n发布信息缺少版本号");
            }
            let release_url = if url.is_empty() {
                format!("https://github.com/{REPO}/releases/tag/v{version}")
            } else {
                url
            };
            let mut assets: Vec<LauncherUpdateAsset> = assets
                .into_iter()
                .filter_map(|asset| {
                    let url = canonical_asset_url(&asset.url, &version);
                    build_asset(asset.name, url, asset.size)
                })
                .collect();
            if assets.is_empty() {
                assets = fallback_assets(&version);
            }
            (version, release_url, assets)
        } else {
            // 旧版 release.json 缺失：回退 API（限流/断网时展示打开 Release 页的入口）
            let api = tokio::time::timeout(Duration::from_secs(4), fetch_latest_release())
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .ok_or_else(|| err_keyed!("errors.update.noRelease", "未找到可下载的发布版本"))?;
            let version = api
                .get("tag_name")
                .and_then(|v| v.as_str())
                .map(normalize_version)
                .filter(|version| !version.is_empty())
                .ok_or_else(|| {
                    keyed_msg!("errors.update.failed", "检查更新失败\n发布信息缺少版本号")
                })?;
            let release_url = api
                .get("html_url")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("https://github.com/{REPO}/releases/tag/v{version}"));
            let mut assets = assets_from_json(&api);
            if assets.is_empty() {
                assets = fallback_assets(&version);
            }
            (version, release_url, assets)
        };

    let platform = current_platform().to_string();
    let arch = current_arch().to_string();
    sort_assets(&mut assets, &platform, &arch);
    if let Some(best) = assets.first_mut() {
        best.recommended = best.platform == platform;
    }

    let config = persist::launcher_update_config();
    let default_download_dir = downloads_dir().to_string_lossy().into_owned();
    let configured = config.download_dir.trim();
    let download_dir = if configured.is_empty() {
        default_download_dir.clone()
    } else {
        configured.to_string()
    };
    Ok(LauncherUpdatePlan {
        version,
        release_url,
        platform,
        arch,
        download_dir,
        default_download_dir,
        assets,
    })
}

fn assets_from_json(json: &serde_json::Value) -> Vec<LauncherUpdateAsset> {
    json.get("assets")
        .and_then(|v| v.as_array())
        .map(|list| list.iter().filter_map(parse_asset).collect())
        .unwrap_or_default()
}

/// GitHub API 不可用（限流/断网）时的兜底文件清单：名称与 `.github/workflows/release.yml` 同步，
/// 大小未知（0），下载地址按 Releases 固定规则拼接。
fn fallback_assets(version: &str) -> Vec<LauncherUpdateAsset> {
    let base = format!("https://github.com/{REPO}/releases/download/v{version}");
    [
        format!("Elytra-Launcher-{version}-windows-x64-setup.exe"),
        format!("Elytra-Launcher-{version}-windows-x64.msi"),
        format!("Elytra-Launcher-{version}-windows-x64-portable.zip"),
        format!("Elytra-Launcher-{version}-macos-universal.dmg"),
        format!("Elytra-Launcher-{version}-x86_64.AppImage"),
        format!("Elytra-Launcher-{version}-x86_64.deb"),
        format!("Elytra-Launcher-{version}-x86_64.rpm"),
        format!("Elytra-Launcher-{version}-linux-x86_64-portable.tar.gz"),
    ]
    .into_iter()
    .map(|name| LauncherUpdateAsset {
        url: format!("{base}/{name}"),
        platform: asset_platform(&name).to_string(),
        arch: asset_arch(&name).to_string(),
        kind: asset_kind(&name).to_string(),
        name,
        size: 0,
        recommended: false,
    })
    .collect()
}

/// 草稿 Release 的产物链接形如 `/releases/download/untagged-<hash>/<name>`，发布后该段 404；
/// 按发布版本改写成规范 tag 路径（发布工作流保证 tag = `v{version}`）。
fn canonical_asset_url(url: &str, version: &str) -> String {
    let Some((head, rest)) = url.split_once("/releases/download/") else {
        return url.to_string();
    };
    let Some((tag, name)) = rest.split_once('/') else {
        return url.to_string();
    };
    if !tag.starts_with("untagged-") {
        return url.to_string();
    }
    format!("{head}/releases/download/v{version}/{name}")
}

fn build_asset(name: String, url: String, size: u64) -> Option<LauncherUpdateAsset> {
    if !url.starts_with("https://") {
        return None;
    }
    let platform = asset_platform(&name).to_string();
    let kind = asset_kind(&name).to_string();
    // release.json / 校验文件等平台、类型都无法识别的条目不进列表
    if platform == "other" && kind == "other" {
        return None;
    }
    Some(LauncherUpdateAsset {
        arch: asset_arch(&name).to_string(),
        name,
        url,
        size,
        platform,
        kind,
        recommended: false,
    })
}

fn parse_asset(value: &serde_json::Value) -> Option<LauncherUpdateAsset> {
    build_asset(
        value.get("name")?.as_str()?.to_string(),
        value.get("browser_download_url")?.as_str()?.to_string(),
        value.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
    )
}

fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        _ => "other",
    }
}

fn current_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "other",
    }
}

fn asset_platform(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.contains("windows") || name.ends_with(".exe") || name.ends_with(".msi") {
        "windows"
    } else if name.contains("macos")
        || name.contains("darwin")
        || name.contains("osx")
        || name.ends_with(".dmg")
    {
        "macos"
    } else if name.contains("linux")
        || name.ends_with(".appimage")
        || name.ends_with(".deb")
        || name.ends_with(".rpm")
    {
        "linux"
    } else {
        "other"
    }
}

fn asset_arch(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.contains("universal") {
        "universal"
    } else if name.contains("arm64") || name.contains("aarch64") {
        "arm64"
    } else if name.contains("x64") || name.contains("x86_64") || name.contains("amd64") {
        "x64"
    } else if name.contains("x86") || name.contains("i686") {
        "x86"
    } else {
        "other"
    }
}

fn asset_kind(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".exe") {
        "setup"
    } else if name.ends_with(".msi") {
        "msi"
    } else if name.ends_with(".dmg") {
        "dmg"
    } else if name.ends_with(".appimage") {
        "appimage"
    } else if name.ends_with(".deb") {
        "deb"
    } else if name.ends_with(".rpm") {
        "rpm"
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        "archive"
    } else if name.ends_with(".zip") {
        if name.contains("portable") {
            "portable"
        } else {
            "zip"
        }
    } else {
        "other"
    }
}

fn arch_rank(asset_arch: &str, current_arch: &str) -> u8 {
    if asset_arch == current_arch {
        0
    } else if asset_arch == "universal" {
        1
    } else {
        2
    }
}

fn kind_rank(kind: &str, platform: &str) -> u8 {
    match platform {
        "windows" => match kind {
            "setup" => 0,
            "msi" => 1,
            "portable" => 2,
            _ => 3,
        },
        "macos" => match kind {
            "dmg" => 0,
            _ => 1,
        },
        "linux" => match kind {
            "appimage" => 0,
            "archive" => 1,
            "deb" => 2,
            "rpm" => 3,
            _ => 4,
        },
        _ => 0,
    }
}

/// 当前平台优先、架构匹配优先、再按安装包类型偏好排序。
fn sort_assets(assets: &mut [LauncherUpdateAsset], platform: &str, arch: &str) {
    assets.sort_by(|a, b| {
        (b.platform == platform)
            .cmp(&(a.platform == platform))
            .then_with(|| arch_rank(&a.arch, arch).cmp(&arch_rank(&b.arch, arch)))
            .then_with(|| kind_rank(&a.kind, &a.platform).cmp(&kind_rank(&b.kind, &b.platform)))
            .then_with(|| a.name.cmp(&b.name))
    });
}

async fn fetch_latest_release() -> Result<Option<serde_json::Value>, CoreError> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let response = http()
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(common_err_ctx_key!("errors.update.failed", "检查更新失败"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if matches!(
        response.status(),
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::TOO_MANY_REQUESTS
    ) {
        bail_key!(
            "errors.update.rateLimited",
            "检查更新失败\nGitHub API 限流，请稍后重试"
        );
    }
    let response = response
        .error_for_status()
        .map_err(common_err_ctx_key!("errors.update.failed", "检查更新失败"))?;
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(common_err_ctx_key!("errors.update.failed", "检查更新失败"))?;

    // latest 端点通常已过滤，防御性跳过草稿/预发布。
    if json.get("draft").and_then(|v| v.as_bool()).unwrap_or(false)
        || json
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    {
        return Ok(None);
    }
    Ok(Some(json))
}

async fn check_github_api() -> Result<Option<UpdateInfo>, CoreError> {
    let Some(json) = fetch_latest_release().await? else {
        return Ok(None);
    };
    let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) else {
        bail_key!("errors.update.failed", "检查更新失败\n发布信息缺少版本号");
    };
    let Some(page_url) = json.get("html_url").and_then(|v| v.as_str()) else {
        bail_key!("errors.update.failed", "检查更新失败\n发布信息缺少下载地址");
    };

    let latest_version = normalize_version(tag);
    Ok(Some(UpdateInfo {
        current_version: APP_VERSION,
        update_available: is_newer(&latest_version, APP_VERSION),
        latest_version,
        url: page_url.to_string(),
        notes: json
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        published_at: json
            .get("published_at")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }))
}

fn normalize_version(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

/// 仅比较 `major.minor.patch`；预发布/构建后缀不参与（latest 端点本就只返回正式版）。
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let core = normalize_version(raw);
    let core = core.split(['-', '+']).next().unwrap_or_default();
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_maps_static_release_json() {
        let info = manifest_to_info(ReleaseManifest {
            version: "v1.0.4".into(),
            notes: "changes".into(),
            published_at: Some("2026-10-28T00:00:00Z".into()),
            url: String::new(),
            assets: Vec::new(),
        })
        .unwrap();
        assert_eq!(info.latest_version, "1.0.4");
        assert_eq!(info.notes, "changes");
        assert_eq!(info.published_at.as_deref(), Some("2026-10-28T00:00:00Z"));
        assert!(info.url.ends_with("/releases/tag/v1.0.4"));
    }

    #[test]
    fn manifest_deserializes_asset_list() {
        let manifest: ReleaseManifest = serde_json::from_value(serde_json::json!({
            "version": "1.2.3",
            "assets": [{
                "name": "Elytra-Launcher_1.2.3_windows-x64_setup.exe",
                "url": "https://example.com/setup.exe",
                "size": 42
            }]
        }))
        .unwrap();
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(manifest.assets[0].size, 42);
        let asset = build_asset(
            manifest.assets[0].name.clone(),
            manifest.assets[0].url.clone(),
            manifest.assets[0].size,
        )
        .unwrap();
        assert_eq!(asset.platform, "windows");
        assert_eq!(asset.kind, "setup");
    }

    #[test]
    fn version_order_is_numeric() {
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(is_newer("1.2.0", "1.1.99"));
        assert!(!is_newer("1.0.2", "1.0.2"));
        assert!(!is_newer("1.0.1", "1.0.2"));
        assert!(!is_newer("nightly", "1.0.2"));
    }

    #[test]
    fn version_tags_accept_v_prefix_and_suffix() {
        assert_eq!(parse_version("v1.0.2"), Some((1, 0, 2)));
        assert_eq!(parse_version("1.0.2-beta.1"), Some((1, 0, 2)));
        assert_eq!(parse_version("1.0"), None);
        assert_eq!(parse_version("1.0.2.3"), None);
    }

    fn test_asset(name: &str, platform: &str, arch: &str, kind: &str) -> LauncherUpdateAsset {
        LauncherUpdateAsset {
            name: name.to_string(),
            url: format!("https://example.com/{name}"),
            size: 1,
            platform: platform.to_string(),
            arch: arch.to_string(),
            kind: kind.to_string(),
            recommended: false,
        }
    }

    #[test]
    fn release_assets_are_classified_from_names() {
        let setup = "Elytra-Launcher_1.6.0_windows-x64_setup.exe";
        assert_eq!(asset_platform(setup), "windows");
        assert_eq!(asset_arch(setup), "x64");
        assert_eq!(asset_kind(setup), "setup");

        let dmg = "Elytra-Launcher_1.6.0_macos-universal.dmg";
        assert_eq!(asset_platform(dmg), "macos");
        assert_eq!(asset_arch(dmg), "universal");
        assert_eq!(asset_kind(dmg), "dmg");

        let appimage = "Elytra-Launcher_1.6.0_linux-x64.AppImage";
        assert_eq!(asset_platform(appimage), "linux");
        assert_eq!(asset_arch(appimage), "x64");
        assert_eq!(asset_kind(appimage), "appimage");

        let archive = "Elytra-Launcher_1.6.0_linux-x64_portable.tar.gz";
        assert_eq!(asset_kind(archive), "archive");
        let portable = "Elytra-Launcher_1.6.0_windows-x64_portable.zip";
        assert_eq!(asset_kind(portable), "portable");
        assert_eq!(asset_platform("release.json"), "other");
        assert_eq!(asset_kind("release.json"), "other");
    }

    #[test]
    fn assets_sort_current_platform_and_preferred_kind_first() {
        let mut assets = vec![
            test_asset("linux.AppImage", "linux", "x64", "appimage"),
            test_asset("windows_portable.zip", "windows", "x64", "portable"),
            test_asset("windows_setup.exe", "windows", "x64", "setup"),
            test_asset("mac.dmg", "macos", "universal", "dmg"),
            test_asset("windows.msi", "windows", "x64", "msi"),
            test_asset("windows_arm64_setup.exe", "windows", "arm64", "setup"),
        ];
        sort_assets(&mut assets, "windows", "x64");
        let names: Vec<&str> = assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            &names[..4],
            &[
                "windows_setup.exe",
                "windows.msi",
                "windows_portable.zip",
                "windows_arm64_setup.exe",
            ]
        );
        assert_eq!(names[4..], ["linux.AppImage", "mac.dmg"]);
    }

    #[test]
    fn fallback_assets_cover_all_platforms() {
        let mut assets = fallback_assets("1.2.3");
        assert_eq!(assets.len(), 8);
        assert!(assets.iter().all(|asset| asset.url.starts_with(
            "https://github.com/nefelibetas/ElytraLauncher/releases/download/v1.2.3/"
        )));
        sort_assets(&mut assets, "windows", "x64");
        assert_eq!(assets[0].kind, "setup");
        assert_eq!(assets[1].kind, "msi");
        assert_eq!(assets[2].kind, "portable");
    }

    #[test]
    fn draft_untagged_asset_urls_are_canonicalized() {
        const NAME: &str = "Elytra-Launcher_1.7.1_windows-x64_portable.zip";
        let untagged = format!(
            "https://github.com/nefelibetas/ElytraLauncher/releases/download/untagged-9befde7b2ea7048c0518/{NAME}"
        );
        let tagged = format!(
            "https://github.com/nefelibetas/ElytraLauncher/releases/download/v1.7.1/{NAME}"
        );
        assert_eq!(canonical_asset_url(&untagged, "1.7.1"), tagged);
        assert_eq!(canonical_asset_url(&tagged, "1.7.1"), tagged);
        let other = "https://mirror.example.com/file.zip";
        assert_eq!(canonical_asset_url(other, "1.7.1"), other);
    }

    #[test]
    fn json_assets_are_filtered_out() {
        let value = serde_json::json!({
            "name": "release.json",
            "browser_download_url": "https://example.com/release.json",
            "size": 10
        });
        assert!(parse_asset(&value).is_none());
        let value = serde_json::json!({
            "name": "Elytra-Launcher_1.6.0_linux-x64.deb",
            "browser_download_url": "http://example.com/a.deb",
            "size": 10
        });
        assert!(parse_asset(&value).is_none(), "非 https 链接应被拒绝");
    }
}
