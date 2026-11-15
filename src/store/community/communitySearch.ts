// 社区资源搜索 store（Modrinth / CurseForge 聚合）：模块级 reactive 单例，页面与组件直接 import。来源为本地状态：Modrinth 无需 key，CurseForge 需 .env 配置的 API Key（后端校验）。
import { reactive, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { dlTranslateMode } from "../config/downloaderConfig";
import { errMsg } from "../../utils/error";
import { currentLocale, t } from "../../i18n";
import {
  AGG_PAGE_SIZE,
  CommunityAggregator,
  StaleSearchError,
  type FetchPage,
} from "./aggregator";
import { modTags, tagSupports } from "./tags";
import type { CommunitySort } from "./rank";

export interface CommunitySearchResult {
  id: string;
  title: string;
  title_zh: string | null;
  /** MC百科条目 id（内置表 slug 匹配；未收录为 null）→ https://www.mcmod.cn/class/{id}.html */
  mcmod_id: number | null;
  description: string;
  translated: string | null;
  loaders: string[];
  /** 支持的 MC 游戏版本范围（"最低 ~ 最高"；无则 null） */
  game_version_range: string | null;
  author: string;
  downloads: number;
  /** 最后更新时间 ISO 8601；缺失为 null */
  updated_at: string | null;
  icon_url: string | null;
  page_url: string;
  category: string | null;
}

export interface CommunitySearchPage {
  items: CommunitySearchResult[];
  total: number;
}

export interface CommunityDisplayItem extends CommunitySearchResult {
  /** 该结果实际来自哪个源（聚合时区分；key = source:id，两源 id 体系不同防撞） */
  source: "modrinth" | "curseforge";
  key: string;
}

/** 项目版本文件（镜像 Rust downloader::community::CommunityVersion；命令 get_community_versions）。 */
export interface CommunityVersion {
  version_id: string;
  version_number: string;
  game_versions: string[];
  /** 该文件支持的 MC 版本范围（"最低 ~ 最高"，仅数值版本）；无法解析为 null */
  game_version_range: string | null;
  loaders: string[];
  filename: string;
  download_url: string;
  file_size: number;
  sha1: string | null;
  date_published: string;
  downloads: number;
  version_type: string;
  dependencies: CommunityDependency[];
}

/** 版本文件依赖（镜像 Rust downloader::community::CommunityDependency）。 */
export interface CommunityDependency {
  project_id: string;
  relation: string;
  /** Modrinth 精确版本提示；CurseForge 为 null */
  version_id: string | null;
}

/** 依赖项目详情（镜像 Rust downloader::community::CommunityDependencyItem；命令 get_community_details）。 */
export interface CommunityDependencyItem {
  id: string;
  title: string;
  description: string;
  icon_url: string | null;
  translated: string | null;
}

export function fetchVersions(source: string, projectId: string): Promise<CommunityVersion[]> {
  return invoke<CommunityVersion[]>("get_community_versions", { source, projectId });
}

export function fetchCommunityDetails(source: string, ids: string[]): Promise<CommunityDependencyItem[]> {
  return invoke<CommunityDependencyItem[]>("get_community_details", { source, ids, translate: needsTranslation() });
}

const MOD_PAGE_SIZE = 30;

export const modLoaderOptions = computed<{ value: string; label: string }[]>(() => [
  { value: "", label: t("download.search.allLoaders") },
  { value: "fabric", label: "Fabric" },
  { value: "forge", label: "Forge" },
  { value: "neoforge", label: "NeoForge" },
  { value: "quilt", label: "Quilt" },
]);

export const modTagOptions = computed<{ value: string; label: string }[]>(() =>
  modTags.map(tag => ({ value: tag.value, label: t(tag.labelKey) })),
);

let autoSearched = false;

/** 剥离 markdown/KaTeX 标记（Modrinth 描述支持 KaTeX，标记会原样返回，UI 纯文本需清洗） */
export function cleanText(s: string): string {
  return s
    .replace(/\$\$(.*?)\$\$/g, "$1")
    .replace(/\*\*(.*?)\*\*/g, "$1")
    .replace(/\[(.*?)\]\(.*?\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .trim();
}

export function toDisplayItem(h: CommunitySearchResult, source: "modrinth" | "curseforge"): CommunityDisplayItem {
  // 译名与原名相同（忽略大小写，如 Fabric API）时视为无译名：只显示一个，避免 "X | X"
  const titleZh =
    h.title_zh && h.title_zh.trim().toLowerCase() !== h.title.trim().toLowerCase()
      ? h.title_zh
      : null;
  return {
    ...h,
    title_zh: titleZh,
    title: cleanText(h.title),
    description: cleanText(h.description),
    source,
    key: `${source}:${h.id}`,
  };
}

/** 社区资源是否显示中文翻译：英文界面或“仅英文”模式下关闭。 */
export const communityTranslationEnabled = computed(
  () => currentLocale.value !== "en-US" && dlTranslateMode.value !== "en",
);

/** 翻译请求开关：英文界面/仅英文模式不再请求 MCIM 翻译。 */
export function needsTranslation(): boolean {
  return communityTranslationEnabled.value;
}

/** 拉取单源一页并转成展示项；聚合器只负责 offset/去重/排序。 */
const fetchCommunitySource: FetchPage<CommunityDisplayItem> = async (source, params, offset, pageSize) => {
  const page = await invoke<CommunitySearchPage>("search_community", {
    source,
    query: {
      query: params.query,
      gameVersion: params.gameVersion || null,
      loader: params.loader || null,
      category: params.category,
      tag: params.tag || null,
      sort: params.sort,
      pageSize,
      offset,
    },
    translate: needsTranslation(),
  });
  return { items: page.items.map(h => toDisplayItem(h, source)), total: page.total };
};

export function createCommunityAggregator(): CommunityAggregator<CommunityDisplayItem> {
  return new CommunityAggregator(fetchCommunitySource, () => t("download.search.allSourcesFailed"));
}

const allAggregator = createCommunityAggregator();
/** 记录上一次搜索的来源：从单源切回“全部”时强制重拉，避免展示切源前的旧聚合列表。 */
let lastSource = "";

export const communitySearch = reactive({
  source: "all" as "all" | "modrinth" | "curseforge",
  query: "",
  gameVersion: "",
  loader: "",
  /** 内容分类 "CFID/MRslug"；空串为全部。 */
  tag: "",
  sort: "relevance" as CommunitySort,
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
      if (this.source !== "all" && this.tag && !tagSupports(this.tag, this.source)) {
        this.results = [];
        this.total = 0;
        this.loaded = 0;
        this.hasMore = false;
        this.page = 1;
        this.searched = true;
        this.partialError = t("download.search.tagUnavailable", {
          source: this.source === "curseforge" ? "CurseForge" : "Modrinth",
        });
        return;
      }
      if (this.source === "all") {
        const res = await allAggregator.page(
          page,
          {
            query: this.query.trim(),
            gameVersion: this.gameVersion,
            loader: this.loader,
            category: null,
            tag: this.tag,
            sort: this.sort,
          },
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
            loader: this.loader || null,
            tag: this.tag || null,
            sort: this.sort,
            pageSize: MOD_PAGE_SIZE,
            offset: (page - 1) * MOD_PAGE_SIZE,
          },
          translate: needsTranslation(),
        });
        if (seq !== this.searchSeq) return;
        this.results = res.items.map(h => toDisplayItem(h, this.source as "modrinth" | "curseforge"));
        this.total = res.total;
        this.loaded = Math.min(res.total, page * MOD_PAGE_SIZE);
        this.hasMore = page * MOD_PAGE_SIZE < res.total;
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
    this.source = "all";
    this.gameVersion = "";
    this.loader = "";
    this.tag = "";
    this.sort = "relevance";
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

export const communityTotalPages = computed(() => {
  if (communitySearch.source === "all") {
    // 并集总量未知：本值只供 pager 可见性/兜底，UI 不展示分母（分母随加载增长会误导）。
    return Math.max(1, Math.ceil(communitySearch.loaded / AGG_PAGE_SIZE), communitySearch.page);
  }
  return Math.max(1, Math.ceil(communitySearch.total / MOD_PAGE_SIZE));
});
