// 分类资源搜索 store（光影包 / 资源包等非 mod 类型）：工厂生成各自独立的 reactive 单例（状态互不干扰），复用 communitySearch 的聚合器，固定 category（project_type / CurseForge classId），无加载器。
import { reactive, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  createCommunityAggregator,
  needsTranslation,
  toDisplayItem,
  type CommunityDisplayItem,
  type CommunitySearchPage,
} from "../community/communitySearch";
import { AGG_PAGE_SIZE, StaleSearchError } from "./aggregator";
import { errMsg } from "../../utils/error";
import { t } from "../../i18n";

const PAGE_SIZE = 30;

export function createCategorySearch(
  category: string,
  initialSource: "all" | "modrinth" | "curseforge" = "all",
) {
  let autoSearched = false;
  /** 上一次搜索来源：从单源切回“全部”时强制重拉聚合列表。 */
  let lastSource = initialSource;
  const aggregator = createCommunityAggregator();

  return reactive({
    category,
    source: initialSource,
    query: "",
    gameVersion: "",
    /** 内容分类 "CFID/MRslug"；当前分类页未开放，保留字段供后续标签表复用。 */
    tag: "",
    searching: false,
    error: "",
    partialError: "",
    searched: false,
    results: [] as CommunityDisplayItem[],
    /** 结果总数：单源为平台精确命中数；聚合（source="all"）为两源较大值＝已知总量下限（非精确），UI 文案须标「至少」。 */
    total: 0,
    /** 聚合模式已累积的结果数（单源为当前页末端位置）；用于推算页码下限。 */
    loaded: 0,
    /** 是否还有可加载结果（聚合模式两源未耗尽或已累积结果未展示完）。 */
    hasMore: false,
    page: 1,
    /** 请求序号：筛选快速连切时只应用最后一次发起的结果（防过期响应覆盖） */
    searchSeq: 0,

    async search(page = 1, force = false) {
      const seq = ++this.searchSeq;
      this.searching = true;
      this.error = "";
      this.partialError = "";
      try {
        const resetAll = force || lastSource !== "all";
        lastSource = this.source;
        if (this.source === "all") {
          const res = await aggregator.page(
            page,
            { query: this.query.trim(), gameVersion: this.gameVersion, loader: "", category, tag: this.tag },
            resetAll,
          );
          if (seq !== this.searchSeq) return;
          this.results = res.items;
          this.loaded = res.loaded;
          this.total = res.total;
          this.hasMore = res.hasMore;
          this.partialError = res.partial ? t("download.search.partialSource") : "";
        } else {
          const res = await invoke<CommunitySearchPage>("search_community", {
            source: this.source,
            query: {
              query: this.query.trim(),
              gameVersion: this.gameVersion || null,
              loader: null,
              category,
              tag: this.tag || null,
              pageSize: PAGE_SIZE,
              offset: (page - 1) * PAGE_SIZE,
            },
            translate: needsTranslation(),
          });
          if (seq !== this.searchSeq) return;
          this.results = res.items.map(h => toDisplayItem(h, this.source as "modrinth" | "curseforge"));
          this.total = res.total;
          this.loaded = Math.min(res.total, page * PAGE_SIZE);
          this.hasMore = page * PAGE_SIZE < res.total;
          this.partialError = "";
        }
        this.page = page;
        this.searched = true;
      } catch (e) {
        if (seq !== this.searchSeq || e instanceof StaleSearchError) return;
        this.error = errMsg(e);
        this.searched = true;
        this.results = [];
        this.total = 0;
        this.loaded = 0;
        this.hasMore = false;
        this.partialError = "";
      } finally {
        if (seq === this.searchSeq) this.searching = false;
      }
    },

    reset() {
      this.query = "";
      this.source = initialSource;
      this.gameVersion = "";
      this.tag = "";
      this.results = [];
      this.total = 0;
      this.loaded = 0;
      this.hasMore = false;
      this.page = 1;
      this.searched = false;
      this.error = "";
      this.search(1, true);
    },

    ensureAutoSearch() {
      if (autoSearched || this.searching) return;
      autoSearched = true;
      this.search(1, true);
    },
  });
}

function totalPages(search: { source: string; total: number; loaded: number; page: number }): number {
  if (search.source === "all") {
    // 并集总量未知：本值只供 pager 可见性/兜底，UI 不展示分母。
    return Math.max(1, Math.ceil(search.loaded / AGG_PAGE_SIZE), search.page);
  }
  return Math.max(1, Math.ceil(search.total / PAGE_SIZE));
}

export const shaderSearch = createCategorySearch("shader");
export const resourcepackSearch = createCategorySearch("resourcepack");
export const datapackSearch = createCategorySearch("datapack");
export const modpackSearch = createCategorySearch("modpack");
/** worlds（地图存档）：CurseForge 单源（Modrinth 无 worlds project_type，锁定 source 防无效请求） */
export const worldsSearch = createCategorySearch("worlds", "curseforge");

export const shaderTotalPages = computed(() => totalPages(shaderSearch));
export const resourcepackTotalPages = computed(() => totalPages(resourcepackSearch));
export const datapackTotalPages = computed(() => totalPages(datapackSearch));
export const modpackTotalPages = computed(() => totalPages(modpackSearch));
export const worldsTotalPages = computed(() => totalPages(worldsSearch));
