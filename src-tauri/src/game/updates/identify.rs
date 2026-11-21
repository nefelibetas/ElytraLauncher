//! sha1 → Modrinth 项目识别：`/v2/version_file/{sha1}` 逐文件命中，
//! `/v2/projects?ids=[...]` 批量补全元数据；网络失败不阻塞列表，降级为本地信息 + network_error。

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;

use super::scan::ScanFilter;
use crate::downloader::fetcher;
use crate::downloader::hash::sha1_hex;
use crate::game::community::wiki_entry;
use crate::game::community::{self, api_routes, get_json_multi, ApiPlatform};

use crate::util;

/// 项目查询并发上限（Modrinth 限 300 req/min）。
pub(super) const QUERY_CONCURRENCY: usize = 8;

/// 本地 mod 详细列表项：命中带项目元数据，未命中/网络失败仅本地信息（其余字段 None）。
#[derive(Debug, Clone, Serialize)]
pub struct ModFileDetail {
    pub file_name: String,
    pub size: u64,
    pub last_modified: u64,
    pub project_id: Option<String>,
    pub title: Option<String>,
    /// MC百科中文译名（slug 匹配；未收录 None）
    pub title_zh: Option<String>,
    pub description: Option<String>,
    /// MCIM 中文简介（失败/未收录 None）
    pub description_zh: Option<String>,
    pub icon_url: Option<String>,
    pub downloads: Option<u64>,
    pub current_version: Option<String>,
}

/// 详细列表结果：网络失败时 mods 仍含本地信息，network_error 供前端提示降级。
#[derive(Debug, Clone, Serialize)]
pub struct ModListResult {
    pub mods: Vec<ModFileDetail>,
    pub network_error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceListResult {
    pub files: Vec<ModFileDetail>,
    pub network_error: bool,
}

/// 读文件计算 sha1（IO 放 spawn_blocking，避免阻塞 async executor）。
pub(super) async fn file_sha1(path: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || std::fs::read(&path).ok().map(|d| sha1_hex(&d)))
        .await
        .ok()
        .flatten()
}

/// 识别单个 mod：sha1 → version_file，返回 (project_id, current_version)；None=404，Err=网络/服务错误。
async fn identify_mod(dir: PathBuf, file_name: String) -> Result<Option<(String, String)>, ()> {
    let sha1 = file_sha1(dir.join(&file_name)).await.ok_or(())?;
    let urls = api_routes(
        ApiPlatform::Modrinth,
        &format!("/v2/version_file/{sha1}?algorithm=sha1"),
    );
    match get_json_multi(&urls, |u| fetcher::http().get(u.to_string())).await {
        Ok(cur) => {
            let project_id = cur["project_id"].as_str().map(str::to_string);
            let current_version = cur["version_number"].as_str().map(str::to_string);
            Ok(match (project_id, current_version) {
                (Some(pid), Some(ver)) => Some((pid, ver)),
                _ => None,
            })
        }
        Err(e) => {
            // 404 = 不在 Modrinth（正常未命中）；其余为网络/服务错误
            if e.contains("HTTP 404") {
                Ok(None)
            } else {
                Err(())
            }
        }
    }
}

/// 识别单个文件并补全元数据（逐文件调用）；未命中(404)/缺失/网络失败均 None（与详细列表降级一致）。
pub async fn identify_one_file(dir: &Path, file_name: &str) -> Option<ModFileDetail> {
    let file_name = file_name.to_string();
    let dir = dir.to_path_buf();
    let (project_id, current_version) =
        identify_mod(dir.clone(), file_name.clone()).await.ok()??;

    let meta = fetch_project_meta(&[Some((project_id.clone(), current_version.clone()))]).await?;
    let p = meta.get(&project_id)?;
    let title_zh = p["slug"].as_str().and_then(|slug| {
        wiki_entry::WIKI
            .lookup("modrinth", slug)
            .and_then(|e| e.chinese_raw.as_deref())
            .and_then(wiki_entry::chinese_name)
    });
    let translated =
        community::fetch_translations("modrinth", std::slice::from_ref(&project_id)).await;
    let meta_file = std::fs::metadata(dir.join(&file_name)).ok();
    let size = meta_file.as_ref().map(|m| m.len()).unwrap_or(0);
    let last_modified = meta_file.as_ref().map(util::modified_secs).unwrap_or(0);

    Some(ModFileDetail {
        file_name,
        size,
        last_modified,
        project_id: Some(project_id.clone()),
        title: p["title"].as_str().map(str::to_string),
        title_zh,
        description: p["description"].as_str().map(str::to_string),
        description_zh: translated.get(&project_id).cloned(),
        icon_url: p["icon_url"].as_str().map(str::to_string),
        downloads: p["downloads"].as_u64(),
        current_version: Some(current_version),
    })
}

type IdentifyResult = Result<Option<(String, String)>, ()>;

/// 并发识别但保持输入顺序：索引回填的 `buffer_unordered`，既避免队首慢请求阻塞补位，也不让结果与文件名错位。
async fn identify_all<F, Fut>(inputs: Vec<(String, PathBuf)>, identify: F) -> Vec<IdentifyResult>
where
    F: Fn(String, PathBuf) -> Fut,
    Fut: Future<Output = IdentifyResult>,
{
    let mut results: Vec<Option<IdentifyResult>> = vec![None; inputs.len()];
    let completed: Vec<(usize, IdentifyResult)> =
        futures_util::stream::iter(inputs.into_iter().enumerate())
            .map(|(index, (file_name, dir))| {
                let fut = identify(file_name, dir);
                async move { (index, fut.await) }
            })
            .buffer_unordered(QUERY_CONCURRENCY)
            .collect()
            .await;
    for (index, result) in completed {
        results[index] = Some(result);
    }
    results
        .into_iter()
        .map(|result| result.expect("identify_all 必须为每个输入写入结果"))
        .collect()
}

/// 扫描 + Modrinth 识别（sha1 → version_file → /v2/projects 批量详情）；mods(jar) 与资源目录(zip) 共用。
/// 网络失败降级为本地列表并置 network_error。
pub async fn list_dir_detailed(dir: &Path, filter: ScanFilter) -> ResourceListResult {
    let files = super::scan::scan_dir_files(dir, filter);
    if files.is_empty() {
        return ResourceListResult {
            files: Vec::new(),
            network_error: false,
        };
    }

    let dir = dir.to_path_buf();
    let inputs: Vec<(String, PathBuf)> = files
        .iter()
        .map(|f| (f.file_name.clone(), dir.clone()))
        .collect();
    let identified = identify_all(inputs, |file_name, dir| async move {
        identify_mod(dir, file_name).await
    })
    .await;

    // 网络/服务错误 → 置降级标记；404 未命中是合法降级
    let network_error = identified.iter().any(|r| r.is_err());

    let hits: Vec<Option<(String, String)>> =
        identified.into_iter().map(|r| r.unwrap_or(None)).collect();

    // project_id → 详情元数据；批量详情失败也算网络错误（命中项缺元数据）
    let meta = fetch_project_meta(&hits).await;
    let network_error = network_error || (meta.is_none() && hits.iter().any(Option::is_some));

    // 中文简介（MCIM 翻译；失败静默降级为英文）
    let ids: Vec<String> = hits.iter().flatten().map(|(pid, _)| pid.clone()).collect();
    let translated = community::fetch_translations("modrinth", &ids).await;

    let files = files
        .into_iter()
        .zip(hits)
        .map(|(f, hit)| match hit {
            Some((project_id, current_version)) => {
                let p = meta.as_ref().and_then(|m| m.get(&project_id));
                // MC百科按 slug 查中文译名（与社区搜索同源）
                let title_zh = p.and_then(|v| v["slug"].as_str()).and_then(|slug| {
                    wiki_entry::WIKI
                        .lookup("modrinth", slug)
                        .and_then(|e| e.chinese_raw.as_deref())
                        .and_then(wiki_entry::chinese_name)
                });
                ModFileDetail {
                    file_name: f.file_name,
                    size: f.size,
                    last_modified: f.last_modified,
                    project_id: Some(project_id.clone()),
                    title: p.and_then(|v| v["title"].as_str().map(str::to_string)),
                    title_zh,
                    description: p.and_then(|v| v["description"].as_str().map(str::to_string)),
                    description_zh: translated.get(&project_id).cloned(),
                    icon_url: p.and_then(|v| v["icon_url"].as_str().map(str::to_string)),
                    downloads: p.and_then(|v| v["downloads"].as_u64()),
                    current_version: Some(current_version),
                }
            }
            None => ModFileDetail {
                file_name: f.file_name,
                size: f.size,
                last_modified: f.last_modified,
                project_id: None,
                title: None,
                title_zh: None,
                description: None,
                description_zh: None,
                icon_url: None,
                downloads: None,
                current_version: None,
            },
        })
        .collect();

    ResourceListResult {
        files,
        network_error,
    }
}

pub async fn list_mods_detailed(mods_dir: &Path) -> ModListResult {
    let r = list_dir_detailed(mods_dir, ScanFilter::Jar).await;
    ModListResult {
        mods: r.files,
        network_error: r.network_error,
    }
}

/// 批量拉取项目元数据（GET /v2/projects?ids=[...]）；空 ids 或失败返回 None。
async fn fetch_project_meta(
    identified: &[Option<(String, String)>],
) -> Option<HashMap<String, serde_json::Value>> {
    let ids: Vec<String> = identified
        .iter()
        .flatten()
        .map(|(pid, _)| pid.clone())
        .collect();
    if ids.is_empty() {
        return None;
    }
    let arr = community::modrinth_projects_raw(&ids).await.ok()?;
    let mut map = HashMap::new();
    for v in arr {
        if let Some(id) = v["id"].as_str() {
            map.insert(id.to_string(), v);
        }
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 网络完成顺序与输入顺序相反时，结果仍必须按文件名顺序返回（否则元数据会串行）。
    #[tokio::test]
    async fn identify_all_preserves_input_order() {
        let inputs = vec![
            ("a.jar".to_string(), PathBuf::from("a")),
            ("b.jar".to_string(), PathBuf::from("b")),
            ("c.jar".to_string(), PathBuf::from("c")),
        ];
        let results = identify_all(inputs, |name, dir| async move {
            let delay = match name.as_str() {
                "a.jar" => 30,
                "b.jar" => 15,
                _ => 1,
            };
            tokio::time::sleep(Duration::from_millis(delay)).await;
            Ok(Some((name, dir.display().to_string())))
        })
        .await;
        let order: Vec<String> = results
            .into_iter()
            .map(|result| result.unwrap().unwrap().0)
            .collect();
        assert_eq!(order, vec!["a.jar", "b.jar", "c.jar"]);
    }

    /// 队首慢请求不能阻塞后续补位：这是 `buffered` 与“索引回填的 buffer_unordered”的关键差异。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn identify_all_refills_slot_before_slow_item_finishes() {
        use std::sync::Arc;
        use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};

        let (release_tx, release_rx) = oneshot::channel();
        let release_rx = Arc::new(AsyncMutex::new(Some(release_rx)));
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let inputs: Vec<(String, PathBuf)> = (0..9)
            .map(|index| (index.to_string(), PathBuf::from(index.to_string())))
            .collect();
        let mut task = Box::pin(identify_all(inputs, move |name, _dir| {
            let release_rx = Arc::clone(&release_rx);
            let started_tx = started_tx.clone();
            async move {
                let index: usize = name.parse().expect("测试索引");
                if index == 0 {
                    if let Some(rx) = release_rx.lock().await.take() {
                        let _ = rx.await;
                    }
                } else if index == 8 {
                    let _ = started_tx.send(());
                }
                Ok(Some((name, String::new())))
            }
        }));

        let refilled = tokio::select! {
            _ = &mut task => false,
            _ = tokio::time::sleep(Duration::from_millis(300)) => false,
            _ = started_rx.recv() => true,
        };
        let _ = release_tx.send(());
        let results = task.await;

        assert!(refilled, "第 9 个识别应在慢任务完成前补位");
        let order: Vec<usize> = results
            .into_iter()
            .map(|result| result.unwrap().unwrap().0.parse().unwrap())
            .collect();
        assert_eq!(order, (0..9).collect::<Vec<_>>());
    }
}
