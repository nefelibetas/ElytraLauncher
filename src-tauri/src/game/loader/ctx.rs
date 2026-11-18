//! ModLoader 安装的公共参数结构。

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::Arc;

use super::LoaderHook;
use crate::config::downloader::DownloaderConfig;
use crate::downloader::progress::ProgressCounters;

/// 安装目标版本目录（由 runner / 整合包解析，含冲突重命名 `{id}_n` 语义）。
#[derive(Clone, Copy)]
pub struct VersionTarget<'a> {
    pub dir_name: &'a str,
    pub vdir: &'a Path,
}

/// 原版 client jar 的并行下载状态：让 installer 等待真实进度/结果，而不是固定 120s。
#[derive(Clone, Copy)]
pub struct ClientJarWait<'a> {
    /// 版本 JSON 是否声明了 `downloads.client`（未声明且文件缺失时立即报错）。
    pub declared: bool,
    /// 1 = 下载并校验完成。
    pub done: &'a AtomicU32,
    /// 下载失败；等待方立即返回，真实错误仍由任务侧汇总。
    pub failed: &'a AtomicBool,
    /// 当前已写入字节（覆盖式），用于判断是否仍有进展。
    pub bytes: &'a AtomicU64,
}

/// 安装上下文：下载配置、进度计数、取消令牌、阶段钩子。
#[derive(Clone, Copy)]
pub struct InstallCtx<'a> {
    pub config: &'a DownloaderConfig,
    pub pc: &'a ProgressCounters,
    pub cancel: &'a Arc<AtomicBool>,
    pub hook: Option<LoaderHook<'a>>,
    /// runner 并行的 ClientJar 步骤状态；非任务调用（整合包/矩阵）为 None。
    pub client_jar: Option<ClientJarWait<'a>>,
}
