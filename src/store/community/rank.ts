/** 社区资源聚合排序：纯函数，无 Vue/IPC 依赖，便于单独验证评分与去重键。 */
export type CommunitySource = "modrinth" | "curseforge";
/** 排序口径：relevance（综合）/ downloads / updated。 */
export type CommunitySort = "relevance" | "downloads" | "updated";

/** scoreItem 需要的最小字段；CommunityDisplayItem 结构满足。 */
export interface RankableItem {
  title: string;
  title_zh: string | null;
  mcmod_id: number | null;
  downloads: number;
  /** 最后更新时间 ISO 8601；缺失为 null/undefined。 */
  updated_at?: string | null;
  source: CommunitySource;
}

/** 归一化：小写并去掉所有非字母数字字符；中文等 Unicode 字母保留。 */
export function normalizeText(value: string): string {
  return value.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
}

function tokenize(value: string): string[] {
  return value
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/**
 * 标题相关度 0..1：完整 > 前缀 > 连续短语 > 按词序覆盖。
 * 不做逐词包含：CF 的 "Cloth Config API (Fabric/Forge/NeoForge)" 会同时含 fabric/api，
 * 但两者被斜杠隔开，不能当成 "fabric api" 的短语命中。
 */
export function titleRelevance(title: string, titleZh: string | null, query: string): number {
  const q = normalizeText(query);
  if (!q) return 0;
  const qTokens = tokenize(query);
  let best = 0;
  for (const candidate of [title, titleZh ?? ""]) {
    const normalized = normalizeText(candidate);
    if (!normalized) continue;
    if (normalized === q) return 1;
    // 连续短语命中与“标题以查询开头”同权：JEI Integration 不能因为前缀相同就压过 Just Enough Items。
    if (normalized.includes(q)) best = Math.max(best, 0.9);
    if (qTokens.length > 1) {
      const tokens = tokenize(candidate);
      let matched = 0;
      let pos = 0;
      for (const token of qTokens) {
        const idx = tokens.indexOf(token, pos);
        if (idx < 0) continue;
        matched += 1;
        pos = idx + 1;
      }
      best = Math.max(best, 0.65 * (matched / qTokens.length));
    }
  }
  return best;
}

/**
 * 聚合排序分：downloads/updated 直接取展示值；relevance 以标题相关度为主，
 * 下载量对数与百科收录作次级权重。下载量一律用原始值，避免可见顺序与排名自相矛盾。
 */
export function scoreItem(
  item: RankableItem,
  query: string,
  sort: CommunitySort,
): number {
  if (sort === "downloads") {
    return item.downloads;
  }
  if (sort === "updated") {
    const ts = Date.parse(item.updated_at ?? "");
    return Number.isNaN(ts) ? 0 : ts;
  }
  if (!query) return item.downloads;
  return (
    titleRelevance(item.title, item.title_zh, query) +
    Math.log10(Math.max(item.downloads, 1)) / 10 +
    (item.mcmod_id == null ? 0 : 0.1)
  );
}
