//! Tauri 外壳的构建期信息：版本号与可选的 CurseForge 内置 key。

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 构建期从项目根 `.env` 注入；缺失时 CurseForge 源只能依赖用户 key。
pub const CURSEFORGE_API_KEY: Option<&str> = option_env!("ELYTRA_CURSEFORGE_API_KEY");
