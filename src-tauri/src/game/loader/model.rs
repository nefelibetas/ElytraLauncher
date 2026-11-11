//! ModLoader 种类与选择（安装目标 / 任务入参共用的领域枚举）。

use serde::{Deserialize, Serialize};

/// ModLoader 种类。前端传 "fabric"/"forge"/"neoforge"/"quilt"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "task/")]
pub enum LoaderKind {
    #[serde(rename = "fabric")]
    Fabric,
    #[serde(rename = "forge")]
    Forge,
    #[serde(rename = "neoforge")]
    NeoForge,
    #[serde(rename = "quilt")]
    Quilt,
}

impl LoaderKind {
    /// 英文标识（目录名/URL 模板用），与 serde rename 一致。
    pub fn name(&self) -> &'static str {
        match self {
            LoaderKind::Fabric => "fabric",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
            LoaderKind::Quilt => "quilt",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
            LoaderKind::Quilt => "Quilt",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "task/")]
pub struct LoaderChoice {
    pub kind: LoaderKind,
    pub version: String,
}
