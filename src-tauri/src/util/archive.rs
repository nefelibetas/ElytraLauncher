//! ZIP 归档读取与条目落盘：父目录创建、错误文案统一，供 JDK 包与资源/整合包共用。

use std::io::Read;
use std::path::Path;

/// 打开 ZIP 归档；`label` 用于错误首行（如「整合包」「JDK 包」）。
pub fn open_zip(path: &Path, label: &str) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(path).map_err(common_err_ctx_key!(
        "errors.file.read",
        "打开{}失败",
        label
    ))?;
    zip::ZipArchive::new(file).map_err(common_err_ctx_key!("errors.file.read", "读取{}失败", label))
}

/// 把归档条目写入目标文件（父目录自动创建）；调用方负责条目类型/路径校验。
pub fn extract_entry_to(entry: &mut impl Read, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(common_err_ctx_key!("errors.file.createDir", "创建目录失败"))?;
    }
    let mut out = std::fs::File::create(dest).map_err(common_err_ctx_key!(
        "errors.file.write",
        "创建 {} 失败",
        dest.display()
    ))?;
    std::io::copy(entry, &mut out).map_err(common_err_ctx_key!(
        "errors.file.extract",
        "解压 {} 失败",
        dest.display()
    ))?;
    Ok(())
}
