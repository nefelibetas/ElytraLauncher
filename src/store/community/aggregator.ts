// 社区资源聚合器：纯状态机，不依赖 Vue/Tauri；请求函数注入，便于单独验证分页与去重。
import { normalizeText, scoreItem, type CommunitySort, type CommunitySource } from "./rank";

/** 聚合模式每页展示条数（从未展示累积项中取页）。 */
export const AGG_PAGE_SIZE = 40;
/** 聚合模式每源每批拉取条数：一次补两源的同一窗口，减少翻页请求；更深的页按需继续补齐。 */
export const AGG_FETCH_SIZE = 40;

export interface AggParams {
  query: string;
  gameVersion: string;
  loader: string;
  category: string | null;
  /** 内容分类 "CFID/MRslug"；空串为全部。 */
  tag: string;
  /** 排序口径。 */
  sort: CommunitySort;
}

/** 聚合器需要的最小字段；CommunityDisplayItem 结构满足。 */
export interface AggregatorItem {
  id: string;
  title: string;
  title_zh: string | null;
  mcmod_id: number | null;
  downloads: number;
  /** 最后更新时间 ISO 8601；缺失为 null/undefined。 */
  updated_at?: string | null;
  source: CommunitySource;
  page_url: string;
  key: string;
  translated?: string | null;
}

export interface AggPage<T extends AggregatorItem> {
  items: T[];
  loaded: number;
  total: number;
  hasMore: boolean;
  partial: boolean;
}

/** 拉取某源的一页；`offset`/`pageSize` 由聚合器维护。 */
export type FetchPage<T extends AggregatorItem> = (
  source: CommunitySource,
  params: AggParams,
  offset: number,
  pageSize: number,
) => Promise<{ items: T[]; total: number }>;

export class StaleSearchError extends Error {}

/** CF 先入：下载量相同时保留 CF 一侧，避免两源返回顺序影响结果。 */
const SOURCE_ORDER: CommunitySource[] = ["curseforge", "modrinth"];

/** 把 from 的缺失译名/百科 id 补到 to；两源同一项目的元数据取并集。 */
function copyMissingMetadata(from: AggregatorItem, to: AggregatorItem) {
  if (to.mcmod_id == null && from.mcmod_id != null) to.mcmod_id = from.mcmod_id;
  if (!to.title_zh && from.title_zh) to.title_zh = from.title_zh;
  if (!to.translated && from.translated) to.translated = from.translated;
}

/**
 * 按源独立记录 offset/total，逐批拉取后对“已累积结果”全局排序。
 * 每页先把两源各自补到 page*40 条：跨源排序时某一源更靠后的条目仍可能插到本页。
 * 补批会重排已加载列表，因此每页从“未展示”条目中取前 40 条并缓存：既避免同一项目
 * 跨页重复，也不会因过滤已展示项造成后续页空缺。
 */
export class CommunityAggregator<T extends AggregatorItem> {
  private signature = "";
  private generation = 0;
  private items: T[] = [];
  /** 已展示条目按 mcmod_id / 归一化标题双索引；跨源缺一侧 id 时靠标题兜底合并。 */
  private byId = new Map<number, T>();
  private byTitle = new Map<string, T>();
  /** 已展示对象身份：合并可能改写 key，不能再用 key 作为跨页去重依据。 */
  private shown = new Set<T>();
  /** 页快照：返回旧页保持原内容；新页只从未展示条目中取。 */
  private pageCache = new Map<number, T[]>();
  private offsets: Record<CommunitySource, number> = { curseforge: 0, modrinth: 0 };
  private totals: Record<CommunitySource, number> = { curseforge: 0, modrinth: 0 };
  private done: Record<CommunitySource, boolean> = { curseforge: false, modrinth: false };
  private partial = false;
  /** 同一次查询内的翻页串行执行，避免双击下一页并发拉取同一 offset。 */
  private queue: Promise<void> = Promise.resolve();

  constructor(
    private readonly fetchPage: FetchPage<T>,
    private readonly allSourcesFailed: () => string,
  ) {}

  private reset(signature: string) {
    this.signature = signature;
    this.generation += 1;
    this.items = [];
    this.byId.clear();
    this.byTitle.clear();
    this.shown.clear();
    this.pageCache.clear();
    this.offsets = { curseforge: 0, modrinth: 0 };
    this.totals = { curseforge: 0, modrinth: 0 };
    this.done = { curseforge: false, modrinth: false };
    this.partial = false;
  }

  page(page: number, params: AggParams, force: boolean): Promise<AggPage<T>> {
    const signature = JSON.stringify([
      params.query,
      params.gameVersion,
      params.loader,
      params.category,
      params.tag,
      params.sort,
    ]);
    if (force || signature !== this.signature) {
      this.reset(signature);
      // 新查询不必等旧请求结束；旧请求在下一次 generation 检查时丢弃。
      this.queue = Promise.resolve();
    }
    const result = this.queue.then(() => this.runPage(page, params));
    this.queue = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  private async runPage(page: number, params: AggParams): Promise<AggPage<T>> {
    const gen = this.generation;
    const cached = this.pageCache.get(page);
    if (cached) return this.pageResult(cached, page);
    const target = page * AGG_PAGE_SIZE;
    let stale = 0;
    // 既要覆盖本页的源窗口，也要有足够未展示条目填满一页；无新增时最多连拉 3 批。
    while (
      (this.needsMore(target) || this.items.length - this.shown.size < AGG_PAGE_SIZE)
      && !(this.done.curseforge && this.done.modrinth)
    ) {
      const before = this.items.length;
      await this.loadBatch(params, gen);
      if (this.items.length === before) {
        if (++stale >= 3) break;
      } else {
        stale = 0;
      }
    }
    if (gen !== this.generation) throw new StaleSearchError();
    // 全局顺序可能因补批改变；取当前未展示条目的前 40 条，已展示项不会再次进入后续页。
    const items = this.items
      .filter(item => !this.shown.has(item))
      .slice(0, AGG_PAGE_SIZE);
    for (const item of items) this.shown.add(item);
    this.pageCache.set(page, items);
    return this.pageResult(items, page);
  }

  private pageResult(items: T[], page: number): AggPage<T> {
    const hasRemaining = this.items.length > this.shown.size;
    return {
      items,
      loaded: this.items.length,
      total: Math.max(this.totals.curseforge, this.totals.modrinth),
      // 返回旧页时，后续页可能已在快照里；单看未展示数会把“下一页”误判为不可用。
      hasMore:
        hasRemaining
        || !(this.done.curseforge && this.done.modrinth)
        || this.pageCache.has(page + 1),
      partial: this.partial,
    };
  }

  /** 本页需要的窗口：去重后够一页，且每个未耗尽的源自身也拉到该页末端。 */
  private needsMore(target: number): boolean {
    if (this.items.length < target) return true;
    return SOURCE_ORDER.some(source => !this.done[source] && this.offsets[source] < target);
  }

  private async loadBatch(params: AggParams, gen: number): Promise<void> {
    const active = SOURCE_ORDER.filter(source => !this.done[source]);
    if (active.length === 0) return;
    const responses = await Promise.all(
      active.map(async source => {
        try {
          const page = await this.fetchPage(source, params, this.offsets[source], AGG_FETCH_SIZE);
          return { source, page, error: null };
        } catch (error) {
          return { source, page: null, error };
        }
      }),
    );
    if (gen !== this.generation) throw new StaleSearchError();

    const batch: T[] = [];
    let succeeded = false;
    for (const { source, page, error } of responses) {
      if (error || page === null) {
        this.done[source] = true;
        this.partial = true;
        continue;
      }
      succeeded = true;
      this.totals[source] = page.total;
      this.offsets[source] += page.items.length;
      // 只在返回空页或 offset 追平 total 时收口；短页可能只是后端过滤了条数，下一批继续。
      if (page.items.length === 0 || this.offsets[source] >= page.total) {
        this.done[source] = true;
      }
      batch.push(...page.items);
    }
    if (!succeeded && this.items.length === 0) {
      throw new Error(this.allSourcesFailed());
    }

    // 同批与跨批统一按 id + 归一化标题查找；重复项只合并元数据，不重复插入。
    const fresh: T[] = [];
    for (const item of batch) {
      const existing = this.findExisting(item);
      if (existing) {
        this.mergeInto(existing, item);
        continue;
      }
      this.index(item);
      fresh.push(item);
    }
    this.items.push(...fresh);
    // 新批次可能含有高于已展示条目的分项（例如另一源后 40 条），必须并入全局顺序。
    this.items.sort(this.comparator(params));
  }

  /** 分数降序；同分按原始下载量与 key 固定次序，避免两源返回顺序影响结果。 */
  private comparator(params: AggParams): (a: T, b: T) => number {
    return (a, b) =>
      scoreItem(b, params.query, params.sort) - scoreItem(a, params.query, params.sort) ||
      b.downloads - a.downloads ||
      a.key.localeCompare(b.key);
  }

  /** 同一项目：mcmod_id 精确优先；缺 id 时按归一化标题兜底，两个不同非空 id 绝不合并。 */
  private findExisting(item: T): T | undefined {
    if (item.mcmod_id != null) {
      const hit = this.byId.get(item.mcmod_id);
      if (hit) return hit;
    }
    const title = normalizeText(item.title);
    if (!title) return undefined;
    const hit = this.byTitle.get(title);
    if (!hit) return undefined;
    if (
      item.mcmod_id != null
      && hit.mcmod_id != null
      && hit.mcmod_id !== item.mcmod_id
    ) {
      return undefined;
    }
    return hit;
  }

  /** 登记 id 与标题索引；标题为空时不登记。 */
  private index(item: T) {
    if (item.mcmod_id != null) this.byId.set(item.mcmod_id, item);
    const title = normalizeText(item.title);
    if (title) this.byTitle.set(title, item);
  }

  /** 合并重复项：下载量更高的一侧作为展示主体，缺失元数据从另一侧补齐。 */
  private mergeInto(existing: T, item: T) {
    if (item.downloads > existing.downloads) {
      const merged = { ...existing, ...item } as T;
      copyMissingMetadata(existing, merged);
      Object.assign(existing, merged);
    } else {
      copyMissingMetadata(item, existing);
    }
    this.index(existing);
    const incomingTitle = normalizeText(item.title);
    if (incomingTitle) this.byTitle.set(incomingTitle, existing);
  }
}
