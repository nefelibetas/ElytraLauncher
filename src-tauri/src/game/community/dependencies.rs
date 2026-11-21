//! 前置 Mod 解析：从主 Mod 版本出发递归展开 required/optional 依赖，按目标实例的 MC/loader
//! 选兼容版本；`visited` 去重与去环，节点上限 512。

use futures_util::stream::{self, StreamExt};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::api::CommunityVersion;
use super::{curseforge, modrinth};
use crate::downloader::hash::file_sha1_matches_streaming;

/// 节点上限：正常整合包远低于此；仅拦截异常/恶意元数据。
const MAX_DEPENDENCY_NODES: usize = 512;
/// 候选上限：控制宽依赖树的计划体积；当前选中版本始终保留。
const MAX_DEPENDENCY_CANDIDATES: usize = 40;
/// 冲突项目检查上限：异常元数据不应触发数百次网络请求。
const MAX_CONFLICT_PROJECTS: usize = 64;
/// 请求最大并发数量
const FETCH_CONCURRENCY: usize = 4;

type VersionsCache = Mutex<HashMap<(String, String, String, String), Vec<CommunityVersion>>>;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "community/")]
pub struct ModDependencyVersion {
    pub version_id: String,
    pub version_label: String,
    pub version_type: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "community/")]
pub struct ModDependencyItem {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub version_label: String,
    pub loader: String,
    pub game_version: String,
    pub url: String,
    pub filename: String,
    /// required | optional
    pub relation: String,
    /// 所属可选根（None = 主 Mod 的必装树）
    pub root_optional: Option<String>,
    pub installed: bool,
    pub error: Option<String>,
    /// 已安装的其他兼容版本（当前选中版本未安装时给出）。
    pub installed_version_label: Option<String>,
    /// 该项目已落盘的版本文件名；用于安装时替换旧版本。
    pub installed_files: Vec<String>,
    /// 仅含与实例 MC/loader 组合兼容的版本；按发布时间倒序。
    pub candidates: Vec<ModDependencyVersion>,
    pub candidate_total: usize,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export_to = "community/")]
pub struct ModDependencyPlan {
    pub instance_mc: String,
    pub instance_loader: String,
    pub items: Vec<ModDependencyItem>,
    pub errors: Vec<String>,
    /// 目标 mods 目录当前文件名；前端据此识别主 Mod 的旧版本。
    pub installed_files: Vec<String>,
}

/// 已安装的冲突 Mod（作者在版本元数据中声明 incompatible，且目标实例 mods/ 中确实存在）。
#[derive(Debug, Clone, Serialize)]
pub struct ModConflictItem {
    pub project_id: String,
    /// 项目标题；详情查询失败时回退为项目 ID。
    pub name: String,
    /// 命中的已安装版本号（多个文件时取最新发布）。
    pub version_label: String,
    pub installed_files: Vec<String>,
}

/// 目标实例的冲突检查结果；`version_applicable=false` 表示选中文件不适用于该实例。
#[derive(Debug, Clone, Serialize)]
pub struct ModConflictReport {
    pub instance_mc: String,
    pub instance_loader: String,
    pub version_applicable: bool,
    pub conflicts: Vec<ModConflictItem>,
    /// 单个冲突项目查询失败；非空表示结果不完整（不能当作“无冲突”）。
    pub errors: Vec<String>,
    /// 非失败提示（如冲突项被截断）；只展示，不触发重试。
    pub notices: Vec<String>,
}

struct Pending {
    project_id: String,
    version_hint: Option<String>,
    relation: String,
    root_optional: Option<String>,
}

#[derive(Default)]
struct SeenNode {
    /// 依赖声明里出现过的最严格版本提示。
    hint: Option<String>,
    /// 已经为该项目选定的版本。
    selected: Option<String>,
}

/// 同一项目出现不同版本要求时的冲突（返回 first, second）。
fn hint_conflict(
    hint: Option<&str>,
    selected: Option<&str>,
    incoming: Option<&str>,
) -> Option<(String, String)> {
    if let (Some(a), Some(b)) = (hint, incoming) {
        if a != b {
            return Some((a.to_string(), b.to_string()));
        }
    }
    if let (Some(selected), Some(incoming)) = (selected, incoming) {
        if selected != incoming {
            return Some((selected.to_string(), incoming.to_string()));
        }
    }
    None
}

/// 从本轮依赖要求推导下一轮需要强制指定的版本；多版本要求留给冲突报告。
fn next_forced_versions(
    root_project: &str,
    requirements: &HashMap<String, HashSet<String>>,
    items: &[ModDependencyItem],
    user_overrides: &HashMap<String, String>,
    previous: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut next = HashMap::new();
    for (project, hints) in requirements {
        if project == root_project || user_overrides.contains_key(project) || hints.len() != 1 {
            continue;
        }
        let Some(hint) = hints.iter().next() else {
            continue;
        };
        let selected = items
            .iter()
            .find(|item| item.project_id == *project)
            .map(|item| item.version_id.as_str());
        // 已满足且之前没有被强制时不必固定；已强制且 hint 仍在时必须保留，否则会被默认选择覆盖。
        if selected != Some(hint.as_str()) || previous.get(project) == Some(hint) {
            next.insert(project.clone(), hint.clone());
        }
    }
    next
}

#[derive(Clone, Copy)]
struct ResolveCtx<'a> {
    source: &'a str,
    project_id: &'a str,
    version_id: &'a str,
    mc_version: &'a str,
    loader: &'a str,
    overrides: &'a HashMap<String, String>,
    mods_dir: &'a Path,
    versions_cache: &'a VersionsCache,
}

/// 解析主 Mod 的全部前置（required + optional）。先按用户覆盖解析；若依赖图里发现
/// 唯一具体版本要求与当前选择不一致，则把该要求加入下一轮覆盖并重跑，最多 8 轮。
pub(crate) async fn resolve_mod_dependencies(
    source: &str,
    project_id: &str,
    version_id: &str,
    mc_version: &str,
    loader: &str,
    version_overrides: &HashMap<String, String>,
    mods_dir: &Path,
) -> Result<ModDependencyPlan, String> {
    let cache: VersionsCache = Mutex::new(HashMap::new());
    let mut forced: HashMap<String, String> = HashMap::new();
    let mut last_plan: Option<ModDependencyPlan> = None;
    for _ in 0..8 {
        let mut effective = version_overrides.clone();
        for (project, hint) in &forced {
            effective
                .entry(project.clone())
                .or_insert_with(|| hint.clone());
        }
        let (mut plan, requirements) = resolve_pass(ResolveCtx {
            source,
            project_id,
            version_id,
            mc_version,
            loader,
            overrides: &effective,
            mods_dir,
            versions_cache: &cache,
        })
        .await?;

        let next = next_forced_versions(
            project_id,
            &requirements,
            &plan.items,
            version_overrides,
            &forced,
        );

        if next == forced {
            fill_names(source, &mut plan.items).await;
            return Ok(plan);
        }
        forced = next;
        last_plan = Some(plan);
    }
    if let Some(mut plan) = last_plan {
        fill_names(source, &mut plan.items).await;
        Ok(plan)
    } else {
        Err(keyed_msg!(
            "errors.community.dependencyTooLarge",
            "依赖链过大，已停止解析"
        ))
    }
}

/// 检查选中版本在目标实例中已安装的冲突 Mod：作者声明 incompatible 的项目必须同时
/// 在 mods/ 中存在（按版本文件名匹配），未安装的冲突项不提示。
pub(crate) async fn resolve_mod_conflicts(
    source: &str,
    project_id: &str,
    version_id: &str,
    mc_version: &str,
    loader: &str,
    mods_dir: &Path,
) -> Result<ModConflictReport, String> {
    let installed = InstalledFiles::from_mods_dir(mods_dir);
    // 没有启用的 Mod 时不可能存在冲突；先返回，省掉主项目/冲突项目两组 API 请求。
    if !installed
        .exact
        .iter()
        .any(|name| !name.to_ascii_lowercase().ends_with(".disabled"))
    {
        return Ok(ModConflictReport {
            instance_mc: mc_version.to_string(),
            instance_loader: loader.to_string(),
            version_applicable: true,
            conflicts: Vec::new(),
            errors: Vec::new(),
            notices: Vec::new(),
        });
    }
    let root_versions = fetch_versions(source, project_id, mc_version, loader).await?;
    let Some(root) = root_versions
        .iter()
        .find(|version| version.version_id == version_id)
    else {
        return Ok(ModConflictReport {
            instance_mc: mc_version.to_string(),
            instance_loader: loader.to_string(),
            version_applicable: false,
            conflicts: Vec::new(),
            errors: Vec::new(),
            notices: Vec::new(),
        });
    };

    let mut seen = HashSet::new();
    let mut conflict_ids: Vec<String> = root
        .dependencies
        .iter()
        .filter(|dependency| {
            dependency.relation == "incompatible" && seen.insert(dependency.project_id.clone())
        })
        .map(|dependency| dependency.project_id.clone())
        .collect();
    let mut errors = Vec::new();
    let mut notices = Vec::new();
    if conflict_ids.len() > MAX_CONFLICT_PROJECTS {
        conflict_ids.truncate(MAX_CONFLICT_PROJECTS);
        notices.push(keyed_msg!(
            format!("errors.community.conflictTooMany?count={MAX_CONFLICT_PROJECTS}"),
            "冲突项过多，仅检查前 {MAX_CONFLICT_PROJECTS} 个"
        ));
    }
    if conflict_ids.is_empty() {
        return Ok(ModConflictReport {
            instance_mc: mc_version.to_string(),
            instance_loader: loader.to_string(),
            version_applicable: true,
            conflicts: Vec::new(),
            errors,
            notices,
        });
    }

    let fetched: Vec<(String, Result<Vec<CommunityVersion>, String>)> = stream::iter(conflict_ids)
        .map(|id| async move {
            let versions = fetch_versions(source, &id, mc_version, loader).await;
            (id, versions)
        })
        .buffer_unordered(FETCH_CONCURRENCY)
        .collect()
        .await;

    let mut conflicts = Vec::new();
    for (id, versions) in fetched {
        let versions = match versions {
            Ok(versions) => versions,
            Err(error) => {
                errors.push(keyed_msg!(
                    "errors.community.conflictCheckFailed",
                    "冲突检查失败\n{id}: {error}"
                ));
                continue;
            }
        };
        let Some((version_label, installed_files)) = installed_conflict(&versions, &installed)
            .or_else(|| installed_conflict_by_hash(&versions, mods_dir))
        else {
            continue;
        };
        conflicts.push(ModConflictItem {
            project_id: id.clone(),
            name: id,
            version_label,
            installed_files,
        });
    }
    if !conflicts.is_empty() {
        fill_conflict_names(source, &mut conflicts).await;
    }
    Ok(ModConflictReport {
        instance_mc: mc_version.to_string(),
        instance_loader: loader.to_string(),
        version_applicable: true,
        conflicts,
        errors,
        notices,
    })
}

/// 冲突项目在磁盘上命中的文件与版本号；`.jar.disabled` 不计入（未启用不构成冲突）。
fn installed_conflict(
    versions: &[CommunityVersion],
    installed: &InstalledFiles,
) -> Option<(String, Vec<String>)> {
    let mut matched: Vec<(&CommunityVersion, String)> = versions
        .iter()
        .filter_map(|version| {
            installed
                .actual_name(&version.filename)
                .filter(|name| !name.to_ascii_lowercase().ends_with(".disabled"))
                .map(|name| (version, name.to_string()))
        })
        .collect();
    let (version, _) = matched
        .iter()
        .max_by(|a, b| a.0.date_published.cmp(&b.0.date_published))?;
    let version_label = version.version_number.clone();
    let mut installed_files: Vec<String> = matched.drain(..).map(|(_, name)| name).collect();
    installed_files.sort();
    installed_files.dedup();
    Some((version_label, installed_files))
}

/// 文件名不一致（跨源/被重命名）时按 sha1 兜底；只对大小命中声明构件的文件做流式校验。
fn installed_conflict_by_hash(
    versions: &[CommunityVersion],
    mods_dir: &Path,
) -> Option<(String, Vec<String>)> {
    let mut by_size: HashMap<u64, Vec<&CommunityVersion>> = HashMap::new();
    for version in versions {
        if version.sha1.as_deref().is_some_and(|sha| !sha.is_empty()) {
            by_size.entry(version.file_size).or_default().push(version);
        }
    }
    if by_size.is_empty() {
        return None;
    }
    let entries = std::fs::read_dir(mods_dir).ok()?;
    let mut matched: Vec<(&CommunityVersion, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".disabled") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(candidates) = by_size.get(&meta.len()) else {
            continue;
        };
        let path = entry.path();
        if let Some(version) = candidates.iter().find(|version| {
            version
                .sha1
                .as_deref()
                .is_some_and(|sha| file_sha1_matches_streaming(&path, sha))
        }) {
            matched.push((version, name));
        }
    }
    let (version, _) = matched
        .iter()
        .max_by(|a, b| a.0.date_published.cmp(&b.0.date_published))?;
    let version_label = version.version_number.clone();
    let mut installed_files: Vec<String> = matched.drain(..).map(|(_, name)| name).collect();
    installed_files.sort();
    installed_files.dedup();
    Some((version_label, installed_files))
}

/// 冲突项标题（详情查询失败保留项目 ID，不阻塞提示）。
async fn fill_conflict_names(source: &str, conflicts: &mut [ModConflictItem]) {
    let ids: Vec<String> = conflicts
        .iter()
        .map(|conflict| conflict.project_id.clone())
        .collect();
    let details = match source {
        "modrinth" => modrinth::modrinth_details(&ids, false).await,
        "curseforge" => curseforge::curseforge_details(&ids, false).await,
        _ => return,
    };
    let Ok(details) = details else {
        return;
    };
    let names: HashMap<String, String> = details
        .into_iter()
        .map(|item| (item.id, item.title))
        .collect();
    for conflict in conflicts {
        if let Some(name) = names.get(&conflict.project_id) {
            conflict.name = name.clone();
        }
    }
}

/// 单次解析：在 `overrides`（用户覆盖 + 迭代学到的版本约束）下展开依赖图。
async fn resolve_pass(
    ctx: ResolveCtx<'_>,
) -> Result<(ModDependencyPlan, HashMap<String, HashSet<String>>), String> {
    let ResolveCtx {
        source,
        project_id,
        version_id,
        mc_version,
        loader,
        overrides,
        mods_dir,
        versions_cache,
    } = ctx;
    let installed = InstalledFiles::from_mods_dir(mods_dir);
    let mut installed_files: Vec<String> = installed.exact.iter().cloned().collect();
    installed_files.sort();
    let mut plan = ModDependencyPlan {
        instance_mc: mc_version.to_string(),
        instance_loader: loader.to_string(),
        items: Vec::new(),
        errors: Vec::new(),
        installed_files,
    };
    let mut requirements: HashMap<String, HashSet<String>> = HashMap::new();

    let root_versions =
        fetch_versions_cached(versions_cache, source, project_id, mc_version, loader).await?;
    let Some(root) = root_versions
        .iter()
        .find(|version| version.version_id == version_id)
    else {
        plan.errors.push(keyed_msg!(
            "errors.community.dependencyMainVersionMismatch",
            "主 Mod 版本不适用于当前实例"
        ));
        return Ok((plan, requirements));
    };

    let mut seen: HashMap<String, SeenNode> = HashMap::new();
    seen.insert(
        project_id.to_string(),
        SeenNode {
            hint: None,
            selected: Some(version_id.to_string()),
        },
    );
    let mut conflicted: HashSet<String> = HashSet::new();

    let mut required_queue: VecDeque<Pending> = VecDeque::new();
    let mut optional_queue: VecDeque<Pending> = VecDeque::new();
    seed_dependencies(root, None, &mut required_queue, &mut optional_queue);

    loop {
        // 同层并发：required 队列优先，排空后再处理 optional（与串行 BFS 的队列优先级一致）。
        let mut batch = Vec::new();
        let mut overflow = false;
        {
            let queue = if required_queue.is_empty() {
                &mut optional_queue
            } else {
                &mut required_queue
            };
            while let Some(pending) = queue.pop_front() {
                let incoming = pending.version_hint.as_deref();
                if let Some(hint) = incoming {
                    requirements
                        .entry(pending.project_id.clone())
                        .or_default()
                        .insert(hint.to_string());
                }
                if let Some(state) = seen.get_mut(&pending.project_id) {
                    let conflict =
                        hint_conflict(state.hint.as_deref(), state.selected.as_deref(), incoming);
                    if state.hint.is_none() {
                        state.hint = incoming.map(str::to_string);
                    }
                    if !overrides.contains_key(&pending.project_id) {
                        if let Some((first, second)) = conflict {
                            record_conflict(
                                &mut plan,
                                &mut conflicted,
                                &pending.project_id,
                                &first,
                                &second,
                            );
                        }
                    }
                    continue;
                }
                seen.insert(
                    pending.project_id.clone(),
                    SeenNode {
                        hint: incoming.map(str::to_string),
                        selected: None,
                    },
                );
                if seen.len() > MAX_DEPENDENCY_NODES {
                    overflow = true;
                    break;
                }
                batch.push(pending);
            }
        }
        backfill_hints(&mut batch, &seen);
        if batch.is_empty() {
            if overflow {
                plan.errors.push(keyed_msg!(
                    "errors.community.dependencyTooLarge",
                    "依赖链过大，已停止解析"
                ));
                break;
            }
            if required_queue.is_empty() && optional_queue.is_empty() {
                break;
            }
            // 本层全是 visited 命中的重复节点：换另一个队列继续。
            continue;
        }

        let mut results: Vec<_> = stream::iter(batch.into_iter().enumerate())
            .map(|(index, pending)| async move {
                let versions = fetch_versions_cached(
                    versions_cache,
                    source,
                    &pending.project_id,
                    mc_version,
                    loader,
                )
                .await;
                (index, pending, versions)
            })
            .buffer_unordered(FETCH_CONCURRENCY)
            .collect()
            .await;
        // buffer_unordered 按完成顺序返回；按原下标还原，保证 items/visited 顺序确定。
        results.sort_by_key(|(index, _, _)| *index);

        for (_, pending, versions) in results {
            let versions = match versions {
                Ok(versions) => versions,
                Err(_) => {
                    plan.items.push(missing_item(
                        &pending,
                        mc_version,
                        loader,
                        &keyed_msg!("errors.community.dependencyFetchFailed", "无法获取前置版本"),
                        &[],
                    ));
                    continue;
                }
            };
            let selected = match pick_version(
                &versions,
                overrides.get(&pending.project_id).map(String::as_str),
                pending.version_hint.as_deref(),
                &installed,
            ) {
                Ok(version) => version,
                Err(error) => {
                    plan.items.push(missing_item(
                        &pending, mc_version, loader, &error, &versions,
                    ));
                    continue;
                }
            };

            if let Some(state) = seen.get_mut(&pending.project_id) {
                state.selected = Some(selected.version_id.clone());
                if !overrides.contains_key(&pending.project_id) {
                    if let Some(hint) = state.hint.as_deref() {
                        if hint != selected.version_id {
                            record_conflict(
                                &mut plan,
                                &mut conflicted,
                                &pending.project_id,
                                hint,
                                &selected.version_id,
                            );
                        }
                    }
                }
            }

            let filename = selected.filename.clone();
            let url = selected.download_url.clone();
            let error = url.is_empty().then(|| {
                keyed_msg!(
                    "errors.community.dependencyNoDownloadUrl",
                    "缺少下载地址（作者可能禁止第三方分发）"
                )
            });
            let (candidates, candidate_total) = build_candidates(&versions, &selected.version_id);
            let installed_files = installed.installed_names(&versions);
            let installed_version_label = versions
                .iter()
                .find(|version| installed.contains(&version.filename))
                .filter(|version| version.version_id != selected.version_id)
                .map(|version| version.version_number.clone());
            plan.items.push(ModDependencyItem {
                project_id: pending.project_id.clone(),
                version_id: selected.version_id.clone(),
                name: selected.filename.clone(),
                version_label: selected.version_number.clone(),
                loader: loader.to_string(),
                game_version: mc_version.to_string(),
                url,
                filename: filename.clone(),
                relation: pending.relation.clone(),
                root_optional: pending.root_optional.clone(),
                installed: installed.contains(&filename),
                installed_version_label,
                installed_files,
                error,
                candidates,
                candidate_total,
            });
            if plan.items.last().is_some_and(|item| item.error.is_some()) {
                continue;
            }

            let child_optional_root = pending.root_optional.as_ref();
            for dependency in &selected.dependencies {
                let relation = dependency.relation.as_str();
                if relation != "required" && relation != "optional" {
                    continue;
                }
                let is_optional_root = relation == "optional";
                let root = if is_optional_root {
                    Some(dependency.project_id.clone())
                } else {
                    child_optional_root.cloned()
                };
                let pending = Pending {
                    project_id: dependency.project_id.clone(),
                    version_hint: dependency.version_id.clone(),
                    relation: relation.to_string(),
                    root_optional: root,
                };
                if child_optional_root.is_some() || is_optional_root {
                    optional_queue.push_back(pending);
                } else {
                    required_queue.push_back(pending);
                }
            }
        }

        if overflow {
            plan.errors.push(keyed_msg!(
                "errors.community.dependencyTooLarge",
                "依赖链过大，已停止解析"
            ));
            break;
        }
    }

    Ok((plan, requirements))
}

/// 同一批次里后到的具体 hint 回填到尚未解析的 pending，避免先按“任意版本”选完再报假冲突。
fn backfill_hints(batch: &mut [Pending], seen: &HashMap<String, SeenNode>) {
    for pending in batch {
        if pending.version_hint.is_none() {
            if let Some(state) = seen.get(&pending.project_id) {
                pending.version_hint = state.hint.clone();
            }
        }
    }
}

fn record_conflict(
    plan: &mut ModDependencyPlan,
    conflicted: &mut HashSet<String>,
    project_id: &str,
    first: &str,
    second: &str,
) {
    if !conflicted.insert(project_id.to_string()) {
        return;
    }
    plan.errors.push(keyed_msg!(
        format!(
            "errors.community.dependencyVersionConflict?project={project_id}&first={first}&second={second}"
        ),
        "前置版本冲突\n{project_id} 同时要求 {first} 与 {second}"
    ));
}

async fn fetch_versions_cached(
    cache: &VersionsCache,
    source: &str,
    project_id: &str,
    mc_version: &str,
    loader: &str,
) -> Result<Vec<CommunityVersion>, String> {
    let key = (
        source.to_string(),
        project_id.to_string(),
        mc_version.to_string(),
        loader.to_string(),
    );
    if let Some(hit) = cache.lock().get(&key) {
        return Ok(hit.clone());
    }
    let versions = fetch_versions(source, project_id, mc_version, loader).await?;
    cache.lock().insert(key, versions.clone());
    Ok(versions)
}

async fn fetch_versions(
    source: &str,
    project_id: &str,
    mc_version: &str,
    loader: &str,
) -> Result<Vec<CommunityVersion>, String> {
    let mut versions = fetch_platform_versions(source, project_id, mc_version, loader).await?;
    // Quilt 可直接加载 Fabric 模组，但平台按 loader 精确过滤；补一份 Fabric 版本（按 id 去重）。
    if loader.eq_ignore_ascii_case("quilt") {
        if let Ok(fabric) = fetch_platform_versions(source, project_id, mc_version, "fabric").await
        {
            let known: HashSet<String> = versions
                .iter()
                .map(|version| version.version_id.clone())
                .collect();
            versions.extend(
                fabric
                    .into_iter()
                    .filter(|version| !known.contains(version.version_id.as_str())),
            );
        }
    }
    Ok(versions
        .into_iter()
        .filter(|version| version_supports(version, mc_version, loader))
        .collect())
}

async fn fetch_platform_versions(
    source: &str,
    project_id: &str,
    mc_version: &str,
    loader: &str,
) -> Result<Vec<CommunityVersion>, String> {
    match source {
        "modrinth" => {
            modrinth::modrinth_versions(project_id, Some(mc_version), Some(loader), false).await
        }
        "curseforge" => {
            curseforge::curseforge_versions(project_id, Some(mc_version), Some(loader), false).await
        }
        other => Err(keyed_msg!(
            "errors.community.unsupportedSource",
            "不支持的资源来源\n{other}"
        )),
    }
}

/// 平台过滤之外再校验一次，确保候选只属于实例的 MC/loader 组合；Quilt 接受 Fabric 条目。
fn version_supports(version: &CommunityVersion, mc_version: &str, loader: &str) -> bool {
    version
        .game_versions
        .iter()
        .any(|value| value == mc_version)
        && version_supports_loader(version, loader)
}

fn version_supports_loader(version: &CommunityVersion, loader: &str) -> bool {
    let quilt = loader.eq_ignore_ascii_case("quilt");
    version.loaders.iter().any(|value| {
        value.eq_ignore_ascii_case(loader) || (quilt && value.eq_ignore_ascii_case("fabric"))
    })
}

/// 版本覆盖优先，其次精确 `version_hint`；否则 release 优先，最后取第一个。
fn pick_version<'a>(
    versions: &'a [CommunityVersion],
    override_id: Option<&str>,
    hint: Option<&str>,
    installed: &InstalledFiles,
) -> Result<&'a CommunityVersion, String> {
    if let Some(id) = override_id.or(hint) {
        return versions
            .iter()
            .find(|version| version.version_id == id)
            .ok_or_else(|| {
                keyed_msg!(
                    "errors.community.dependencyVersionMismatch",
                    "指定前置版本不适用于当前实例"
                )
            });
    }
    versions
        .iter()
        .find(|version| installed.contains(&version.filename))
        .or_else(|| {
            versions
                .iter()
                .find(|version| version.version_type == "release")
        })
        .or_else(|| versions.first())
        .ok_or_else(|| {
            keyed_msg!(
                "errors.community.dependencyNoCompatibleVersion",
                "没有适用于当前实例的前置版本"
            )
        })
}

/// 候选按发布时间倒序，限制数量；当前选中项即使超出上限也补进列表。
fn build_candidates(
    versions: &[CommunityVersion],
    selected_id: &str,
) -> (Vec<ModDependencyVersion>, usize) {
    let mut seen = HashSet::new();
    let mut unique: Vec<&CommunityVersion> = versions
        .iter()
        .filter(|version| seen.insert(version.version_id.as_str()))
        .collect();
    unique.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    let total = unique.len();
    let mut candidates: Vec<ModDependencyVersion> = unique
        .iter()
        .take(MAX_DEPENDENCY_CANDIDATES)
        .map(|version| dependency_version(version))
        .collect();
    if !candidates
        .iter()
        .any(|candidate| candidate.version_id == selected_id)
    {
        if let Some(selected) = unique
            .iter()
            .find(|version| version.version_id == selected_id)
        {
            candidates.push(dependency_version(selected));
        }
    }
    (candidates, total)
}

fn dependency_version(version: &CommunityVersion) -> ModDependencyVersion {
    ModDependencyVersion {
        version_id: version.version_id.clone(),
        version_label: version.version_number.clone(),
        version_type: version.version_type.clone(),
    }
}

fn seed_dependencies(
    root: &CommunityVersion,
    root_optional: Option<&str>,
    required: &mut VecDeque<Pending>,
    optional: &mut VecDeque<Pending>,
) {
    for dependency in &root.dependencies {
        let relation = dependency.relation.as_str();
        let is_optional_root = relation == "optional";
        if relation != "required" && !is_optional_root {
            continue;
        }
        let pending = Pending {
            project_id: dependency.project_id.clone(),
            version_hint: dependency.version_id.clone(),
            relation: relation.to_string(),
            root_optional: if is_optional_root {
                Some(dependency.project_id.clone())
            } else {
                root_optional.map(str::to_string)
            },
        };
        if is_optional_root || root_optional.is_some() {
            optional.push_back(pending);
        } else {
            required.push_back(pending);
        }
    }
}

/// 已安装 Mod 文件名集合：跨源文件名可能仅分隔符不同（`fabric-api-0.166.2+26.3.jar` vs
/// `fabric-api-0.166.2-26.3.jar`），归一化后仍视为已安装。
struct InstalledFiles {
    exact: HashSet<String>,
    /// 归一化文件名 → 磁盘实际文件名（替换旧文件时使用磁盘名）
    normalized: HashMap<String, String>,
}

impl InstalledFiles {
    fn from_mods_dir(mods_dir: &Path) -> Self {
        let mut exact = HashSet::new();
        let mut normalized = HashMap::new();
        let Ok(entries) = std::fs::read_dir(mods_dir) else {
            return Self { exact, normalized };
        };
        for entry in entries.flatten() {
            // mods/ 可能混有配置目录/临时目录；只把文件当作已安装 Mod。
            if !entry.metadata().is_ok_and(|meta| meta.is_file()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            normalized
                .entry(normalize_mod_filename(&name))
                .or_insert_with(|| name.clone());
            exact.insert(name);
        }
        Self { exact, normalized }
    }

    fn contains(&self, filename: &str) -> bool {
        self.exact.contains(filename)
            || self
                .normalized
                .contains_key(&normalize_mod_filename(filename))
    }

    /// 版本文件对应的磁盘实际文件名（精确或归一化命中）。
    fn actual_name<'a>(&'a self, filename: &'a str) -> Option<&'a str> {
        if self.exact.contains(filename) {
            return Some(filename);
        }
        self.normalized
            .get(&normalize_mod_filename(filename))
            .map(String::as_str)
    }

    /// 版本列表中所有已安装文件对应的磁盘名（去重排序）。
    fn installed_names(&self, versions: &[CommunityVersion]) -> Vec<String> {
        let mut names: Vec<String> = versions
            .iter()
            .filter_map(|version| self.actual_name(&version.filename).map(str::to_string))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    #[cfg(test)]
    fn for_test(names: &[&str]) -> Self {
        let exact: HashSet<String> = names.iter().map(|name| name.to_string()).collect();
        let normalized = exact
            .iter()
            .map(|name| (normalize_mod_filename(name), name.clone()))
            .collect();
        Self { exact, normalized }
    }
}

/// 文件名归一化：大小写、`+`/`_`/空格统一为 `-`，并合并连续分隔符；只用于跨源安装判定。
fn normalize_mod_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_separator = false;
    for ch in name.chars().map(|c| c.to_ascii_lowercase()) {
        let mapped = match ch {
            '+' | '_' | ' ' => '-',
            c => c,
        };
        if mapped == '-' {
            if prev_separator {
                continue;
            }
            prev_separator = true;
        } else {
            prev_separator = false;
        }
        out.push(mapped);
    }
    out
}

fn missing_item(
    pending: &Pending,
    mc_version: &str,
    loader: &str,
    error: &str,
    versions: &[CommunityVersion],
) -> ModDependencyItem {
    let (candidates, candidate_total) = build_candidates(versions, "");
    ModDependencyItem {
        project_id: pending.project_id.clone(),
        version_id: pending.version_hint.clone().unwrap_or_default(),
        name: pending.project_id.clone(),
        version_label: String::new(),
        loader: loader.to_string(),
        game_version: mc_version.to_string(),
        url: String::new(),
        filename: String::new(),
        relation: pending.relation.clone(),
        root_optional: pending.root_optional.clone(),
        installed: false,
        installed_version_label: None,
        installed_files: Vec::new(),
        error: Some(error.to_string()),
        candidates,
        candidate_total,
    }
}

/// 批量补项目标题；失败时保留文件名/项目 ID。
async fn fill_names(source: &str, items: &mut [ModDependencyItem]) {
    let ids: Vec<String> = items
        .iter()
        .map(|item| item.project_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return;
    }
    let names: HashMap<String, String> = match source {
        "modrinth" => modrinth::modrinth_details(&ids, false)
            .await
            .map(|items| {
                items
                    .into_iter()
                    .map(|item| (item.id, item.title))
                    .collect()
            })
            .unwrap_or_default(),
        "curseforge" => curseforge::curseforge_details(&ids, false)
            .await
            .map(|details| {
                details
                    .into_iter()
                    .map(|detail| (detail.id, detail.title))
                    .collect()
            })
            .unwrap_or_default(),
        _ => HashMap::new(),
    };
    for item in items {
        if let Some(name) = names.get(&item.project_id) {
            item.name = name.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::api::CommunityDependency;
    use super::*;

    fn version(
        id: &str,
        version_type: &str,
        dependencies: Vec<CommunityDependency>,
    ) -> CommunityVersion {
        CommunityVersion {
            version_id: id.to_string(),
            version_number: id.to_string(),
            game_versions: vec!["1.20.1".into()],
            game_version_range: Some("1.20.1".into()),
            loaders: vec!["fabric".into()],
            filename: format!("{id}.jar"),
            download_url: format!("https://example.com/{id}.jar"),
            file_size: 0,
            sha1: None,
            date_published: String::new(),
            downloads: 0,
            version_type: version_type.to_string(),
            dependencies,
        }
    }

    fn dependency(
        project_id: &str,
        relation: &str,
        version_id: Option<&str>,
    ) -> CommunityDependency {
        CommunityDependency {
            project_id: project_id.to_string(),
            relation: relation.to_string(),
            version_id: version_id.map(str::to_string),
        }
    }

    #[test]
    fn backfill_hints_fills_only_missing_hints() {
        let pending = |project: &str, hint: Option<&str>| Pending {
            project_id: project.to_string(),
            version_hint: hint.map(str::to_string),
            relation: "required".into(),
            root_optional: None,
        };
        let mut batch = vec![pending("a", None), pending("b", Some("keep"))];
        let seen = HashMap::from([
            (
                "a".to_string(),
                SeenNode {
                    hint: Some("picked".into()),
                    selected: None,
                },
            ),
            (
                "b".to_string(),
                SeenNode {
                    hint: Some("other".into()),
                    selected: None,
                },
            ),
        ]);
        backfill_hints(&mut batch, &seen);
        assert_eq!(batch[0].version_hint.as_deref(), Some("picked"));
        assert_eq!(batch[1].version_hint.as_deref(), Some("keep"));
    }

    #[test]
    fn next_forced_versions_only_for_single_unmet_hints() {
        let mut requirements = HashMap::new();
        requirements.insert("one".to_string(), HashSet::from(["v1".to_string()]));
        requirements.insert("met".to_string(), HashSet::from(["v1".to_string()]));
        requirements.insert(
            "multi".to_string(),
            HashSet::from(["v1".into(), "v2".into()]),
        );
        requirements.insert("pinned".to_string(), HashSet::from(["v1".to_string()]));
        requirements.insert("root".to_string(), HashSet::from(["other".to_string()]));
        let mut met = missing_item(
            &Pending {
                project_id: "met".into(),
                version_hint: None,
                relation: "required".into(),
                root_optional: None,
            },
            "1.20.1",
            "fabric",
            "x",
            &[],
        );
        met.version_id = "v1".into();
        let items = vec![met];
        let overrides = HashMap::from([("pinned".to_string(), "v0".to_string())]);
        let next = next_forced_versions("root", &requirements, &items, &overrides, &HashMap::new());
        assert_eq!(next.get("one").map(String::as_str), Some("v1"));
        assert!(!next.contains_key("met"));
        assert!(!next.contains_key("multi"));
        assert!(!next.contains_key("pinned"));
        assert!(!next.contains_key("root"));

        let retained = next_forced_versions(
            "root",
            &requirements,
            &items,
            &overrides,
            &HashMap::from([("met".to_string(), "v1".to_string())]),
        );
        assert_eq!(retained.get("met").map(String::as_str), Some("v1"));
    }

    #[test]
    fn hint_conflicts_are_detected_against_hint_and_selected() {
        assert_eq!(
            hint_conflict(Some("a"), None, Some("b")),
            Some(("a".into(), "b".into()))
        );
        assert_eq!(
            hint_conflict(None, Some("a"), Some("b")),
            Some(("a".into(), "b".into()))
        );
        assert_eq!(hint_conflict(None, Some("a"), Some("a")), None);
        assert_eq!(hint_conflict(Some("a"), None, None), None);
        assert_eq!(hint_conflict(None, None, Some("a")), None);
    }

    #[test]
    fn pick_version_prefers_hint_then_release() {
        let beta = version("beta", "beta", Vec::new());
        let release = version("release", "release", Vec::new());
        let versions = vec![beta, release];

        assert_eq!(
            pick_version(
                &versions,
                None,
                Some("beta"),
                &InstalledFiles::for_test(&[])
            )
            .unwrap()
            .version_id,
            "beta"
        );
        assert_eq!(
            pick_version(&versions, None, None, &InstalledFiles::for_test(&[]))
                .unwrap()
                .version_id,
            "release"
        );
        assert!(pick_version(
            &versions,
            None,
            Some("missing"),
            &InstalledFiles::for_test(&[])
        )
        .is_err());
    }

    #[test]
    fn pick_version_override_wins_and_must_match() {
        let beta = version("beta", "beta", Vec::new());
        let release = version("release", "release", Vec::new());
        let versions = vec![beta, release];

        assert_eq!(
            pick_version(
                &versions,
                Some("beta"),
                Some("release"),
                &InstalledFiles::for_test(&[])
            )
            .unwrap()
            .version_id,
            "beta"
        );
        assert!(pick_version(
            &versions,
            Some("missing"),
            None,
            &InstalledFiles::for_test(&[])
        )
        .is_err());
    }

    #[test]
    fn pick_version_prefers_installed_when_unpinned() {
        let beta = version("beta", "beta", Vec::new());
        let release = version("release", "release", Vec::new());
        let versions = vec![beta, release];
        let installed = InstalledFiles::for_test(&["beta.jar"]);

        assert_eq!(
            pick_version(&versions, None, None, &installed)
                .unwrap()
                .version_id,
            "beta"
        );
    }

    #[test]
    fn pick_version_matches_cross_source_filename() {
        let mut cf = version("cf", "release", Vec::new());
        cf.filename = "fabric-api-0.166.2-26.3.jar".into();
        let versions = vec![cf];
        let installed = InstalledFiles::for_test(&["fabric-api-0.166.2+26.3.jar"]);

        assert_eq!(
            pick_version(&versions, None, None, &installed)
                .unwrap()
                .version_id,
            "cf"
        );
    }

    #[test]
    fn pick_version_keeps_installed_older_version() {
        let mut newer = version("newer", "release", Vec::new());
        newer.filename = "fabric-api-0.166.2+26.3.jar".into();
        let mut older = version("older", "release", Vec::new());
        older.filename = "fabric-api-0.165.0+26.3.jar".into();
        let installed = InstalledFiles::for_test(&["fabric-api-0.165.0+26.3.jar"]);

        assert_eq!(
            pick_version(&[newer, older], None, None, &installed)
                .unwrap()
                .version_id,
            "older",
            "不同版本按文件名区分；已安装的旧版本优先于最新版"
        );
    }

    #[test]
    fn version_supports_requires_matching_game_and_loader() {
        let mut item = version("v", "release", Vec::new());
        assert!(version_supports(&item, "1.20.1", "fabric"));
        assert!(!version_supports(&item, "1.21.1", "fabric"));
        assert!(!version_supports(&item, "1.20.1", "forge"));
        assert!(
            version_supports(&item, "1.20.1", "quilt"),
            "Quilt 可直接使用 Fabric 版本"
        );
        item.loaders.push("Quilt".into());
        assert!(version_supports(&item, "1.20.1", "quilt"));
    }

    #[tokio::test]
    async fn empty_mods_dir_skips_conflict_fetch() {
        let dir = tempfile::tempdir().unwrap();
        // source 非法：若未走空 mods 快速返回，这里会因 API 源错误而失败。
        let report = resolve_mod_conflicts(
            "unsupported-source",
            "project",
            "version",
            "1.20.1",
            "fabric",
            dir.path(),
        )
        .await
        .expect("空 mods 目录应直接返回");
        assert!(report.version_applicable);
        assert!(report.conflicts.is_empty());
        assert!(report.errors.is_empty());
    }

    #[test]
    fn installed_conflict_ignores_missing_and_disabled_files() {
        let mut active = version("active", "release", Vec::new());
        active.version_number = "1.0".into();
        active.date_published = "2026-01-01".into();
        active.filename = "sodium-dynamic-lights-1.0.jar".into();
        let mut disabled = version("disabled", "release", Vec::new());
        disabled.filename = "ryoamiclights-2.0.jar".into();
        let installed = InstalledFiles::for_test(&[
            "sodium-dynamic-lights-1.0.jar",
            "ryoamiclights-2.0.jar.disabled",
        ]);

        let (label, files) = installed_conflict(&[active, disabled], &installed).unwrap();
        assert_eq!(label, "1.0");
        assert_eq!(files, vec!["sodium-dynamic-lights-1.0.jar"]);

        let versions = vec![version("missing", "release", Vec::new())];
        assert!(
            installed_conflict(&versions, &InstalledFiles::for_test(&[])).is_none(),
            "未安装的冲突项目不应产生提示"
        );
    }

    #[test]
    fn installed_conflict_matches_renamed_file_by_sha1() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("renamed.jar"), b"abc").unwrap();
        std::fs::write(dir.path().join("renamed.jar.disabled"), b"abc").unwrap();
        let mut item = version("hash", "release", Vec::new());
        item.version_number = "1.0".into();
        item.date_published = "2026-01-01".into();
        item.filename = "original-name.jar".into();
        item.file_size = 3;
        item.sha1 = Some("a9993e364706816aba3e25717850c26c9cd0d89d".into());

        let (label, files) = installed_conflict_by_hash(&[item], dir.path()).unwrap();
        assert_eq!(label, "1.0");
        assert_eq!(
            files,
            vec!["renamed.jar"],
            "文件名不同时应按 sha1 命中，且忽略 .disabled"
        );
    }

    #[test]
    fn candidates_include_selected_outside_limit() {
        let mut versions = Vec::new();
        for index in 0..(MAX_DEPENDENCY_CANDIDATES + 5) {
            let mut item = version(&format!("v{index}"), "release", Vec::new());
            item.date_published = format!("2026-01-{:02}", index + 1);
            versions.push(item);
        }
        let (candidates, total) = build_candidates(&versions, "v0");

        assert_eq!(total, versions.len());
        assert_eq!(candidates.len(), MAX_DEPENDENCY_CANDIDATES + 1);
        assert_eq!(candidates[0].version_id, format!("v{}", versions.len() - 1));
        assert!(candidates.iter().any(|item| item.version_id == "v0"));
    }

    #[test]
    fn seeds_required_and_optional_with_optional_root() {
        let root = version(
            "root",
            "release",
            vec![
                dependency("required", "required", Some("v1")),
                dependency("optional", "optional", None),
                dependency("incompatible", "incompatible", None),
            ],
        );
        let mut required = VecDeque::new();
        let mut optional = VecDeque::new();
        seed_dependencies(&root, None, &mut required, &mut optional);

        assert_eq!(required.len(), 1);
        assert_eq!(required[0].project_id, "required");
        assert!(required[0].root_optional.is_none());
        assert_eq!(required[0].version_hint.as_deref(), Some("v1"));

        assert_eq!(optional.len(), 1);
        assert_eq!(optional[0].project_id, "optional");
        assert_eq!(optional[0].root_optional.as_deref(), Some("optional"));
    }

    #[test]
    fn installed_files_reads_mods_dir_and_normalizes_separators() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jar"), b"a").unwrap();
        std::fs::write(dir.path().join("b.jar.disabled"), b"b").unwrap();
        std::fs::write(dir.path().join("fabric-api-0.166.2+26.3.jar"), b"c").unwrap();
        let files = InstalledFiles::from_mods_dir(dir.path());
        assert!(files.contains("a.jar"));
        assert!(files.contains("b.jar.disabled"));
        // 跨源文件名：Modrinth `+` vs CurseForge `-`
        assert!(files.contains("fabric-api-0.166.2-26.3.jar"));
        assert_eq!(
            files.actual_name("fabric-api-0.166.2-26.3.jar"),
            Some("fabric-api-0.166.2+26.3.jar")
        );
    }

    #[test]
    fn installed_files_keep_actual_disk_name_for_replacement() {
        let installed = InstalledFiles::for_test(&["fabric-api-0.166.2+26.3.jar"]);
        let mut old = version("old", "release", Vec::new());
        old.filename = "fabric-api-0.166.2-26.3.jar".into();
        let names = installed.installed_names(&[old]);
        assert_eq!(names, vec!["fabric-api-0.166.2+26.3.jar".to_string()]);
    }
}

#[cfg(test)]
mod real_tests {
    use std::collections::HashMap;

    use super::super::modrinth::modrinth_versions;
    use super::{resolve_mod_conflicts, resolve_mod_dependencies};

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "需要真实 Modrinth API"]
    async fn resolves_real_modrinth_dependencies() {
        let versions = modrinth_versions("YL57xq9U", Some("1.20.1"), Some("fabric"), false)
            .await
            .expect("拉取 Iris 版本");
        let version = versions
            .iter()
            .find(|version| version.version_type == "release")
            .or_else(|| versions.first())
            .expect("Iris 应有 1.20.1 Fabric 版本");
        let dir = tempfile::tempdir().unwrap();
        let plan = resolve_mod_dependencies(
            "modrinth",
            "YL57xq9U",
            &version.version_id,
            "1.20.1",
            "fabric",
            &HashMap::new(),
            dir.path(),
        )
        .await
        .expect("解析前置");
        assert!(plan.errors.is_empty(), "{}", plan.errors.join("; "));
        assert!(!plan.items.is_empty(), "Iris 应有前置 Mod");
        println!("items: {:#?}", plan.items);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "需要真实 Modrinth API"]
    async fn conflicts_only_when_installed_and_applicable() {
        let dir = tempfile::tempdir().unwrap();
        let report = resolve_mod_conflicts(
            "modrinth",
            "yBW8D80W",
            "ksaGCvSu",
            "1.21.1",
            "fabric",
            dir.path(),
        )
        .await
        .expect("检查冲突");
        assert!(report.version_applicable);
        assert!(
            report.conflicts.is_empty(),
            "未安装冲突 Mod 时不应提示: {:?}",
            report.conflicts
        );

        std::fs::write(
            dir.path()
                .join("sodiumdynamiclights-fabric-1.0.10-1.21.1.jar"),
            b"x",
        )
        .unwrap();
        let report = resolve_mod_conflicts(
            "modrinth",
            "yBW8D80W",
            "ksaGCvSu",
            "1.21.1",
            "fabric",
            dir.path(),
        )
        .await
        .expect("检查冲突");
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(report.conflicts[0].name, "Sodium Dynamic Lights");
        assert!(
            report.conflicts[0]
                .installed_files
                .contains(&"sodiumdynamiclights-fabric-1.0.10-1.21.1.jar".to_string()),
            "应报告命中的磁盘文件: {:?}",
            report.conflicts[0].installed_files
        );

        let report = resolve_mod_conflicts(
            "modrinth",
            "yBW8D80W",
            "rylKnJ4M",
            "26.3",
            "fabric",
            dir.path(),
        )
        .await
        .expect("检查冲突");
        assert!(report.version_applicable);
        assert!(
            report.conflicts.is_empty(),
            "冲突 Mod 没有目标 MC 版本时不应提示: {:?}",
            report.conflicts
        );

        let plan = resolve_mod_dependencies(
            "modrinth",
            "yBW8D80W",
            "ksaGCvSu",
            "1.21.1",
            "quilt",
            &HashMap::new(),
            dir.path(),
        )
        .await
        .expect("Quilt 解析前置");
        let fabric_api = plan
            .items
            .iter()
            .find(|item| item.project_id == "P7dR8mSH")
            .expect("LambDynamicLights 应有 Fabric API 前置");
        assert!(
            fabric_api.error.is_none(),
            "Quilt 应能使用 Fabric 版前置: {:?}",
            fabric_api.error
        );
    }
}
