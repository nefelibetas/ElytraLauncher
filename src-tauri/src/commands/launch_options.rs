use elytra_domain::error::CoreError;
use elytra_domain::jdk;
use elytra_domain::launcher::memory;
use elytra_domain::launcher::model;
use elytra_domain::launcher::model::InstanceLaunchOptions;
use elytra_domain::persist;

/// 系统内存信息（MB）：总内存 + 当前可用内存。
#[tauri::command]
pub fn get_memory_info() -> (u64, u64) {
    memory::system_memory_mb()
}

#[tauri::command]
pub fn get_launch_options() -> model::LaunchOptions {
    persist::launch_options()
}

#[tauri::command]
pub fn save_launch_options(config: model::LaunchOptions) -> Result<(), CoreError> {
    persist::save_launch_options(&config)
        .map_err(common_err_ctx_key!("errors.config.write", "保存配置失败"))
}

/// 实例级启动参数状态：`global` 为全局快照，`instance` 为实例快照，`effective` 为实际生效值。
#[derive(serde::Serialize)]
pub struct InstanceLaunchState {
    pub global: InstanceLaunchOptions,
    pub instance: Option<InstanceLaunchOptions>,
    pub effective: InstanceLaunchOptions,
}

fn mc_dir() -> std::path::PathBuf {
    persist::downloader_config().minecraft_dir_abs()
}

#[tauri::command]
pub fn get_instance_launch_options(version_id: String) -> InstanceLaunchState {
    let global_launch = persist::launch_options();
    let global = InstanceLaunchOptions::from_launch_options(&global_launch, jdk::read().selection);
    let instance = persist::instance::read_launch(&mc_dir(), &version_id);
    let effective = instance.clone().unwrap_or_else(|| global.clone());
    InstanceLaunchState {
        global,
        instance,
        effective,
    }
}

/// 保存实例启动参数；`None` = 删除 `[launch]` 段，回退全局。
#[tauri::command]
pub fn save_instance_launch_options(
    version_id: String,
    options: Option<InstanceLaunchOptions>,
) -> Result<(), CoreError> {
    persist::instance::write_launch(&mc_dir(), &version_id, options.as_ref())
}
