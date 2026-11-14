//! 版本目录名 / Java 版本字符串解析（与前端 versionClassify 同规则）。

use serde_json::Value;

/// loader 标记 key（与前端 utils/versionClassify 的 LOADER_KEYS 一致；互不重叠，顺序不影响结果）。
const LOADER_KEYS: &[&str] = &["neoforge", "forge", "fabric", "quilt", "optifine"];

/// loader 判定优先级：Quilt/NeoForge 的 json 都含 `net.fabricmc` 系库（sponge-mixin 等），
/// 只能按坐标/参数精确匹配，fabric 不得依赖包名前缀先命中。
const LOADER_PRIORITY: [&str; 5] = ["quilt", "neoforge", "forge", "fabric", "optifine"];

/// 剥离冲突重命名尾部 `_n` 后缀（同前端 /_\d+$/）：只剥尾部一段 `_<数字>`
/// （`26.2-forge-65.1.0_1` → `26.2-forge-65.1.0`；`x_1_2` → `x_1`；`x_` 不剥）。
fn strip_conflict_suffix(name: &str) -> &str {
    if let Some(i) = name.rfind('_') {
        let tail = &name[i + 1..];
        if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            return &name[..i];
        }
    }
    name
}

/// 大小写不敏感查找 `-{key}-` loader 标记（标记前必须还有内容，idx > 0）。
/// ASCII 小写化不改变字节长度，返回的 idx 可直接回用于原串切片。
fn find_loader_marker(base: &str) -> Option<(usize, &'static str)> {
    let lower = base.to_ascii_lowercase();
    for key in LOADER_KEYS {
        let marker = format!("-{key}-");
        if let Some(idx) = lower.find(&marker) {
            if idx > 0 {
                return Some((idx, key));
            }
        }
    }
    None
}

/// 从版本目录名提取 MC 版本（与前端 utils/versionClassify 同规则）：找 `-{loader_key}-` 标记（大小写不敏感）
/// 取标记前部分；无标记（vanilla）取整名；尾部 `_数字` 剥离一次。**仅限展示与本地键**（natives 共享目录等）；
/// 对外真实 MC 版本应解析版本 json（commands/version.rs::resolve_version_meta），目录名解析只是兜底。
pub fn mc_version_from_dir(dir_name: &str) -> &str {
    let base = strip_conflict_suffix(dir_name);
    match find_loader_marker(base) {
        Some((idx, _)) => &base[..idx],
        None => base,
    }
}

/// 从版本目录名解析 ModLoader：(loader_key, loader_version)。
/// `26.2-fabric-0.16.9` → Some(("fabric", "0.16.9"))；vanilla → None；大小写不敏感（PCL2 外部目录可能大写）。
/// optifine 也识别（导出 mrpack 时忽略——dependencies 无 OptiFine 标准键，本体已在 overrides 里）。
pub fn loader_from_dir(dir_name: &str) -> Option<(&str, &str)> {
    let base = strip_conflict_suffix(dir_name);
    let (idx, key) = find_loader_marker(base)?;
    Some((key, &base[idx + key.len() + 2..]))
}

/// 从版本字符串解析主版本号。
/// 支持格式："17.0.8" → 17, "21" → 21, "25-ea" → 25, "1.8.0_202" → 8
pub fn parse_java_major(version: &str) -> Option<u32> {
    // "1.x" 格式（Java 8 及更早，如 "1.8.0_202" → 8）
    if let Some(v) = version.strip_prefix("1.") {
        return v.split('.').next()?.parse::<u32>().ok();
    }
    // 新格式："17.0.8" → 17
    version
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse::<u32>().ok())
}

/// 从版本 json 文本推断 loader：解析后按 mainClass / 库坐标 / FML 参数精确判定
/// （Quilt/NeoForge 均含 `net.fabricmc:sponge-mixin`，字符串前缀扫描会误判成 Fabric）；
/// 合法 JSON 无 loader 特征时按 `id` 的目录名约定兜底，文本不可解析才退回子串扫描；无特征视为原版。
pub fn infer_loader_from_json(text: &str) -> String {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => loader_from_value(&value)
            .or_else(|| {
                value
                    .get("id")
                    .and_then(|id| id.as_str())
                    .and_then(loader_from_dir)
                    .map(|(kind, _)| kind)
            })
            .unwrap_or("vanilla")
            .to_string(),
        Err(_) => infer_loader_from_text(text),
    }
}

/// 收集全部 loader 特征后按 [`LOADER_PRIORITY`] 取最高者（递归覆盖 HMCL `patches[]` 等嵌套清单）。
fn loader_from_value(value: &Value) -> Option<&'static str> {
    let mut hits = [false; LOADER_PRIORITY.len()];
    collect_loader_signatures(value, &mut hits);
    LOADER_PRIORITY
        .iter()
        .zip(hits)
        .find(|(_, hit)| *hit)
        .map(|(kind, _)| *kind)
}

fn collect_loader_signatures(value: &Value, hits: &mut [bool; LOADER_PRIORITY.len()]) {
    match value {
        Value::Object(map) => {
            if let Some(main) = map.get("mainClass").and_then(|v| v.as_str()) {
                mark_loader(hits, classify_main_class(main));
            }
            if let Some(libs) = map.get("libraries").and_then(|v| v.as_array()) {
                for name in libs
                    .iter()
                    .filter_map(|lib| lib.get("name").and_then(|v| v.as_str()))
                {
                    mark_loader(hits, classify_coordinate(name));
                }
            }
            for child in map.values() {
                collect_loader_signatures(child, hits);
            }
        }
        Value::Array(list) => {
            for child in list {
                collect_loader_signatures(child, hits);
            }
        }
        Value::String(text) => mark_loader(hits, classify_fml_arg(text)),
        _ => {}
    }
}

fn mark_loader(hits: &mut [bool; LOADER_PRIORITY.len()], kind: Option<&'static str>) {
    if let Some(kind) = kind {
        if let Some(index) = LOADER_PRIORITY.iter().position(|key| *key == kind) {
            hits[index] = true;
        }
    }
}

/// 主类前缀判定；Forge/NeoForge 21.x 都用 `cpw.mods.bootstraplauncher`，只能靠库坐标区分。
fn classify_main_class(main: &str) -> Option<&'static str> {
    if main.starts_with("org.quiltmc") {
        Some("quilt")
    } else if main.starts_with("net.neoforged") {
        Some("neoforge")
    } else if main.starts_with("net.minecraftforge") {
        Some("forge")
    } else if main.starts_with("net.fabricmc") {
        Some("fabric")
    } else if main.to_ascii_lowercase().contains("optifine") {
        Some("optifine")
    } else {
        None
    }
}

/// 库坐标按 `group:artifact` 精确匹配；NeoForge 的 Mixin 分支是 `net.fabricmc:sponge-mixin`，
/// 不能匹配成 fabric（fabric 只认 `fabric-loader`）。
fn classify_coordinate(name: &str) -> Option<&'static str> {
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    if group == "net.neoforged" || group.starts_with("net.neoforged.") {
        return Some("neoforge");
    }
    match (group, artifact) {
        ("org.quiltmc", "quilt-loader") => Some("quilt"),
        ("net.fabricmc", "fabric-loader") => Some("fabric"),
        ("net.minecraftforge", "forge" | "fmlloader" | "minecraftforge") => Some("forge"),
        // 纯 OptiFine 自包含 json 的库坐标 optifine:OptiFine:<patch>
        ("optifine", _) => Some("optifine"),
        _ => None,
    }
}

/// Forge/NeoForge installer 在 game 参数里写自己的版本号（21.x 主类相同，参数是可靠区分点）。
fn classify_fml_arg(arg: &str) -> Option<&'static str> {
    match arg {
        "--fml.neoForgeVersion" => Some("neoforge"),
        "--fml.forgeVersion" => Some("forge"),
        _ => None,
    }
}

/// 非 JSON 文本兜底：NeoForge 的文本同样含 net.fabricmc，fabric 必须最后判。
fn infer_loader_from_text(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let kind = if lower.contains("org.quiltmc") {
        "quilt"
    } else if lower.contains("net.neoforged") {
        "neoforge"
    } else if lower.contains("net.minecraftforge") {
        "forge"
    } else if lower.contains("net.fabricmc") {
        "fabric"
    } else if lower.contains("optifine") {
        "optifine"
    } else {
        "vanilla"
    };
    kind.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_version_from_dir_all_naming_forms() {
        assert_eq!(mc_version_from_dir("26.2"), "26.2");
        assert_eq!(mc_version_from_dir("26.2-forge-65.1.0"), "26.2");
        assert_eq!(mc_version_from_dir("26.2-neoforge-26.2.0.53-beta"), "26.2");
        assert_eq!(
            mc_version_from_dir("1.20.1-pre1-forge-40.1.0"),
            "1.20.1-pre1"
        );
        assert_eq!(mc_version_from_dir("1.18.2-fabric-0.16.11"), "1.18.2");
        assert_eq!(mc_version_from_dir("1.12.2"), "1.12.2");
        assert_eq!(mc_version_from_dir("26.2-forge-65.1.0_1"), "26.2");
        // 与前端 classifyVersion 对齐：标记大小写不敏感（PCL2 等外部目录名大写）
        assert_eq!(mc_version_from_dir("1.20.1-Fabric-0.14.22"), "1.20.1");
        assert_eq!(mc_version_from_dir("1.20.1-FABRIC-0.14.22"), "1.20.1");
        // 尾部 `_数字` 只剥一段（与前端 /_\d+$/ 一致）
        assert_eq!(mc_version_from_dir("1.20.1_1_2"), "1.20.1_1");
        // 任意名无标记：整串兜底（vanilla 语义，仅供展示/本地键）
        assert_eq!(mc_version_from_dir("[abcd]"), "[abcd]");
        assert_eq!(mc_version_from_dir("我的整合包-fabric-1.0"), "我的整合包");
    }

    /// loader 解析与前端同规则（大小写不敏感 + 尾部冲突后缀）；解析器信任目录名，
    /// 对外真实语义由版本 json 权威层兜底（commands/version.rs::resolve_version_meta）。
    #[test]
    fn loader_from_dir_parity_forms() {
        assert_eq!(
            loader_from_dir("26.2-fabric-0.16.9"),
            Some(("fabric", "0.16.9"))
        );
        assert_eq!(
            loader_from_dir("1.20.1-Fabric-0.14.22"),
            Some(("fabric", "0.14.22"))
        );
        assert_eq!(
            loader_from_dir("26.2-forge-65.1.0_1"),
            Some(("forge", "65.1.0"))
        );
        assert_eq!(loader_from_dir("26.2"), None);
        assert_eq!(loader_from_dir("[abcd]"), None);
        // 伪标记示例：解析器原样给出，json 权威层会覆盖为真实 loader
        assert_eq!(loader_from_dir("X-fabric-Y"), Some(("fabric", "Y")));
    }

    #[test]
    fn test_parse_java_major() {
        assert_eq!(parse_java_major("1.8.0_202"), Some(8));
        assert_eq!(parse_java_major("1.7.0"), Some(7));
        assert_eq!(parse_java_major("1.6.0"), Some(6));
        assert_eq!(parse_java_major("17.0.8"), Some(17));
        assert_eq!(parse_java_major("21"), Some(21));
        assert_eq!(parse_java_major("25-ea"), Some(25));
        assert_eq!(parse_java_major("11.0.2"), Some(11));
    }

    #[test]
    fn infer_loader_variants() {
        assert_eq!(
            infer_loader_from_json(
                r#"{"mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient"}"#
            ),
            "fabric"
        );
        assert_eq!(
            infer_loader_from_json(r#"{"libraries":[{"name":"net.minecraftforge:forge:47.1.0"}]}"#),
            "forge"
        );
        assert_eq!(
            infer_loader_from_json(r#"{"libraries":[{"name":"net.neoforged:neoforge:21.0.0"}]}"#),
            "neoforge"
        );
        // NeoForge 1.20.1 线用 net.neoforged:forge，不能判成 Forge
        assert_eq!(
            infer_loader_from_json(r#"{"libraries":[{"name":"net.neoforged:forge:20.1.0"}]}"#),
            "neoforge"
        );
        assert_eq!(
            infer_loader_from_json(r#"{"mainClass":"net.neoforged.bootstrap.BootstrapLauncher"}"#),
            "neoforge"
        );
        assert_eq!(
            infer_loader_from_json(r#"{"mainClass":"net.minecraft.client.main.Main"}"#),
            "vanilla"
        );
        // Quilt json 同时含 net.fabricmc（sponge-mixin/intermediary）与 org.quiltmc，必须判 quilt 而非 fabric
        assert_eq!(
            infer_loader_from_json(
                r#"{"mainClass":"org.quiltmc.loader.impl.launch.knot.KnotClient","libraries":[{"name":"org.quiltmc:quilt-loader:0.23.1"},{"name":"net.fabricmc:sponge-mixin:0.12.5"}]}"#
            ),
            "quilt"
        );
        // 纯 OptiFine 自包含 json 的库坐标
        assert_eq!(
            infer_loader_from_json(
                r#"{"libraries":[{"name":"optifine:OptiFine:1.20.1_HD_U_I6"}]}"#
            ),
            "optifine"
        );
    }

    /// NeoForge/Forge 21.x 的 mainClass 同为 `cpw.mods.bootstraplauncher.BootstrapLauncher`；
    /// NeoForge 的库列表还含 `net.fabricmc:sponge-mixin`，只能靠 `net.neoforged*` 坐标与 FML 参数区分。
    #[test]
    fn infer_loader_neoforge_not_fabric() {
        let neoforge = r#"{
            "id": "1.21.1-neoforge-21.1.266",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "arguments": {"game": ["--fml.neoForgeVersion", "21.1.266", "--launchTarget", "forgeclient"]},
            "libraries": [
                {"name": "net.neoforged.fancymodloader:loader:4.0.45"},
                {"name": "net.neoforged:bus:8.0.5"},
                {"name": "net.fabricmc:sponge-mixin:0.16.4+mixin.0.8.7"}
            ]
        }"#;
        assert_eq!(infer_loader_from_json(neoforge), "neoforge");

        // 26.x 换了主类，同样不得判成 fabric
        let modern = r#"{
            "id": "26.2-neoforge-26.2.0.53",
            "mainClass": "net.neoforged.fml.startup.Client",
            "libraries": [{"name": "net.fabricmc:sponge-mixin:0.17.4+mixin.0.8.7"}]
        }"#;
        assert_eq!(infer_loader_from_json(modern), "neoforge");

        // 同形态的 Forge 版本（bootstraplauncher + fmlloader + mergetool）仍判 forge
        let forge = r#"{
            "id": "1.20.1-forge-47.1.0",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "arguments": {"game": ["--fml.forgeVersion", "47.1.0"]},
            "libraries": [
                {"name": "net.minecraftforge:fmlloader:1.20.1-47.1.0"},
                {"name": "net.minecraftforge:mergetool:1.1.5:api"}
            ]
        }"#;
        assert_eq!(infer_loader_from_json(forge), "forge");
    }

    #[test]
    fn infer_loader_fallbacks() {
        // 合法 JSON 但无 loader 特征：按 id 的目录名约定兜底
        assert_eq!(
            infer_loader_from_json(r#"{"id":"26.2-neoforge-26.2.0.53"}"#),
            "neoforge"
        );
        // 非 JSON 文本退回子串扫描：NeoForge 文本也含 net.fabricmc，fabric 必须最后判
        assert_eq!(
            infer_loader_from_json(
                r#"{"libraries":[{"name":"net.fabricmc:sponge-mixin"},{"name":"net.neoforged:bus"#
            ),
            "neoforge"
        );
    }
}
