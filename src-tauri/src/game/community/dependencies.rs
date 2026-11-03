//! 前置 Mod 解析：从主 Mod 版本出发递归展开 required/optional 依赖，按目标实例的 MC/loader
//! 选兼容版本；`visited` 去重与去环，节点上限 512。

use futures_util::stream::{self, StreamExt};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::api::CommunityVersion;
use super::{curseforge, modrinth};

/// 节点上限：正常整合包远低于此；仅拦截异常/恶意元数据。
const MAX_DEPENDENCY_NODES: usize = 512;
/// 候选上限：控制宽依赖树的计划体积；当前选中版本始终保留。
const MAX_DEPENDENCY_CANDIDATES: usize = 40;
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
    let installed = installed_filenames(mods_dir);
    let mut installed_files: Vec<String> = installed.iter().cloned().collect();
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
            let installed_files: Vec<String> = versions
                .iter()
                .filter(|version| installed.contains(&version.filename))
                .map(|version| version.filename.clone())
                .collect();
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
    let versions = match source {
        "modrinth" => modrinth::modrinth_versions(project_id, Some(mc_version), Some(loader)).await,
        "curseforge" => {
            curseforge::curseforge_versions(project_id, Some(mc_version), Some(loader)).await
        }
        other => {
            return Err(keyed_msg!(
                "errors.community.unsupportedSource",
                "不支持的资源来源\n{other}"
            ))
        }
    }?;
    Ok(versions
        .into_iter()
        .filter(|version| version_supports(version, mc_version, loader))
        .collect())
}

/// 平台过滤之外再校验一次，确保候选只属于实例的 MC/loader 组合。
fn version_supports(version: &CommunityVersion, mc_version: &str, loader: &str) -> bool {
    version
        .game_versions
        .iter()
        .any(|value| value == mc_version)
        && version
            .loaders
            .iter()
            .any(|value| value.eq_ignore_ascii_case(loader))
}

/// 版本覆盖优先，其次精确 `version_hint`；否则 release 优先，最后取第一个。
fn pick_version<'a>(
    versions: &'a [CommunityVersion],
    override_id: Option<&str>,
    hint: Option<&str>,
    installed: &HashSet<String>,
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

fn installed_filenames(mods_dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect()
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
        "modrinth" => modrinth::modrinth_projects_raw(&ids)
            .await
            .map(|projects| {
                projects
                    .iter()
                    .filter_map(|project| {
                        Some((
                            project["id"].as_str()?.to_string(),
                            project["title"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        "curseforge" => curseforge::curseforge_details(&ids)
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
            pick_version(&versions, None, Some("beta"), &HashSet::new())
                .unwrap()
                .version_id,
            "beta"
        );
        assert_eq!(
            pick_version(&versions, None, None, &HashSet::new())
                .unwrap()
                .version_id,
            "release"
        );
        assert!(pick_version(&versions, None, Some("missing"), &HashSet::new()).is_err());
    }

    #[test]
    fn pick_version_override_wins_and_must_match() {
        let beta = version("beta", "beta", Vec::new());
        let release = version("release", "release", Vec::new());
        let versions = vec![beta, release];

        assert_eq!(
            pick_version(&versions, Some("beta"), Some("release"), &HashSet::new())
                .unwrap()
                .version_id,
            "beta"
        );
        assert!(pick_version(&versions, Some("missing"), None, &HashSet::new()).is_err());
    }

    #[test]
    fn pick_version_prefers_installed_when_unpinned() {
        let beta = version("beta", "beta", Vec::new());
        let release = version("release", "release", Vec::new());
        let versions = vec![beta, release];
        let installed = HashSet::from(["beta.jar".to_string()]);

        assert_eq!(
            pick_version(&versions, None, None, &installed)
                .unwrap()
                .version_id,
            "beta"
        );
    }

    #[test]
    fn version_supports_requires_matching_game_and_loader() {
        let mut item = version("v", "release", Vec::new());
        assert!(version_supports(&item, "1.20.1", "fabric"));
        assert!(!version_supports(&item, "1.21.1", "fabric"));
        assert!(!version_supports(&item, "1.20.1", "forge"));
        item.loaders.push("Quilt".into());
        assert!(version_supports(&item, "1.20.1", "quilt"));
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
    fn installed_filenames_reads_mods_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jar"), b"a").unwrap();
        std::fs::write(dir.path().join("b.jar.disabled"), b"b").unwrap();
        let names = installed_filenames(dir.path());
        assert!(names.contains("a.jar"));
        assert!(names.contains("b.jar.disabled"));
    }
}

#[cfg(test)]
mod real_tests {
    use std::collections::HashMap;

    use super::super::modrinth::modrinth_versions;
    use super::resolve_mod_dependencies;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "需要真实 Modrinth API"]
    async fn resolves_real_modrinth_dependencies() {
        let versions = modrinth_versions("YL57xq9U", Some("1.20.1"), Some("fabric"))
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
}
