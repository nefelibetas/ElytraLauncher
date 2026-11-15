/** 社区资源聚合排序：纯函数，无 Vue/IPC 依赖，便于单独验证评分与去重键。 */
export type CommunitySource = "modrinth" | "curseforge";
/** 排序口径：relevance（综合）/ downloads / updated。 */
export type CommunitySort = "relevance" | "downloads" | "updated";

/** scoreItem / identityKey 需要的最小字段；CommunityDisplayItem 结构满足。 */
export interface RankableItem {
  title: string;
  title_zh: string | null;
  mcmod_id: number | null;
  downloads: number;
  /** 最后更新时间 ISO 8601；缺失为 null/undefined。 */
  updated_at?: string | null;
  source: CommunitySource;
  page_url: string;
}

/**
 * 平台下载量权重：两平台累计下载口径不同，按资源类型做量级补偿。
 * 系数只参与排序，不改变 UI 上展示的原始下载量。
 */
export function platformMultiplier(category: string | null, source: CommunitySource): number {
  const kind = category ?? "mod";
  if (kind === "datapack") return source === "curseforge" ? 10 : 1;
  if (kind === "resourcepack" || kind === "shader") return source === "curseforge" ? 1 : 4;
  // mod / modpack：MR 头部项目下载量与 CF 同量级，×2 只补长尾，避免 ×5 时首屏几乎全被 MR 占据。
  return source === "curseforge" ? 1 : 2;
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

/** 聚合排序分：按 sort 选择下载量 / 更新时间 / 标题相关度 + 百科加成。 */
export function scoreItem(
  item: RankableItem,
  query: string,
  category: string | null,
  sort: CommunitySort,
): number {
  if (sort === "downloads") {
    return item.downloads * platformMultiplier(category, item.source);
  }
  if (sort === "updated") {
    const ts = Date.parse(item.updated_at ?? "");
    return Number.isNaN(ts) ? 0 : ts;
  }
  const weighted = item.downloads * platformMultiplier(category, item.source);
  if (!query) return weighted;
  return (
    titleRelevance(item.title, item.title_zh, query) +
    Math.log10(Math.max(weighted, 1)) / 10 +
    (item.mcmod_id == null ? 0 : 0.1)
  );
}

/** 跨源同一项目的去重键：百科条目 id 优先（wiki 表已映射两平台 slug），否则归一化标题。 */
export function identityKey(item: RankableItem): string {
  if (item.mcmod_id != null) return `m:${item.mcmod_id}`;
  const title = normalizeText(item.title);
  return title ? `t:${title}` : `u:${item.page_url}`;
}
