// ── 整合包导出任务（MRPACK / CurseForge）：扫描匹配 → 写临时 zip → 原子替换目标 ──
// 目标文件只在完整写完后才出现/被替换，导出期间复制目标路径拿到的只会是旧文件或不存在。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::AppHandle;

use crate::downloader::path_lock::PathLock;
use crate::game::modpack::{
    export_pack_totals, mod_candidate_totals, pack_curseforge_with, pack_modpack_with,
    resolve_hosted_mods_with, ExportObserver, ExportPhase,
};
use crate::game::version::dir::version_dir_path;
use crate::task::model::{ModpackExportFormat, StepKind, TaskText};
use crate::task::progress::{
    cancelled, set_cancelled, set_done, set_error, set_phase, set_total, spawn_heartbeat,
    step_done, HeartbeatStats, StepRef,
};
use crate::task::store::JobSpec;
use crate::task::{spawn_task, EventSink, StepSpec, StepStatus, TaskProgress, TaskStatus};

/// 扫描/匹配在线文件；打包写入临时文件。两段共用同一组原子计数（心跳负责转成进度帧）。
struct TaskExportObserver {
    sink: Arc<dyn EventSink>,
    progress: Arc<Mutex<TaskProgress>>,
    cancel: Arc<AtomicBool>,
    bytes: Arc<AtomicU64>,
    files: Arc<AtomicU32>,
    step_idx: Arc<AtomicUsize>,
    step_bytes: Arc<AtomicU64>,
    step_files: Arc<AtomicU32>,
    step_total: Arc<AtomicU64>,
    step_files_total: Arc<AtomicU32>,
    pack_bytes: u64,
    pack_files: u32,
}

impl ExportObserver for TaskExportObserver {
    fn on_bytes(&self, bytes: u64) -> bool {
        if self.cancel.load(Ordering::SeqCst) {
            return false;
        }
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.step_bytes.fetch_add(bytes, Ordering::Relaxed);
        true
    }

    fn on_file(&self) -> bool {
        if self.cancel.load(Ordering::SeqCst) {
            return false;
        }
        self.files.fetch_add(1, Ordering::Relaxed);
        self.step_files.fetch_add(1, Ordering::Relaxed);
        true
    }

    fn on_phase(&self, phase: ExportPhase) -> bool {
        if self.cancel.load(Ordering::SeqCst) {
            return false;
        }
        if phase == ExportPhase::Pack {
            step_done(self.sink.as_ref(), &self.progress, 0);
            self.step_bytes.store(0, Ordering::Relaxed);
            self.step_files.store(0, Ordering::Relaxed);
            self.step_idx.store(1, Ordering::Relaxed);
            self.step_total.store(self.pack_bytes, Ordering::Relaxed);
            self.step_files_total
                .store(self.pack_files, Ordering::Relaxed);
            set_phase(
                self.sink.as_ref(),
                &self.progress,
                Some(TaskStatus::Running),
                "打包整合包",
                1,
                StepStatus::Running,
            );
        }
        true
    }
}

fn export_steps() -> Vec<StepSpec> {
    vec![
        StepSpec {
            name: "扫描整合包内容".into(),
            kind: StepKind::ExportScan,
        },
        StepSpec {
            name: "打包整合包".into(),
            kind: StepKind::ExportPack,
        },
    ]
}

/// 临时文件与目标同目录，保证 rename 不跨卷；`.elytra-part` 明确标识未完成。
fn temp_path(dest: &Path) -> Result<PathBuf, String> {
    let name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| keyed_msg!("errors.modpack.exportPathEmpty", "导出路径不能为空"))?;
    Ok(dest.with_file_name(format!("{name}.elytra-part")))
}

/// 注册导出任务：校验路径 → 写快照 → spawn（执行体 [`run_export`]）。
pub fn spawn_modpack_export(
    app: AppHandle,
    version_name: String,
    save_path: String,
    format: ModpackExportFormat,
) -> Result<String, String> {
    let vdir = version_dir_path(&version_name)?;
    let save_path = save_path.trim().to_string();
    if save_path.is_empty() {
        bail_key!("errors.modpack.exportPathEmpty", "导出路径不能为空");
    }
    let dest = PathBuf::from(&save_path);
    if dest.file_name().is_none() {
        bail_key!("errors.modpack.exportPathEmpty", "导出路径不能为空");
    }
    if dest.starts_with(&vdir) {
        bail_key!(
            "errors.modpack.exportInInstance",
            "不能导出到实例目录内\n{}",
            dest.display()
        );
    }

    let (name_key, fallback) = match format {
        ModpackExportFormat::Mrpack => (
            "task.backend.name.exportMrpack",
            format!("{version_name} 导出 MRPACK"),
        ),
        ModpackExportFormat::Curseforge => (
            "task.backend.name.exportCurseforge",
            format!("{version_name} 导出 CurseForge"),
        ),
    };
    let text = TaskText::new(name_key, &[("version", version_name.clone())]);
    spawn_task(
        app,
        fallback,
        text,
        export_steps(),
        JobSpec::ExportModpack {
            version_name: version_name.clone(),
            save_path: save_path.clone(),
            format,
        },
        move |sink, cancel, progress| {
            tauri::async_runtime::spawn(async move {
                run_export(
                    sink,
                    &cancel,
                    &progress,
                    vdir,
                    version_name,
                    PathBuf::from(save_path),
                    format,
                )
                .await;
            });
        },
    )
}

/// 导出执行体：两阶段进度 + 失败/取消清理临时文件；成功后原子替换目标。
pub(crate) async fn run_export(
    sink: Arc<dyn EventSink>,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<Mutex<TaskProgress>>,
    vdir: PathBuf,
    version_name: String,
    save_path: PathBuf,
    format: ModpackExportFormat,
) {
    let app = sink.as_ref();
    let tmp = match temp_path(&save_path) {
        Ok(path) => path,
        Err(e) => {
            set_error(app, progress, e);
            return;
        }
    };
    let Some(_lock) = PathLock::try_acquire(&tmp) else {
        set_error(
            app,
            progress,
            keyed_msg!("errors.modpack.exportBusy", "该导出文件正在被其他任务使用"),
        );
        return;
    };
    if let Some(parent) = tmp.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            set_error(
                app,
                progress,
                format!(
                    "{}\n{}",
                    keyed_msg!("errors.file.createDir", "创建目录失败"),
                    e
                ),
            );
            return;
        }
    }
    let _ = std::fs::remove_file(&tmp);

    set_phase(
        app,
        progress,
        Some(TaskStatus::Running),
        "扫描整合包内容",
        0,
        StepStatus::Running,
    );
    let (scan_files, scan_bytes) = mod_candidate_totals(&vdir);
    let (pack_files, pack_bytes) = export_pack_totals(&vdir);
    let total_files = scan_files.saturating_add(pack_files);
    let total_bytes = scan_bytes.saturating_add(pack_bytes);
    set_total(app, progress, total_files, total_bytes);

    let bytes = Arc::new(AtomicU64::new(0));
    let files = Arc::new(AtomicU32::new(0));
    let step_idx = Arc::new(AtomicUsize::new(0));
    let step_bytes = Arc::new(AtomicU64::new(0));
    let step_files = Arc::new(AtomicU32::new(0));
    let step_total = Arc::new(AtomicU64::new(scan_bytes));
    let step_files_total = Arc::new(AtomicU32::new(scan_files));
    let observer = Arc::new(TaskExportObserver {
        sink: Arc::clone(&sink),
        progress: Arc::clone(progress),
        cancel: Arc::clone(cancel),
        bytes: Arc::clone(&bytes),
        files: Arc::clone(&files),
        step_idx: Arc::clone(&step_idx),
        step_bytes: Arc::clone(&step_bytes),
        step_files: Arc::clone(&step_files),
        step_total: Arc::clone(&step_total),
        step_files_total: Arc::clone(&step_files_total),
        pack_bytes,
        pack_files,
    });
    let heartbeat = spawn_heartbeat(
        Arc::clone(&sink),
        progress,
        HeartbeatStats {
            done_files: files,
            total_files,
            bytes,
            est_total_bytes: Arc::new(AtomicU64::new(total_bytes)),
            percent_by_steps: false,
            step: Some(StepRef {
                idx: step_idx,
                step_bytes,
                step_total,
                files: step_files,
                files_total: step_files_total,
            }),
        },
    );

    let tmp_text = tmp.to_string_lossy().to_string();
    let result = match format {
        ModpackExportFormat::Mrpack => {
            let (hosted, hosted_names) = resolve_hosted_mods_with(&vdir, observer.as_ref()).await;
            if cancelled(cancel) {
                Ok(())
            } else {
                let vdir = vdir.clone();
                let version_name = version_name.clone();
                let observer = Arc::clone(&observer);
                match tokio::task::spawn_blocking(move || {
                    pack_modpack_with(
                        &vdir,
                        &version_name,
                        &tmp_text,
                        &hosted,
                        &hosted_names,
                        observer.as_ref(),
                    )
                })
                .await
                {
                    Ok(result) => result,
                    Err(e) => Err(keyed_msg!("errors.file.write", "写入导出文件失败\n{e}")),
                }
            }
        }
        ModpackExportFormat::Curseforge => {
            match pack_curseforge_with(&vdir, &version_name, &tmp_text, observer.as_ref()).await {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            }
        }
    };

    heartbeat.abort();
    if cancelled(cancel) {
        let _ = std::fs::remove_file(&tmp);
        set_cancelled(app, progress);
        return;
    }
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        set_error(app, progress, e);
        return;
    }
    if let Err(e) = commit_temp(&tmp, &save_path) {
        // 目标被占用时保留完整临时文件，避免整包重做。
        set_error(
            app,
            progress,
            format!(
                "{}\n{}\n{}",
                keyed_msg!(
                    "errors.modpack.exportCommit",
                    "无法保存导出文件（目标可能被占用），已完成的内容保留在临时文件"
                ),
                tmp.display(),
                e
            ),
        );
        return;
    }
    set_done(app, progress, "导出完成");
}

/// 用完整临时文件替换目标：Windows 下目标存在时 rename 会失败，先删旧文件再重试。
fn commit_temp(tmp: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(common_err_ctx_key!("errors.file.createDir", "创建目录失败"))?;
    }
    match std::fs::rename(tmp, dest) {
        Ok(()) => Ok(()),
        Err(_) if dest.exists() => {
            std::fs::remove_file(dest)
                .map_err(common_err_ctx_key!("errors.file.write", "写入文件失败"))?;
            std::fs::rename(tmp, dest)
                .map_err(common_err_ctx_key!("errors.file.write", "写入文件失败"))
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{commit_temp, export_steps, run_export, temp_path};
    use crate::task::model::{ModpackExportFormat, TaskText};
    use crate::task::{global, EventSink, TaskStatus};
    use std::sync::Arc;

    #[test]
    fn temp_path_marks_partial_next_to_target() {
        let dest = std::path::Path::new("packs/demo.mrpack");
        assert_eq!(
            temp_path(dest).unwrap(),
            std::path::Path::new("packs/demo.mrpack.elytra-part")
        );
    }

    #[test]
    fn commit_replaces_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("demo.mrpack.elytra-part");
        let dest = dir.path().join("demo.mrpack");
        std::fs::write(&tmp, b"new").unwrap();
        std::fs::write(&dest, b"old").unwrap();

        commit_temp(&tmp, &dest).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!tmp.exists(), "临时文件应已被移走");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_export_completes_atomically() {
        struct NullSink;

        impl EventSink for NullSink {
            fn send(&self, _event: &'static str, _payload: serde_json::Value) {}
        }

        let dir = tempfile::tempdir().unwrap();
        let vdir = dir.path().join("1.20.1");
        std::fs::create_dir_all(vdir.join("config")).unwrap();
        std::fs::write(vdir.join("config").join("a.txt"), b"hello").unwrap();
        let dest = dir.path().join("pack.mrpack");

        let (id, cancel, progress) =
            global().create("测试导出", &TaskText::default(), &export_steps());
        run_export(
            Arc::new(NullSink),
            &cancel,
            &progress,
            vdir,
            "1.20.1".into(),
            dest.clone(),
            ModpackExportFormat::Mrpack,
        )
        .await;

        assert_eq!(progress.lock().status, TaskStatus::Done, "导出应完成");
        assert!(dest.is_file(), "目标文件应存在");
        assert!(
            !dir.path().join("pack.mrpack.elytra-part").exists(),
            "临时文件应已移走"
        );
        assert!(!global().exists(&id), "任务应已收口");
    }
}
