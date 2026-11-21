<script setup lang="ts">
import { computed, type Component } from "vue";
import { useI18n } from "vue-i18n";
import { AlertCircle } from "@lucide/vue";
import { communityTranslationEnabled, type CommunityDisplayItem } from "../../store/community/communitySearch";
import { openResourceDetail } from "../../store/community/resourceDetail";
import { displayLoadersForKind } from "../../store/community/resourceKind";
import type { DownloadTab } from "../../store/task/downloadTab";
import { fmtCount } from "../../utils/format";
import IconImage from "../common/IconImage.vue";
import LoadingSpinner from "../common/LoadingSpinner.vue";
import EmptyState from "../common/EmptyState.vue";

/** 搜索状态实例的最小结构（communitySearch 与 createCategorySearch 单例均满足）。 */
export interface CommunityResultState {
  source: "all" | "modrinth" | "curseforge";
  query: string;
  gameVersion: string;
  searching: boolean;
  error: string;
  partialError?: string;
  searched: boolean;
  results: CommunityDisplayItem[];
  total: number;
  /** 聚合模式：已加载列表之外是否还有结果（决定“下一页”是否可点） */
  hasMore?: boolean;
  /** 排序口径（relevance/downloads/updated） */
  sort?: string;
  page: number;
  search(page?: number, force?: boolean): Promise<void> | void;
}

const props = withDefaults(
  defineProps<{
    /** 搜索状态实例（reactive 单例，只读展示 + 翻页调用 search） */
    search: CommunityResultState;
    totalPages: number;
    label: string;
    emptyIcon: Component;
    /** 打开详情回填的加载器（Mod 传 search.loader 保持响应式） */
    detailLoader?: string;
    /** 当前资源入口；用于只展示与该目录匹配的加载器徽章 */
    kind?: DownloadTab;
  }>(),
  {
    detailLoader: "",
  },
);

const { t } = useI18n();

/** 聚合模式的并集总量未知，hasMore 才是权威；totalPages 仅作单源/兜底。 */
const canNext = computed(
  () => props.search.hasMore ?? (props.search.page < props.totalPages),
);

/** 当前入口应展示的加载器徽章；与详情页共用同一过滤口径。 */
function shownLoaders(item: CommunityDisplayItem): string[] {
  return props.kind ? displayLoadersForKind(props.kind, item.loaders) : item.loaders;
}
</script>

<template>
  <div class="cs-result-card">
    <div v-if="search.partialError" class="cs-partial">{{ search.partialError }}</div>
    <button
      v-if="search.error"
      type="button"
      class="cs-load-error"
      :title="t('download.search.clickToRetry')"
      @click="search.search(1, true)"
    >
      <AlertCircle :size="36" :stroke-width="1.5" />
      <span class="cs-load-error-info">
        <span class="cs-load-error-title">{{ t("download.search.loadFailed") }}</span>
        <span class="cs-load-error-desc">{{ search.error }}</span>
      </span>
    </button>
    <LoadingSpinner
      v-else-if="search.searching"
      :text="search.query.trim() ? t('download.search.searchingHint', { category: label }) : t('download.search.loadingPopular', { category: label })"
    />
    <EmptyState v-else-if="search.searched && search.results.length === 0">
      <component :is="emptyIcon" :size="40" :stroke-width="1.5" opacity="0.5" />
      <p>{{ t("download.search.emptyHint", { category: label }) }}</p>
    </EmptyState>
    <template v-else-if="search.searched">
      <div class="cs-list">
        <div v-for="m in search.results" :key="m.key" class="cs-card">
          <div
            class="cs-card-body"
            @click="openResourceDetail(m, { loader: detailLoader, gameVersion: search.gameVersion, selectedVersion: '' })"
          >
            <IconImage :src="m.icon_url" :size="32" />
            <span class="cs-info">
              <span class="cs-title-row">
                <span class="cs-title">{{ communityTranslationEnabled && m.title_zh ? m.title_zh : m.title }}</span>
                <span v-if="communityTranslationEnabled && m.title_zh" class="cs-title-en"> | {{ m.title }}</span>
                <span v-if="search.source === 'all'" class="cs-badge" :class="m.source">{{ m.source === "modrinth" ? "Modrinth" : "CurseForge" }}</span>
                <span class="cs-sub">{{ t("download.search.downloads", { count: fmtCount(m.downloads) }) }}</span>
              </span>
              <span class="cs-desc">{{ communityTranslationEnabled ? (m.translated ?? m.description) : m.description }}</span>
              <span
                v-if="shownLoaders(m).length > 0 || m.game_version_range"
                class="cs-support"
              >
                <span v-if="shownLoaders(m).length > 0" class="cs-support-loaders">
                  <span v-for="ld in shownLoaders(m)" :key="ld" class="cs-ld" :class="ld">{{ ld }}</span>
                </span>
                <span v-if="m.game_version_range" class="cs-support-gv">
                  <span class="cs-gv">{{ m.game_version_range }}</span>
                </span>
              </span>
            </span>
          </div>
        </div>
      </div>
      <div class="cs-footer">
        <div v-if="totalPages > 1 || search.hasMore" class="cs-pager">
          <button class="cs-pager-btn" :disabled="search.page <= 1" @click="search.search(search.page - 1)">{{ t("download.search.prev") }}</button>
          <span v-if="search.source === 'all'" class="cs-pager-info">{{ t("download.search.pageLabel", { page: search.page }) }}</span>
          <span v-else class="cs-pager-info">{{ search.page }} / {{ totalPages }}</span>
          <button class="cs-pager-btn" :disabled="!canNext" @click="search.search(search.page + 1)">{{ t("download.search.next") }}</button>
        </div>
        <!-- 聚合 all 的 total 是去重后下限，故用「至少」；单源精确用「共」 -->
        <span
          class="cs-result-count"
          :title="search.source === 'all' && (search.sort ?? 'relevance') === 'relevance' ? t('download.search.rankHint') : undefined"
        >{{ t(search.source === "all" ? "download.search.resultCountAtLeast" : "download.search.resultCountExact", { count: search.total }, search.total) }}</span>
      </div>
    </template>
    <EmptyState v-else>
      <component :is="emptyIcon" :size="40" :stroke-width="1.5" opacity="0.5" />
      <p>{{ t("download.search.idleHint", { category: label }) }}</p>
    </EmptyState>
  </div>
</template>

<style scoped>
.cs-partial {
  margin: 0 0 8px;
  padding: 7px 10px;
  border: 1px solid var(--color-warning);
  border-radius: var(--radius-sm);
  color: var(--color-warning);
  font-size: var(--fs-sm);
}
.cs-result-card {
  flex: 1; min-height: 0; display: flex; flex-direction: column;
  background: var(--surface-card);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.cs-load-error {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  width: 100%;
  padding: 20px;
  border: none;
  background: transparent;
  text-align: left;
  font: inherit;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background var(--dur-fast);
}

.cs-load-error:hover { background: var(--surface-hover); }

.cs-load-error > svg {
  flex: none;
  color: var(--color-danger);
}

.cs-load-error-info {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  max-width: 560px;
}

.cs-load-error-title {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
}

.cs-load-error-desc {
  font-size: var(--fs-sm);
  color: var(--color-danger);
  word-break: break-word;
}

.cs-list { display: flex; flex-direction: column; padding: 6px 8px; }
.cs-card { overflow: hidden; }
.cs-card + .cs-card { border-top: 1px solid var(--border-subtle, var(--border-default)); }
.cs-card-body {
  width: 100%; display: flex; align-items: center; gap: 10px;
  padding: 8px 14px; background: transparent;
  color: var(--text-primary); font-size: var(--fs-base); user-select: none;
  transition: background 0.1s;
}
.cs-card-body:hover { background-color: var(--surface-hover); }
.cs-card-body { cursor: pointer; }
.cs-info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; text-align: left; }
.cs-title-row { display: flex; align-items: center; gap: 8px; min-width: 0; }
.cs-title { font-weight: 600; font-size: var(--fs-base); color: var(--text-primary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cs-title-en {
  font-size: var(--fs-xs); font-weight: 400; color: var(--text-tertiary);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.cs-badge { font-size: var(--fs-2xs); padding: 1px 6px; border-radius: var(--radius-md); color: var(--text-inverse); flex-shrink: 0; font-weight: 600; }
.cs-badge.modrinth { background: var(--color-modrinth); }
.cs-badge.curseforge { background: var(--color-curseforge); }
.cs-sub { flex-shrink: 0; margin-left: auto; font-size: var(--fs-xs); color: var(--text-tertiary); white-space: nowrap; }
.cs-desc {
  font-size: var(--fs-xs); color: var(--text-tertiary); line-height: 1.4;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  max-width: 100%;
}
.cs-support { display: flex; align-items: center; gap: 6px; min-width: 0; }
.cs-support-loaders {
  display: flex; align-items: center; gap: 5px; min-width: 0; flex-wrap: wrap;
}
.cs-support-gv {
  display: flex; align-items: center; min-width: 0; flex: 0 0 auto;
  margin-left: auto; justify-content: flex-end; max-width: 50%;
}
.cs-ld {
  flex-shrink: 0; font-size: var(--fs-2xs); font-weight: 600; padding: 0 5px;
  border-radius: var(--radius-sm); background: var(--color-brand-bg); color: var(--color-brand);
}
/* 加载器配色：与详情页 .md-ld-* 共用 --ld-* token；未匹配的加载器保留品牌色兜底。 */
.cs-ld.fabric { color: var(--ld-fabric); }
.cs-ld.forge { color: var(--ld-forge); }
.cs-ld.neoforge { color: var(--ld-neoforge); }
.cs-ld.quilt { color: var(--ld-quilt); }
.cs-ld.optifine { color: var(--ld-optifine); }
/* 徽章底色用 color-mix，需 WebKitGTK ≥ 2.42（Ubuntu 22.04 的 2.40 不支持）；不支持时降级为纯文字色徽章，避免整条声明失效（与详情页同款降级）。 */
@supports (color-mix(in srgb, red 12%, transparent)) {
  .cs-ld.fabric { background: color-mix(in srgb, var(--ld-fabric) 12%, transparent); }
  .cs-ld.forge { background: color-mix(in srgb, var(--ld-forge) 12%, transparent); }
  .cs-ld.neoforge { background: color-mix(in srgb, var(--ld-neoforge) 12%, transparent); }
  .cs-ld.quilt { background: color-mix(in srgb, var(--ld-quilt) 12%, transparent); }
  .cs-ld.optifine { background: color-mix(in srgb, var(--ld-optifine) 12%, transparent); }
}
.cs-gv {
  font-size: var(--fs-2xs); font-weight: 600; color: var(--text-secondary);
  background: var(--surface-hover); border-radius: var(--radius-sm); padding: 0 5px;
  line-height: 1.6; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  max-width: 100%;
}

/* 分页器居中、计数固定最右；min-height 不能省：仅计数时 footer 会塌陷，绝对定位的计数紧贴末条目。 */
.cs-footer {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 46px;
  padding: 6px 16px 14px;
}
.cs-pager { display: flex; align-items: center; justify-content: center; gap: 12px; }
.cs-result-count {
  position: absolute; right: 16px; top: 50%; transform: translateY(-50%);
  font-size: var(--fs-sm); color: var(--text-tertiary); user-select: none;
}
.cs-pager-btn {
  padding: 5px 14px;
  border: 1px solid var(--border-default); border-radius: var(--radius-sm);
  background: var(--surface-card); color: var(--text-primary); font-size: var(--fs-sm);
  cursor: pointer; transition: all var(--dur-fast);
}
.cs-pager-btn:hover:not(:disabled) { border-color: var(--color-brand); }
.cs-pager-btn:disabled { opacity: 0.45; cursor: not-allowed; }
.cs-pager-info { font-size: var(--fs-sm); color: var(--text-tertiary); user-select: none; }
</style>
