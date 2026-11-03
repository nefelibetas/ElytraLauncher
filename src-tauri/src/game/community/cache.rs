//! 社区版本列表的进程内 TTL 缓存：重复打开下载弹窗/依赖解析时跳过相同 API。

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::api::{CommunityDependencyItem, CommunityVersion};

/// 版本列表 TTL：作者发布新版本不频繁；刷新/重开超过窗口后自然回源。
const TTL: Duration = Duration::from_secs(60);
/// 缓存条目上限，避免浏览大量 Mod 后无限增长。
const MAX_ENTRIES: usize = 128;

type Key = (String, String, String, String);
type CacheEntry = (Instant, Vec<CommunityVersion>);

static CACHE: LazyLock<Mutex<HashMap<Key, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(source: &str, project_id: &str, game_version: Option<&str>, loader: Option<&str>) -> Key {
    (
        source.to_string(),
        project_id.to_string(),
        game_version.unwrap_or_default().to_string(),
        loader.unwrap_or_default().to_string(),
    )
}

pub(crate) fn get_version(
    source: &str,
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
) -> Option<Vec<CommunityVersion>> {
    get_version_at(source, project_id, game_version, loader, Instant::now())
}

fn get_version_at(
    source: &str,
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    now: Instant,
) -> Option<Vec<CommunityVersion>> {
    let key = key(source, project_id, game_version, loader);
    let mut cache = CACHE.lock();
    let hit = cache.get(&key)?;
    if now.saturating_duration_since(hit.0) <= TTL {
        return Some(hit.1.clone());
    }
    cache.remove(&key);
    None
}

pub(crate) fn put_version(
    source: &str,
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    versions: &[CommunityVersion],
) {
    put_version_at(
        source,
        project_id,
        game_version,
        loader,
        versions,
        Instant::now(),
    );
}

fn put_version_at(
    source: &str,
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    versions: &[CommunityVersion],
    at: Instant,
) {
    let mut cache = CACHE.lock();
    if cache.len() >= MAX_ENTRIES {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(
        key(source, project_id, game_version, loader),
        (at, versions.to_vec()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: &str) -> CommunityVersion {
        CommunityVersion {
            version_id: id.into(),
            version_number: id.into(),
            game_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            filename: format!("{id}.jar"),
            download_url: String::new(),
            file_size: 0,
            sha1: None,
            date_published: String::new(),
            downloads: 0,
            version_type: "release".into(),
            dependencies: Vec::new(),
        }
    }

    fn detail(id: &str) -> CommunityDependencyItem {
        CommunityDependencyItem {
            id: id.into(),
            title: format!("title-{id}"),
            description: String::new(),
            icon_url: None,
            translated: None,
        }
    }

    #[test]
    fn detail_cache_keeps_requested_order() {
        let source = format!("detail-{}", std::process::id());
        put_details(&source, &[detail("a"), detail("b")]);
        let ids = vec!["b".to_string(), "c".to_string()];
        let (cached, missing) = get_details(&source, &ids);
        assert_eq!(
            cached.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["b"]
        );
        assert_eq!(missing, ["c"]);
        let merged = merge_details(&ids, cached, vec![detail("c"), detail("a")]);
        assert_eq!(
            merged.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["b", "c"]
        );
    }

    #[test]
    fn expired_entry_is_dropped() {
        let source = format!("expired-{}", std::process::id());
        let old = Instant::now() - TTL - Duration::from_secs(1);
        put_version_at(&source, "a", None, None, &[version("a1")], old);
        assert!(get_version(&source, "a", None, None).is_none());
    }

    #[test]
    fn version_cache_roundtrips_and_evicts_oldest() {
        let source = format!("test-{}", std::process::id());
        put_version(
            &source,
            "a",
            Some("1.20.1"),
            Some("fabric"),
            &[version("a1")],
        );
        let hit = get_version(&source, "a", Some("1.20.1"), Some("fabric")).expect("cache hit");
        assert_eq!(hit[0].version_id, "a1");
        assert!(get_version(&source, "a", Some("1.21.1"), Some("fabric")).is_none());

        for i in 0..MAX_ENTRIES {
            put_version(&source, &format!("p{i}"), None, None, &[]);
        }
        // 第一批写入的 a 应被容量淘汰；仍能接受当前条目。
        assert!(get_version(&source, "a", Some("1.20.1"), Some("fabric")).is_none());
    }
}

type DetailKey = (String, String);
type DetailEntry = (Instant, CommunityDependencyItem);

static DETAIL_CACHE: LazyLock<Mutex<HashMap<DetailKey, DetailEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const MAX_DETAILS: usize = 256;

/// 返回已缓存详情（保持请求顺序）与需要回源的 ID。
pub(crate) fn get_details(
    source: &str,
    ids: &[String],
) -> (Vec<CommunityDependencyItem>, Vec<String>) {
    let now = Instant::now();
    let mut cache = DETAIL_CACHE.lock();
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for id in ids {
        let key = (source.to_string(), id.clone());
        match cache.get(&key) {
            Some((at, item)) if now.saturating_duration_since(*at) <= TTL => {
                found.push(item.clone());
            }
            Some(_) => {
                cache.remove(&key);
                missing.push(id.clone());
            }
            None => missing.push(id.clone()),
        }
    }
    (found, missing)
}

pub(crate) fn put_details(source: &str, items: &[CommunityDependencyItem]) {
    let now = Instant::now();
    let mut cache = DETAIL_CACHE.lock();
    for item in items {
        if cache.len() >= MAX_DETAILS {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert((source.to_string(), item.id.clone()), (now, item.clone()));
    }
}

/// 按请求顺序把缓存项与本次回源项合并；缺失项自然省略。
pub(crate) fn merge_details(
    ids: &[String],
    cached: Vec<CommunityDependencyItem>,
    fetched: Vec<CommunityDependencyItem>,
) -> Vec<CommunityDependencyItem> {
    let mut by_id: HashMap<String, CommunityDependencyItem> = cached
        .into_iter()
        .chain(fetched)
        .map(|item| (item.id.clone(), item))
        .collect();
    ids.iter().filter_map(|id| by_id.remove(id)).collect()
}
