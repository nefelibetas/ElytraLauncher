<script setup lang="ts">
// 版本设置-存档管理 tab：标题卡（打开存档文件夹）+ 列表卡（缩略图 + 名称 + 最后游玩），支持多选批量删除。
import { computed, onMounted, onUnmounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { Folder, Trash2, Map } from "@lucide/vue";
import { deleteWorld } from "../../store/task/versionDetail";
import { openGameFolder } from "../../store/config/gameDirs";
import { fmtUnix } from "../../utils/format";
import { useSelectable } from "../../utils/useSelectable";
import { useBatchDelete } from "../../utils/useBatchDelete";
import { copyText } from "../../utils/clipboard";
import { showNativeActionMenu } from "../../utils/contextMenu";
import IconImage from "../common/IconImage.vue";
import ConfirmDialog from "../common/ConfirmDialog.vue";
import LoadingSpinner from "../common/LoadingSpinner.vue";
import EmptyState from "../common/EmptyState.vue";
import ErrorState from "../common/ErrorState.vue";
import BatchActionBar from "./BatchActionBar.vue";
import {
  worlds, worldsLoading, worldsError, worldIcons, worldsLoadedFor,
  loadWorlds, startWorldsLivePolling, stopWorldsLivePolling,
} from "../../store/task/versionDetail";

const route = useRoute();
const router = useRouter();
const { t } = useI18n();

const versionName = computed(() => String(route.query.name ?? ""));

function previewWorld(world: string) {
  void router.push({
    path: "/world-preview",
    query: { version: versionName.value, world, from: "setup" },
  });
}

const { selected, selectedSet, selecting, allVisibleSelected, toggle, toggleAll, clear } =
  useSelectable(computed(() => worlds.value.map(w => w.name)));

const {
  dialog: deleteDialog,
  message: deleteMessage,
  open: openDelete,
  confirm: confirmDelete,
} = useBatchDelete({
  remove: (name) => deleteWorld(versionName.value, name),
  describe: (names) => names.length === 1
    ? t("settings.version.setup.saves.targetOne", { name: names[0] })
    : t("settings.version.setup.saves.targetMany", { count: names.length }),
  success: (name) => t("settings.version.setup.saves.deleted", { name }),
  after: async () => {
    await loadWorlds(versionName.value);
    clear();
  },
});

/** 行级右键菜单：复制存档名 / 预览 / 打开目录 / 删除。 */
async function openRowMenu(w: { name: string }) {
  await showNativeActionMenu(
    [
      { id: "copy", label: t("common.contextMenu.copyFileName") },
      { id: "preview", label: t("worldPreview.open") },
      { id: "folder", label: t("common.contextMenu.openFolder") },
      { id: "delete", label: t("common.action.delete") },
    ],
    (id) => {
      if (id === "copy") void copyText(w.name);
      else if (id === "preview") previewWorld(w.name);
      else if (id === "folder") void openSavesFolder();
      else if (id === "delete") openDelete([w.name]);
    },
  );
}

onMounted(async () => {
  if (!versionName.value) return;
  // 缓存命中则跳过加载，避免 loading 闪烁
  if (worldsLoadedFor.value !== versionName.value) {
    await loadWorlds(versionName.value);
  }
  // 实时轮询存档：游戏运行中新建/删除自动反映（缩略图由 store watch 补载）
  startWorldsLivePolling(versionName.value);
});

onUnmounted(() => stopWorldsLivePolling());

/** 打开存档文件夹（目录不存在先创建）。 */
async function openSavesFolder() {
  await openGameFolder("saves", versionName.value);
}
</script>

<template>
  <div class="vss">
    <!-- 标题卡 -->
    <div class="vs-card vs-head-card">
      <Map :size="16" class="vs-head-icon" />
      <span class="vs-head-title">{{ t("settings.version.setup.saves.title") }}</span>
      <button class="vs-act-btn" @click="openSavesFolder">
        <Folder :size="13" />
        <span>{{ t("settings.version.setup.saves.openFolder") }}</span>
      </button>
    </div>

    <!-- 列表卡 -->
    <div class="vs-card vs-list-card">
      <div class="vs-list-content">
        <Transition name="fade" mode="out-in">
          <LoadingSpinner v-if="worldsLoading" key="loading" :text="t('settings.version.setup.saves.loading')" />
          <ErrorState v-else-if="worldsError" key="err" :text="worldsError" />
          <EmptyState v-else-if="worlds.length === 0" key="empty">
            <Map :size="40" :stroke-width="1.5" opacity="0.5" />
            <p>{{ t("settings.version.setup.saves.empty") }}</p>
          </EmptyState>
          <div v-else key="list" class="vs-list">
            <div v-for="w in worlds" :key="w.name" class="vs-list-item">
              <div
                class="vss-item-body"
                :class="{ selected: selectedSet.has(w.name) }"
                @click="toggle(w.name)"
                @contextmenu.prevent.stop="openRowMenu(w)"
              >
                <IconImage
                  v-if="worldIcons[w.name]"
                  :src="worldIcons[w.name]"
                  :size="36"
                  class="vss-thumb"
                />
                <Map v-else :size="18" class="vss-local-icon" />
                <span class="vss-info">
                  <span class="vss-local-name">{{ w.name }}</span>
                  <span class="vss-local-meta">{{ t("settings.version.setup.saves.lastPlayed", { date: fmtUnix(w.last_modified) }) }}</span>
                </span>
                <button
                  class="vss-preview"
                  :title="t('worldPreview.open')"
                  :aria-label="t('worldPreview.open')"
                  @click.stop="previewWorld(w.name)"
                >
                  <Map :size="15" />
                </button>
              </div>
            </div>
          </div>
        </Transition>
      </div>
       </div>

    <!-- 底部 action 栏：选中 ≥1 时出现，固定列表卡底部 -->
    <Transition name="fade">
      <BatchActionBar
        v-if="selecting"
        variant="boxed"
        :all-selected="allVisibleSelected"
        :count="selected.length"
        @toggle-all="toggleAll"
        @clear="clear"
      >
        <button class="vs-ab-btn danger" @click="openDelete([...selected])">
          <Trash2 :size="13" />
          <span>{{ t("settings.version.setup.common.deleteCount", { count: selected.length }) }}</span>
        </button>
      </BatchActionBar>
    </Transition>

    <!-- 删除确认 dialog -->
    <ConfirmDialog
      v-model="deleteDialog"
      :title="t('settings.version.setup.saves.deleteTitle')"
      :message="t('settings.version.setup.saves.deleteConfirm', { target: deleteMessage })"
      @confirm="confirmDelete"
    />
  </div>
</template>

<style>
@import "../../styles/version-setup-shared.css";
</style>

<style scoped>
.vss {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.vss-item-body {
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

.vss-item-body:hover {
  border-color: var(--border-strong);
  background: var(--surface-hover);
}

.vss-item-body.selected {
  border-color: var(--color-brand);
}

.vss-thumb {
  flex: none;
  border-radius: var(--radius-md);
  overflow: hidden;
  border: 1px solid var(--border-default);
}

.vss-local-icon {
  flex: none;
  color: var(--text-tertiary);
}

.vss-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.vss-local-name {
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.vss-local-meta {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.vss-preview {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.vss-preview:hover {
  color: var(--color-brand);
  border-color: var(--color-brand);
}
</style>
