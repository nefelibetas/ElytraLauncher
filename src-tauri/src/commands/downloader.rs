use elytra_domain::config::downloader::DownloaderConfig;
use elytra_domain::config::game_dirs;
use elytra_domain::downloader::fetcher;
use elytra_domain::error::CoreError;
use elytra_domain::game::version::dir;
use elytra_domain::game::version::dir::resolve_version_dir;
use elytra_domain::game::version::manifest;
use elytra_domain::game::version::model::VersionManifest;
use elytra_domain::persist::{self, downloader_config};

#[tauri::command]
pub async fn get_version_list() -> Result<VersionManifest, CoreError> {
    let config: DownloaderConfig = downloader_config();
    manifest::fetch_version_list(&config).await
}

#[tauri::command]
pub fn get_downloader_config() -> DownloaderConfig {
    downloader_config()
}

#[tauri::command]
pub fn save_downloader_config(config: DownloaderConfig) -> Result<(), CoreError> {
    let old: DownloaderConfig = downloader_config();
    persist::save_downloader_config(&config)
        .map_err(common_err_ctx_key!("errors.config.write", "保存配置失败"))?;
    fetcher::set_speed_limit(config.max_speed);
    if old.network_mode != config.network_mode {
        fetcher::reset_http_client();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_installed_versions() -> Result<Vec<dir::InstalledVersionInfo>, CoreError> {
    tauri::async_runtime::spawn_blocking(dir::list_installed)
        .await
        .map_err(common_err_ctx_key!(
            "errors.version.read",
            "读取版本信息失败"
        ))?
}

/// 激活目录 + 版本列表一次返回：启动页少一次 IPC，也避免两次枚举之间目录被切换。
#[derive(serde::Serialize)]
pub struct VersionState {
    pub active_dir: String,
    pub versions: Vec<dir::InstalledVersionInfo>,
}

#[tauri::command]
pub async fn get_version_state() -> Result<VersionState, CoreError> {
    tauri::async_runtime::spawn_blocking(|| -> Result<VersionState, CoreError> {
        let active_dir = game_dirs::active_dir().path;
        let versions = dir::list_installed()?;
        Ok(VersionState {
            active_dir,
            versions,
        })
    })
    .await
    .map_err(common_err_ctx_key!(
        "errors.version.read",
        "读取版本信息失败"
    ))?
}

#[tauri::command]
pub fn resolve_version_name(base_name: String) -> Result<String, CoreError> {
    let config: DownloaderConfig = downloader_config();
    let mc = config.minecraft_dir_abs();
    let (name, _) = resolve_version_dir(&mc, &base_name);
    Ok(name)
}
