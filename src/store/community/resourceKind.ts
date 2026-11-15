// 社区资源入口 ↔ 版本文件类型判定（单一事实源）。Rust 侧 MOD_LOADERS 见
// src-tauri/src/game/community/modrinth.rs，新增加载器族时两边同步。
import type { DownloadTab } from "../task/downloadTab";

type LoaderFamily = "mod" | "plugin" | "datapack" | "shader" | "resourcepack";
type FamilyMap = Partial<Record<LoaderFamily, true>>;

/** 加载器标签 → 类别。未知标签返回 undefined。 */
const LOADER_FAMILY: Record<string, LoaderFamily> = {
  fabric: "mod",
  forge: "mod",
  neoforge: "mod",
  quilt: "mod",
  bukkit: "plugin",
  paper: "plugin",
  purpur: "plugin",
  spigot: "plugin",
  folia: "plugin",
  velocity: "plugin",
  bungeecord: "plugin",
  waterfall: "plugin",
  datapack: "datapack",
  // iris/optifine/canvas 是光影加载器；optifine 也常作为资源包兼容标签
  iris: "shader",
  optifine: "shader",
  canvas: "shader",
  vanilla: "shader",
  minecraft: "resourcepack",
};

/** 该标签是否属于可进 mods/ 的游戏加载器。 */
export function isModLoader(loader: string): boolean {
  return LOADER_FAMILY[loader.toLowerCase()] === "mod";
}

/** 各入口允许的加载器族；null = 不按加载器过滤（项目类型本身已限定）。 */
const ALLOWED_FAMILIES: Record<DownloadTab, FamilyMap | null> = {
  vanilla: null,
  mod: { mod: true },
  datapack: { datapack: true },
  // OptiFine 同时是资源包兼容标签；允许 shader 族避免误杀
  resourcepack: { resourcepack: true, shader: true },
  // minecraft 标签偶尔出现在光影文件；允许 resourcepack 族避免误杀
  shader: { shader: true, resourcepack: true },
  // 整合包版本 loaders 表示包内加载器（fabric/forge…），不是文件类型
  modpack: { mod: true },
  worlds: null,
};

/** 版本行展示的加载器族；空表 = 不显示徽章（入口本身已说明类型）。 */
const DISPLAY_FAMILIES: Record<DownloadTab, FamilyMap | null> = {
  vanilla: null,
  mod: { mod: true },
  datapack: {},
  resourcepack: {},
  shader: { shader: true },
  modpack: { mod: true },
  worlds: {},
};

/**
 * 版本文件是否属于当前下载入口。空 loaders 视为未知保留；已知标签但只要有一个
 * 命中允许族就保留（混版文件仍有可用形态）；全部是未知标签时也保留，避免误杀旧文件。
 */
export function versionMatchesKind(kind: DownloadTab, loaders: string[]): boolean {
  if (loaders.length === 0) return true;
  const allowed = ALLOWED_FAMILIES[kind];
  if (!allowed) return true;
  const families = loaders.map(loader => LOADER_FAMILY[loader.toLowerCase()] ?? null);
  if (families.some(family => family !== null && allowed[family] === true)) {
    return true;
  }
  return families.every(family => family === null);
}

/** 当前入口下应展示的加载器（过滤掉插件/数据包等不属于该目录的标签）。 */
export function displayLoadersForKind(kind: DownloadTab, loaders: string[]): string[] {
  const display = DISPLAY_FAMILIES[kind];
  if (!display) return loaders;
  return loaders.filter(loader => {
    const family = LOADER_FAMILY[loader.toLowerCase()];
    return family !== undefined && display[family] === true;
  });
}
