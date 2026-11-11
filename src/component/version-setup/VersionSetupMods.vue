<script setup lang="ts">
// 版本设置-Mod 管理 tab：搜索卡 + Tab（全部/可更新）+ 列表；双态：识别到 Modrinth → 详情，未识别 → 本地文件，可更新 item 行右侧有更新按钮。
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { Search, RefreshCw, Upload, Puzzle, Folder, Trash2, Ban, Power } from "@lucide/vue";
import { openGameFolder } from "../../store/config/gameDirs";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import CommunityFileMeta from "../common/CommunityFileMeta.vue";
import ConfirmDialog from "../common/ConfirmDialog.vue";
import LoadingSpinner from "../common/LoadingSpinner.vue";
import EmptyState from "../common/EmptyState.vue";
import ErrorState from "../common/ErrorState.vue";
import BatchActionBar from "./BatchActionBar.vue";
import { useSelectable } from "../../utils/useSelectable";
import { useBatchDelete } from "../../utils/useBatchDelete";
import {
  mods, modsLoading, modsError, modsNetworkError, disabledMods,
  updates, updatesChecked, checkingUpdates, updatesError, updatingFiles,
  loadVersionLoader, loadMods, checkModUpdates, updateMod, deleteMod, toggleModsBulk,
} from "../../store/task/versionDetail";

const route = useRoute();
const { t } = useI18n();

const versionName = computed(() => String(route.query.name ?? ""));

const tab = ref<"all" | "updatable">("all");

const searchQuery = ref("");

const updMap = computed<Record<string, (typeof updates.value)[number]>>(() =>
  Object.fromEntries(updates.value.map(u => [u.file_name, u])),
);

const filteredMods = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return mods.value;
  return mods.value.filter(m =>
    (m.title_zh ?? m.title ?? m.file_name).toLowerCase().includes(q) ||
    m.file_name.toLowerCase().includes(q),
  );
});

const items = computed(() =>
  tab.value === "all" ? filteredMods.value : filteredMods.value.filter(m => updMap.value[m.file_name]),
);

const { selected, selectedSet, selecting, allVisibleSelected, toggle, toggleAll, clear } =
  useSelectable(computed(() => items.value.map(m => m.file_name)));

const updatableSelected = computed(() =>
  selected.value.filter(name => updMap.value[name]).map(name => updMap.value[name]),
);

/** 批量更新选中项：跳过更新中的项；完成后清空选中。 */
async function doBatchUpdate() {
  if (updatingFiles.value.size > 0) return;
  const targets = updatableSelected.value;
  if (targets.length === 0) return;
  for (const upd of targets) {
    await updateMod(versionName.value, upd);
  }
  clear();
}

const {
  dialog: deleteDialog,
  message: deleteMessage,
  open: openDelete,
  confirm: confirmDelete,
} = useBatchDelete({
  remove: (name) => deleteMod(versionName.value, name),
  describe: (names) => names.length === 1
    ? t("settings.version.setup.mods.targetOne", { name: names[0] })
    : t("settings.version.setup.mods.targetMany", { count: names.length }),
  success: (name) => t("settings.version.setup.mods.deleted", { name }),
  after: () => clear(),
});

const selectedEnabled = computed(() => selected.value.filter(n => !disabledMods.value.has(n)));
const selectedDisabled = computed(() => selected.value.filter(n => disabledMods.value.has(n)));

/** 选中项全为已禁用 → 按钮转为「启用」，否则禁用其中仍启用的项（混合选择按禁用处理）。 */
const toggleEnabling = computed(() => selected.value.length > 0 && selectedEnabled.value.length === 0);
const toggleTargets = computed(() => (toggleEnabling.value ? selectedDisabled.value : selectedEnabled.value));
const toggleLabel = computed(() => t("settings.version.setup.mods.toggleWithCount", {
  action: toggleEnabling.value ? t("settings.version.setup.mods.enable") : t("settings.version.setup.mods.disable"),
  count: toggleTargets.value.length,
}));

const toggling = ref(false);

/** 批量启用/禁用：逐个重命名后统一重扫一次列表；完成后清空选中避免误操作。 */
async function doBatchToggle() {
  if (toggling.value) {
    showToast(t("settings.version.setup.mods.opInProgress"));
    return;
  }
  const enabling = toggleEnabling.value;
  const names = [...toggleTargets.value];
  if (names.length === 0) {
    showToast(enabling ? t("settings.version.setup.mods.allEnabled") : t("settings.version.setup.mods.allDisabled"));
    return;
  }
  toggling.value = true;
  try {
    const { ok, failed } = await toggleModsBulk(versionName.value, names);
    showToast(
      failed.length === 0
        ? t(enabling ? "settings.version.setup.mods.toggledEnabled" : "settings.version.setup.mods.toggledDisabled", { count: ok })
        : t(enabling ? "settings.version.setup.mods.toggledPartialEnabled" : "settings.version.setup.mods.toggledPartialDisabled", { ok, failed: failed.length, error: failed[0] }),
    );
    clear();
  } finally {
    toggling.value = false;
  }
}

onMounted(async () => {
  if (!versionName.value) return;
  await loadVersionLoader(versionName.value);
  // 每次进入都重扫本地（禁用态/外部增删立即反映）；识别结果按文件指纹复用，不重复联网
  await loadMods(versionName.value);
});

async function doCheckUpdates() {
  await checkModUpdates(versionName.value);
  if (updatesError.value) {
    showToast(errMsg(updatesError.value));
  } else if (!updates.value.length) {
    showToast(t("settings.version.setup.mods.allUpToDate"));
  }
}

/** 打开 mods 文件夹（目录不存在先创建）。 */
async function openModsFolder() {
  await openGameFolder("mods", versionName.value);
}
</script>

<template>
  <div class="vsm">
    <!-- 上层：搜索卡片 -->
    <div class="vs-card vsm-search-card">
      <Search :size="14" class="vsm-search-icon" />
      <input
        v-model="searchQuery"
        class="vsm-search-input"
        type="text"
        :placeholder="t('settings.version.setup.mods.searchPlaceholder')"
        autocomplete="off"
      />
    </div>

    <!-- 下层：列表卡片 -->
    <div class="vs-card vs-list-card">
      <!-- Tab 行：全部 / 可更新 + 右侧操作 -->
      <div class="vsm-tabbar">
        <div class="vsm-tabs">
          <div
            class="vsm-tab"
            :class="{ active: tab === 'all' }"
            @click="tab = 'all'"
          >{{ t("settings.version.setup.mods.tabAll", { count: mods.length }) }}</div>
          <div
            class="vsm-tab"
            :class="{ active: tab === 'updatable' }"
            @click="tab = 'updatable'"
          >{{ t("settings.version.setup.mods.tabUpdatable", { count: updates.length }) }}</div>
        </div>
        <div class="vsm-actions">
          <button
            class="vsm-act-btn"
            :disabled="checkingUpdates || mods.length === 0"
            @click="doCheckUpdates"
          >
            <RefreshCw :size="13" :class="{ spin: checkingUpdates }" />
            <span>{{ checkingUpdates ? t("settings.version.setup.mods.checking") : t("settings.version.setup.mods.checkUpdates") }}</span>
          </button>
          <button class="vsm-act-btn" @click="openModsFolder">
            <Folder :size="13" />
            <span>{{ t("settings.version.setup.common.openFolder") }}</span>
          </button>
        </div>
      </div>

      <!-- 内容：Mod items -->
      <div class="vs-list-content">
        <Transition name="fade" mode="out-in">
          <LoadingSpinner v-if="modsLoading && mods.length === 0" key="loading" :text="t('settings.version.setup.mods.loading')" />
          <ErrorState v-else-if="modsError" key="err" :text="modsError" />
          <EmptyState v-else-if="mods.length === 0" key="empty">
            <Puzzle :size="40" :stroke-width="1.5" opacity="0.5" />
            <p>{{ t("settings.version.setup.mods.empty") }}</p>
          </EmptyState>
          <EmptyState v-else-if="items.length === 0" key="filtered">
            <Puzzle :size="40" :stroke-width="1.5" opacity="0.5" />
            <p>
              {{ tab === "updatable"
                ? (updatesChecked ? t("settings.version.setup.mods.noUpdate") : t("settings.version.setup.mods.clickCheck"))
                : t("settings.version.setup.mods.noMatch", { query: searchQuery.trim() }) }}
            </p>
          </EmptyState>
          <div v-else key="list">
            <div v-if="modsNetworkError" class="vs-net-warn">{{ t("settings.version.setup.mods.networkWarn") }}</div>
            <div class="vs-list">
              <div v-for="m in items" :key="m.file_name" class="vs-list-item">
                <div
                  class="vsm-item-body"
                  :class="{ selected: selectedSet.has(m.file_name), 'is-disabled': disabledMods.has(m.file_name) }"
                  @click="toggle(m.file_name)"
                >
                  <CommunityFileMeta :item="m" :local-icon="Puzzle">
                    <template #meta>
                      <span v-if="updMap[m.file_name]" class="vsm-new">→ v{{ updMap[m.file_name].latest_version }}</span>
                    </template>
                    <template #local-meta>
                      <span v-if="m.identify === 'pending'" class="vsm-query-hint">
                        <RefreshCw :size="10" class="spin" />{{ t("settings.version.setup.mods.querying") }}
                      </span>
                      <span v-else-if="m.identify === 'failed'" class="vsm-query-hint">{{ t("settings.version.setup.mods.notFound") }}</span>
                      <span v-if="disabledMods.has(m.file_name)" class="vsm-off-badge">{{ t("settings.version.setup.mods.disabledBadge") }}</span>
                    </template>
                  </CommunityFileMeta>
                </div>
              </div>
            </div>
          </div>
        </Transition>
      </div>

      <!-- 底部 action 栏：选中 ≥1 时出现 -->
      <Transition name="fade">
        <BatchActionBar
          v-if="selecting"
          :all-selected="allVisibleSelected"
          :count="selected.length"
          @toggle-all="toggleAll"
          @clear="clear"
        >
          <button
            class="vs-ab-btn update"
            :disabled="updatableSelected.length === 0 || updatingFiles.size > 0"
            @click="doBatchUpdate"
          >
            <Upload :size="13" />
            <span>{{ t("settings.version.setup.mods.updateCount", { count: updatableSelected.length }) }}</span>
          </button>
          <button
            class="vs-ab-btn"
            :disabled="toggleTargets.length === 0 || toggling"
            @click="doBatchToggle"
          >
            <Ban v-if="!toggleEnabling" :size="13" />
            <Power v-else :size="13" />
            <span>{{ toggleLabel }}</span>
          </button>
          <button class="vs-ab-btn danger" @click="openDelete([...selected])">
            <Trash2 :size="13" />
            <span>{{ t("settings.version.setup.common.deleteCount", { count: selected.length }) }}</span>
          </button>
        </BatchActionBar>
      </Transition>
    </div>

    <!-- 删除 Mod 确认 dialog -->
    <ConfirmDialog
      v-model="deleteDialog"
      :title="t('settings.version.setup.mods.deleteTitle')"
      :message="t('settings.version.setup.mods.deleteConfirm', { target: deleteMessage })"
      @confirm="confirmDelete"
    />
  </div>
</template>

<style>
@import "../../styles/version-setup-shared.css";
</style>

<style scoped>
.vsm {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 卡片通用 */

/* 上层：搜索卡片 */
.vsm-search-card {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
}

.vsm-search-icon {
  flex: none;
  color: var(--text-tertiary);
}

.vsm-search-input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-primary);
  font-size: var(--fs-base);
}

.vsm-search-input::placeholder {
  color: var(--text-tertiary);
}

/* 下层：列表卡：占满剩余高度，内容滚动 */

/* Tab 行 */
.vsm-tabbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 14px;
  border-bottom: 1px solid var(--border-default);
  flex-wrap: wrap;
}

.vsm-tabs {
  display: flex;
  gap: 2px;
}

.vsm-tab {
  padding: 10px 12px;
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
  position: relative;
  transition: color var(--dur-fast);
}

.vsm-tab:hover {
  color: var(--text-primary);
}

.vsm-tab.active {
  color: var(--color-brand);
  font-weight: 600;
}

.vsm-tab.active::after {
  content: "";
  position: absolute;
  left: 12px;
  right: 12px;
  bottom: 0;
  height: 2px;
  border-radius: 1px;
  background: var(--color-brand);
}

.vsm-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.vsm-act-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: all var(--dur-fast);
}

.vsm-act-btn:hover:not(:disabled) {
  border-color: var(--color-brand);
  color: var(--color-brand);
}

.vsm-act-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.spin {
  animation: vsm-spin 0.9s linear infinite;
}

@keyframes vsm-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 内容区：卡片内滚动 */

/* 列表 */

.vsm-item-body {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  background: var(--surface-page);
  border: 2px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: border-color var(--dur-fast), background var(--dur-fast);
}

.vsm-item-body:hover {
  border-color: var(--border-strong);
  background: var(--surface-hover);
}

.vsm-item-body.selected {
  border-color: var(--color-brand);
}

/* 已禁用：降对比 + 灰底 + 虚线边（选中态用品牌色实线边，故排除 .selected） */
.vsm-item-body.is-disabled {
  opacity: 0.6;
  background: var(--surface-hover);
}

.vsm-item-body.is-disabled:not(.selected) {
  border-color: var(--border-default);
  border-style: dashed;
}

.vsm-new {
  flex-shrink: 0;
  font-size: var(--fs-2xs);
  font-weight: 600;
  padding: 0 5px;
  border-radius: var(--radius-sm);
  background: var(--color-danger-bg);
  color: var(--color-danger);
}

/* 本地文件样式 */

/* 「已禁用」角标（禁用项仍列出，靠它标识状态） */
.vsm-off-badge {
  margin-left: 6px;
  padding: 1px 6px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

/* 元数据查询状态（查询中 / 未查询到） */
.vsm-query-hint {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  margin-left: 6px;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

/* action 栏公共样式见 version-setup-shared.css；update 态仅 Mods 用 */
.vs-ab-btn.update {
  border-color: var(--color-brand);
  background: var(--color-brand-bg);
  color: var(--color-brand);
}

.vs-ab-btn.update:hover:not(:disabled) {
  background: var(--color-brand);
  color: var(--text-inverse);
}

</style>
