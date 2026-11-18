//! 配置 / 数据 / 游戏目录基准（平台感知）。

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[cfg(target_os = "windows")]
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

use crate::constants;

/// 配置目录在进程生命周期内不变；缓存避免每次 `current_exe()` 系统调用。
static CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(compute_config_dir);

/// 配置目录（Settings.toml / secrets.bin / tasks.db / 日志基准）：三端统一走平台用户数据目录
/// （Windows `%APPDATA%\Elytra`、macOS `~/Library/Application Support/Elytra`、Linux `$XDG_DATA_HOME/Elytra`）；
/// exe 旁在只读安装位置不可写、多用户共用会因 `secrets.bin` 存 token 而互相污染；dev / cargo test 仍走 exe 旁隔离。
pub fn config_dir() -> PathBuf {
    CONFIG_DIR.clone()
}

/// 平台默认下载目录：Windows `%USERPROFILE%\Downloads`、macOS `~/Downloads`、
/// Linux 优先 XDG user-dirs 的 `XDG_DOWNLOAD_DIR`（本地化目录名）否则 `~/Downloads`；
/// 无 HOME 的极端环境回退配置目录（必然可写）。
pub fn downloads_dir() -> PathBuf {
    platform_downloads_dir().unwrap_or_else(config_dir)
}

#[cfg(target_os = "windows")]
fn platform_downloads_dir() -> Option<PathBuf> {
    if let Some(dir) = windows_known_downloads() {
        return Some(dir);
    }
    let profile = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            let mut joined = PathBuf::from(drive);
            joined.push(path);
            Some(joined)
        })?;
    Some(profile.join("Downloads"))
}

/// 读取 Windows「下载」已知文件夹（支持被 OneDrive/用户重定向到其他盘）。
#[cfg(target_os = "windows")]
fn windows_known_downloads() -> Option<PathBuf> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
        .ok()?;
    let raw: String = key
        .get_value("{374DE290-123F-4565-9164-39C4925E467B}")
        .ok()?;
    let expanded = expand_env_vars(&raw);
    if expanded.is_empty() || expanded.contains('%') {
        return None;
    }
    Some(PathBuf::from(expanded))
}

/// 展开 `%VAR%`（注册表 User Shell Folders 常存 `%USERPROFILE%\Downloads`）。
#[cfg(any(test, target_os = "windows"))]
fn expand_env_vars(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push('%');
            out.push_str(after);
            return out;
        };
        let name = &after[..end];
        match std::env::var(name) {
            Ok(value) if !value.is_empty() => out.push_str(&value),
            _ => {
                out.push('%');
                out.push_str(name);
                out.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(target_os = "macos")]
fn platform_downloads_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_downloads_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if let Ok(text) = std::fs::read_to_string(config_home.join("user-dirs.dirs")) {
        if let Some(dir) = parse_xdg_download_dir(&text, &home) {
            // 配置指向已删除/未挂载目录时退回通用 Downloads，避免 downloader 在错误位置新建
            if dir.is_dir() {
                return Some(dir);
            }
        }
    }
    Some(home.join("Downloads"))
}

/// 解析 `user-dirs.dirs` 的 `XDG_DOWNLOAD_DIR`；只接受绝对路径，`$HOME` / `${HOME}` 展开。
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn parse_xdg_download_dir(contents: &str, home: &Path) -> Option<PathBuf> {
    let home_text = home.to_string_lossy();
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(value) = line.strip_prefix("XDG_DOWNLOAD_DIR=") else {
            continue;
        };
        let raw = value.trim().trim_matches('"');
        if raw.is_empty() {
            continue;
        }
        let expanded = raw
            .replace("${HOME}", &home_text)
            .replace("$HOME", &home_text);
        let path = PathBuf::from(expanded);
        if path.is_absolute() {
            return Some(path);
        }
    }
    None
}

fn compute_config_dir() -> PathBuf {
    if is_dev_runtime() {
        return exe_dir().join(constants::CONFIG_DIR);
    }
    if let Some(root) = platform_data_root() {
        return root.join(constants::CONFIG_DIR);
    }
    // 极端环境（APPDATA/HOME 均缺失）：退回 exe 旁，至少让应用能起来
    exe_dir().join(constants::CONFIG_DIR)
}

/// 平台用户数据根目录（Windows）：`%APPDATA%`（Roaming），缺省回退 `%USERPROFILE%\AppData\Roaming`。
#[cfg(target_os = "windows")]
fn platform_data_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| {
        std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("AppData/Roaming"))
    })
}

/// 平台用户数据根目录（macOS）：`~/Library/Application Support`。
#[cfg(target_os = "macos")]
fn platform_data_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
}

/// 平台用户数据根目录（Linux）：`$XDG_DATA_HOME` 非空用之，否则 `~/.local/share`（XDG）。
#[cfg(all(unix, not(target_os = "macos")))]
fn platform_data_root() -> Option<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x));
        }
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
}

/// exe 是否位于 cargo target/debug|release 下（dev 与 cargo test 进程）。
/// 此类进程配置走 exe 旁，使测试隔离于 `target/debug/deps/`、不污染真实用户目录。
fn is_dev_runtime() -> bool {
    std::env::current_exe()
        .ok()
        .map(|p| path_under_cargo_target(&p))
        .unwrap_or(false)
}

/// 路径是否形如 `…/target/{debug|release}/…`（纯函数，便于单测）。
fn path_under_cargo_target(p: &Path) -> bool {
    let comps: Vec<&str> = p
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    comps
        .windows(2)
        .any(|w| w[0] == "target" && matches!(w[1], "debug" | "release"))
}

/// 启动器 exe 所在目录（Windows 便携语义锚点）；macOS/Linux 数据布局不依赖 exe 位置。
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| {
            // current_exe 解析失败（极端环境）：回退用户主目录，不依赖进程 cwd
            std::env::var_os("HOME").map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 默认游戏目录（.minecraft）基准；`None` = 无合适默认，交用户显式添加。Windows exe 旁（便携，整目录拷走即迁移），
/// 仅只读安装位置（Program Files，MSI 默认）无默认（建目录都会失败）；macOS/Linux 一律无默认（exe 位置由包管理器/挂载器决定）；
/// dev / cargo test 走 exe 旁隔离。装机形态游戏目录可能在安装目录内，**卸载程序必须保留它**。
pub fn default_game_dir_base() -> Option<PathBuf> {
    if is_dev_runtime() {
        return Some(exe_dir());
    }
    #[cfg(target_os = "windows")]
    if !exe_dir_is_readonly() {
        return Some(exe_dir());
    }
    None
}

/// exe 是否位于常见的只读安装位置（Windows 的 Program Files）。
#[cfg(target_os = "windows")]
fn exe_dir_is_readonly() -> bool {
    path_is_readonly_install(&exe_dir())
}

/// 路径是否位于只读安装位置（Program Files）——纯函数不做 IO；命中时默认游戏目录改走用户数据目录。
#[cfg(target_os = "windows")]
fn path_is_readonly_install(p: &Path) -> bool {
    p.to_string_lossy().to_lowercase().contains("program files")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// cargo 产物路径判定：target/{debug|release} 段命中（deps 测试进程在 debug 下）。
    #[test]
    fn path_under_cargo_target_detects_dev_runtime() {
        assert!(path_under_cargo_target(Path::new(
            "src-tauri/target/debug/Elytra.exe"
        )));
        assert!(path_under_cargo_target(Path::new(
            "src-tauri/target/debug/deps/Elytra-abc.exe"
        )));
        assert!(path_under_cargo_target(Path::new(
            "home/u/elytra/target/release/elytra"
        )));
        assert!(!path_under_cargo_target(Path::new("/usr/bin/elytra")));
        assert!(!path_under_cargo_target(Path::new("target/debuggy/x")));
        assert!(!path_under_cargo_target(Path::new(
            "/tmp/.mount_elytraX/usr/bin/elytra"
        )));
    }

    /// 只读安装位置判定：Program Files 下默认游戏目录不能放 exe 旁（建目录会失败），应回退用户数据目录。
    #[cfg(target_os = "windows")]
    #[test]
    fn readonly_install_detects_program_files() {
        assert!(path_is_readonly_install(Path::new(
            r"C:\Program Files\Elytra\Elytra Launcher.exe"
        )));
        assert!(path_is_readonly_install(Path::new(
            r"C:\Program Files (x86)\Elytra\Elytra.exe"
        )));
        // 用户级安装（NSIS 默认位置）与便携目录都不算只读
        assert!(!path_is_readonly_install(Path::new(
            r"C:\Users\u\AppData\Local\Programs\Elytra\Elytra.exe"
        )));
        assert!(!path_is_readonly_install(Path::new(
            r"D:\Games\Elytra\Elytra.exe"
        )));
    }

    /// 注册表 User Shell Folders 的 `%VAR%` 展开：未知变量保留原样，供上层回退。
    #[test]
    fn env_vars_expand_and_unknown_are_kept() {
        let expanded = expand_env_vars("%PATH%/elytra-download");
        assert!(!expanded.contains("%PATH%"));
        assert!(expanded.ends_with("/elytra-download"));
        assert_eq!(expand_env_vars("plain"), "plain");
        assert_eq!(
            expand_env_vars("%ELY_UNKNOWN_DOWNLOAD_DIR%/x"),
            "%ELY_UNKNOWN_DOWNLOAD_DIR%/x"
        );
    }

    /// XDG 下载目录：展开 $HOME、忽略注释、拒绝相对路径。
    #[test]
    fn xdg_download_dir_parsing() {
        let home = std::env::temp_dir();
        let text = "# xdg\nXDG_DOWNLOAD_DIR=\"$HOME/下载\"\n";
        assert_eq!(parse_xdg_download_dir(text, &home), Some(home.join("下载")));

        let text = "XDG_DOWNLOAD_DIR=\"${HOME}/Downloads\"\n";
        assert_eq!(
            parse_xdg_download_dir(text, &home),
            Some(home.join("Downloads"))
        );

        assert_eq!(
            parse_xdg_download_dir("XDG_DOWNLOAD_DIR=\"relative/path\"", &home),
            None
        );
        assert_eq!(parse_xdg_download_dir("", &home), None);
    }
}
