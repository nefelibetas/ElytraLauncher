// 社区资源聚合器：纯状态机，不依赖 Vue/Tauri；请求函数注入，便于单独验证分页与去重。
import { identityKey, scoreItem, type CommunitySource } from "./rank";

/** 聚合模式每页展示条数（累积列表切片）。 */
export const AGG_PAGE_SIZE = 40;
/** 聚合模式每源每批拉取条数：首批最多 80 条，够铺两页，减少翻页时新增批次对顺序的影响。 */
export const AGG_FETCH_SIZE = 40;

export interface AggParams {
  query: string;
  gameVersion: string;
  loader: string;
  category: string | null;
  /** 内容分类 "CFID/MRslug"；空串为全部。 */
  tag: string;
}

/** 聚合器需要的最小字段；CommunityDisplayItem 结构满足。 */
export interface AggregatorItem {
  id: string;
  title: string;
  title_zh: string | null;
  mcmod_id: number | null;
  downloads: number;
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
 * 按源独立记录 offset/total，逐批拉取后在“已累积结果”上追加。
 * 不重排已展示条目，翻页顺序稳定；批内按 rank.ts 的加权分排序。
 */
export class CommunityAggregator<T extends AggregatorItem> {
  private signature = "";
  private generation = 0;
  private items: T[] = [];
  private seen = new Set<string>();
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
    this.seen.clear();
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
    const target = page * AGG_PAGE_SIZE;
    let stale = 0;
    while (this.items.length < target && !(this.done.curseforge && this.done.modrinth)) {
      const before = this.items.length;
      await this.loadBatch(params, gen);
      // 两源大量重复时可能连续几批没有新增；连续 3 批无进展即停，避免无上限请求。
      if (this.items.length === before) {
        if (++stale >= 3) break;
      } else {
        stale = 0;
      }
    }
    if (gen !== this.generation) throw new StaleSearchError();
    // 当前页刚好填满且仍有源未耗尽：再探一批。若只探到重复项，下一页会空，
    // 这里提前把 hasMore 收口，避免用户点进空页。
    if (this.items.length === target && !(this.done.curseforge && this.done.modrinth)) {
      await this.loadBatch(params, gen);
    }
    if (gen !== this.generation) throw new StaleSearchError();
    const start = (page - 1) * AGG_PAGE_SIZE;
    const end = start + AGG_PAGE_SIZE;
    return {
      items: this.items.slice(start, end),
      loaded: this.items.length,
      total: Math.max(this.totals.curseforge, this.totals.modrinth),
      hasMore: this.items.length > end || !(this.done.curseforge && this.done.modrinth),
      partial: this.partial,
    };
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

    // 同一批内按身份合并：保留下载量更高的一侧，并把另一侧的译名/百科 id 补过来。
    const byIdentity = new Map<string, T>();
    for (const item of batch) {
      const key = identityKey(item);
      const prev = byIdentity.get(key);
      if (prev === undefined) {
        byIdentity.set(key, item);
      } else if (item.downloads > prev.downloads) {
        copyMissingMetadata(prev, item);
        byIdentity.set(key, item);
      } else {
        copyMissingMetadata(item, prev);
      }
    }

    const fresh: T[] = [];
    for (const item of byIdentity.values()) {
      const key = identityKey(item);
      if (this.seen.has(key)) continue;
      this.seen.add(key);
      fresh.push(item);
    }
    fresh.sort(
      (a, b) =>
        scoreItem(b, params.query, params.category) - scoreItem(a, params.query, params.category) ||
        b.downloads - a.downloads,
    );
    this.items.push(...fresh);
  }
}
