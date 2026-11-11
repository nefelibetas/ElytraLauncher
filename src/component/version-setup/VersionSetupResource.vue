<script setup lang="ts">
// 版本设置-资源管理 tab（材质包/数据包/光影包）：标题卡 + 列表卡；双态：识别到 Modrinth → 社区详情，未识别 → 本地文件 + 类型图标。
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { Folder, Trash2, Palette, Database, Sun } from "@lucide/vue";
import { deleteResourceFile } from "../../store/task/versionDetail";
import { openGameFolder } from "../../store/config/gameDirs";
import { useSelectable } from "../../utils/useSelectable";
import { useBatchDelete } from "../../utils/useBatchDelete";
import { copyText } from "../../utils/clipboard";
import { showNativeActionMenu } from "../../utils/contextMenu";
import CommunityFileMeta from "../common/CommunityFileMeta.vue";
import ConfirmDialog from "../common/ConfirmDialog.vue";
import LoadingSpinner from "../common/LoadingSpinner.vue";
import EmptyState from "../common/EmptyState.vue";
import ErrorState from "../common/ErrorState.vue";
import BatchActionBar from "./BatchActionBar.vue";
import {
  resourceRefs,
  loadResourceDetailed, loadResourceQuick,
  type ResourceKind,
} from "../../store/task/versionDetail";

const props = defineProps<{ kind: ResourceKind }>();

const route = useRoute();
const { t } = useI18n();

const versionName = computed(() => String(route.query.name ?? ""));

const META = {
  resourcepacks: { labelKey: "settings.version.setup.resource.kindResourcepacks", icon: Palette },
  datapacks: { labelKey: "settings.version.setup.resource.kindDatapacks", icon: Database },
  shaderpacks: { labelKey: "settings.version.setup.resource.kindShaderpacks", icon: Sun },
} as const;

const meta = computed(() => META[props.kind]);

/** 类型名（材质包/数据包/光影包），用于标题、空态与加载文案。 */
const kindLabel = computed(() => t(meta.value.labelKey));

/** 当前类型的列表状态（store 按 kind 取 ref，此处解包供模板直用）。 */
const res = computed(() => {
  const r = resourceRefs(props.kind);
  return {
    list: r.list.value,
    loading: r.loading.value,
    error: r.error.value,
    networkError: r.networkError.value,
  };
});

const loadedFor = computed(() => resourceRefs(props.kind).loadedFor.value);

const { selected, selectedSet, selecting, allVisibleSelected, toggle, toggleAll, clear } =
  useSelectable(computed(() => res.value.list.map(f => f.file_name)));

const {
  dialog: deleteDialog,
  message: deleteMessage,
  open: openDelete,
  confirm: confirmDelete,
} = useBatchDelete({
  remove: (name) => deleteResourceFile(versionName.value, props.kind, name),
  describe: (names) => names.length === 1
    ? t("settings.version.setup.resource.targetOne", { name: names[0] })
    : t("settings.version.setup.resource.targetMany", { count: names.length }),
  success: (name) => t("settings.version.setup.resource.deleted", { name }),
  after: async () => {
    // 刷新详细列表 + 快速扫描（同步主页卡片图标判定）
    await loadResourceDetailed(versionName.value, props.kind);
    await loadResourceQuick(versionName.value, props.kind);
    clear();
  },
});

onMounted(async () => {
  if (!versionName.value) return;
  // 缓存命中则跳过加载，避免 loading 闪烁
  if (loadedFor.value !== versionName.value) {
    await loadResourceDetailed(versionName.value, props.kind);
  }
});

/** 行级右键菜单：复制文件名 / 打开目录 / 删除。 */
async function openRowMenu(f: { file_name: string }) {
  await showNativeActionMenu(
    [
      { id: "copy", label: t("common.contextMenu.copyFileName") },
      { id: "folder", label: t("common.contextMenu.openFolder") },
      { id: "delete", label: t("common.action.delete") },
    ],
    (id) => {
      if (id === "copy") void copyText(f.file_name);
      else if (id === "folder") void openResourceFolder();
      else if (id === "delete") openDelete([f.file_name]);
    },
  );
}

/** 打开资源目录（目录不存在先创建）。 */
async function openResourceFolder() {
  await openGameFolder(props.kind, versionName.value);
}
</script>

<template>
  <div class="vsr">
    <!-- 标题卡：类型名 + 打开文件夹 -->
    <div class="vs-card vs-head-card">
      <component :is="meta.icon" :size="16" class="vs-head-icon" />
      <span class="vs-head-title">{{ t("settings.version.setup.resource.title", { kind: kindLabel }) }}</span>
      <button class="vs-act-btn" @click="openResourceFolder">
        <Folder :size="13" />
        <span>{{ t("settings.version.setup.common.openFolder") }}</span>
      </button>
    </div>

    <!-- 列表卡 -->
    <div class="vs-card vs-list-card">
      <div class="vs-list-content">
        <Transition name="fade" mode="out-in">
          <LoadingSpinner v-if="res.loading && res.list.length === 0" key="loading" :text="t('settings.version.setup.resource.loading', { kind: kindLabel })" />
          <ErrorState v-else-if="res.error" key="err" :text="res.error" />
          <EmptyState v-else-if="res.list.length === 0" key="empty">
            <component :is="meta.icon" :size="40" :stroke-width="1.5" opacity="0.5" />
            <p>{{ t("settings.version.setup.resource.empty", { kind: kindLabel }) }}</p>
          </EmptyState>
          <div v-else key="list">
            <div v-if="res.networkError" class="vs-net-warn">{{ t("settings.version.setup.resource.networkWarn") }}</div>
            <div class="vs-list">
              <div v-for="f in res.list" :key="f.file_name" class="vs-list-item">
                <div
                  class="vsr-item-body"
                  :class="{ selected: selectedSet.has(f.file_name) }"
                  @click="toggle(f.file_name)"
                  @contextmenu.prevent.stop="openRowMenu(f)"
                >
                  <CommunityFileMeta :item="f" :local-icon="meta.icon" />
                </div>
              </div>
            </div>
          </div>
        </Transition>
      </div>
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
        <button class="vs-ab-btn danger" @click="openDelete([...selected])">
          <Trash2 :size="13" />
          <span>{{ t("settings.version.setup.common.deleteCount", { count: selected.length }) }}</span>
        </button>
      </BatchActionBar>
    </Transition>

    <!-- 删除确认 dialog -->
    <ConfirmDialog
      v-model="deleteDialog"
      :title="t('settings.version.setup.resource.deleteTitle')"
      :message="t('settings.version.setup.resource.deleteConfirm', { target: deleteMessage })"
      @confirm="confirmDelete"
    />
  </div>
</template>

<style>
@import "../../styles/version-setup-shared.css";
</style>

<style scoped>
.vsr {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 标题卡 */

/* 列表卡：占满剩余高度，内容滚动 */

.vsr-item-body {
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

.vsr-item-body:hover {
  border-color: var(--border-strong);
  background: var(--surface-hover);
}

.vsr-item-body.selected {
  border-color: var(--color-brand);
}

/* 本地文件样式 */

</style>
