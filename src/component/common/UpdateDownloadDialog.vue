<script setup lang="ts">
// 启动器更新下载弹窗：按平台筛选 Release 文件、配置保存目录，并展示工具箱下载器进度。
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  AlertCircle, Check, Clock, Download, ExternalLink, FolderOpen, Info, Pause, Play,
  RotateCcw, X,
} from "@lucide/vue";
import AlertDialog from "./AlertDialog.vue";
import CheckToggle from "./CheckToggle.vue";
import LoadingSpinner from "./LoadingSpinner.vue";
import { fmtEta, fmtSize, fmtSpeed } from "../../utils/format";
import { errMsg, formatError } from "../../utils/error";
import { showToast } from "../../utils/toast";
import {
  cancelUpdateDownload, closeUpdateDownloadDialog, configSaving,
  openUpdateDownloadFolder, openUpdateReleasePage, pauseUpdateDownload,
  pickUpdateDownloadDir, planError, planLoading, refreshUpdatePlan,
  resetUpdateDownloadDir, resumeUpdateDownload, selectedAssetUrl,
  setShowAllPlatforms, showAllPlatforms, startUpdateDownload, updateDownloadDirIsDefault,
  updateDownloadEta, updateDownloadOpen, updateDownloadPercent, updateDownloadTask,
  updateDownloadVersion, updatePlan, visibleAssets,
} from "../../store/app/updateDownload";
import { updateInfo } from "../../store/app/update";

const { t } = useI18n();

const PLATFORM_KEYS: Record<string, string> = {
  windows: "settings.about.updateDialog.platform.windows",
  macos: "settings.about.updateDialog.platform.macos",
  linux: "settings.about.updateDialog.platform.linux",
  other: "settings.about.updateDialog.platform.other",
};
const ARCH_KEYS: Record<string, string> = {
  x64: "settings.about.updateDialog.arch.x64",
  arm64: "settings.about.updateDialog.arch.arm64",
  universal: "settings.about.updateDialog.arch.universal",
  x86: "settings.about.updateDialog.arch.x86",
  other: "settings.about.updateDialog.arch.other",
};
const KIND_KEYS: Record<string, string> = {
  setup: "settings.about.updateDialog.kind.setup",
  msi: "settings.about.updateDialog.kind.msi",
  portable: "settings.about.updateDialog.kind.portable",
  dmg: "settings.about.updateDialog.kind.dmg",
  appimage: "settings.about.updateDialog.kind.appimage",
  deb: "settings.about.updateDialog.kind.deb",
  rpm: "settings.about.updateDialog.kind.rpm",
  archive: "settings.about.updateDialog.kind.archive",
  zip: "settings.about.updateDialog.kind.zip",
  other: "settings.about.updateDialog.kind.other",
};
const STATUS_KEYS: Record<string, string> = {
  queued: "settings.about.updateDialog.queued",
  downloading: "settings.about.updateDialog.downloading",
  paused: "settings.about.updateDialog.paused",
  done: "settings.about.updateDialog.done",
  error: "settings.about.updateDialog.failed",
};

function labelOf(keys: Record<string, string>, value: string): string {
  return t(keys[value] ?? keys.other);
}

const open = computed({
  get: () => updateDownloadOpen.value,
  set: (value: boolean) => {
    if (!value) closeUpdateDownloadDialog();
    else updateDownloadOpen.value = true;
  },
});

const title = computed(() =>
  t("settings.about.updateDialog.title", { version: updateDownloadVersion.value || "—" }),
);
const planErrorDetail = computed(() => {
  const formatted = formatError(planError.value);
  return formatted.detail || formatted.summary;
});
const statusText = computed(() => {
  const status = updateDownloadTask.value?.status;
  return status ? labelOf(STATUS_KEYS, status) : "";
});
const taskError = computed(() => {
  const error = updateDownloadTask.value?.error;
  return error ? formatError(error) : null;
});
/** 总长未知时只有「下载中」显示不确定进度条；失败/暂停没有可画的比例，留白更干净。 */
const showProgressBar = computed(() => {
  const task = updateDownloadTask.value;
  return !!task && (task.total > 0 || task.status === "downloading");
});
const isIndeterminate = computed(() => {
  const task = updateDownloadTask.value;
  return !!task && task.status === "downloading" && task.total <= 0;
});
const showProgressMeta = computed(() => {
  const task = updateDownloadTask.value;
  return !!task
    && (task.total > 0 || task.downloaded > 0 || task.speed > 0 || updateDownloadEta.value !== null);
});
const cancelable = computed(() =>
  ["queued", "downloading", "paused", "error"].includes(updateDownloadTask.value?.status ?? ""),
);

/** fmtSize 对 0 返回空串（多数位置用于隐藏），进度卡片需要显式 0 B。 */
function sizeText(bytes: number): string {
  return fmtSize(bytes) || "0 B";
}
const starting = ref(false);
const canStart = computed(() =>
  !!selectedAssetUrl.value
  && visibleAssets.value.some((asset) => asset.url === selectedAssetUrl.value)
  && !updateDownloadTask.value
  && !planLoading.value
  && !starting.value,
);

function selectAsset(url: string) {
  selectedAssetUrl.value = url;
}

async function handleStart() {
  starting.value = true;
  try {
    await startUpdateDownload();
  } catch (e) {
    showToast(errMsg(String(e)));
  } finally {
    starting.value = false;
  }
}

async function handlePause() {
  try {
    await pauseUpdateDownload();
  } catch (e) {
    showToast(errMsg(String(e)));
  }
}

async function handleResume() {
  try {
    await resumeUpdateDownload();
  } catch (e) {
    showToast(errMsg(String(e)));
  }
}

async function handleCancel() {
  try {
    await cancelUpdateDownload();
  } catch (e) {
    showToast(errMsg(String(e)));
  }
}

async function handleOpenFolder() {
  try {
    await openUpdateDownloadFolder();
  } catch (e) {
    showToast(errMsg(String(e)));
  }
}

async function handleOpenRelease() {
  try {
    await openUpdateReleasePage();
  } catch (e) {
    showToast(errMsg(String(e)));
  }
}

async function handlePickDir() {
  try {
    await pickUpdateDownloadDir();
  } catch (e) {
    showToast(errMsg(String(e)));
  }
}

async function handleResetDir() {
  try {
    await resetUpdateDownloadDir();
  } catch (e) {
    showToast(errMsg(String(e)));
  }
}
</script>

<template>
  <AlertDialog v-model="open" :title="title" width="lg">
    <div class="upd">
      <div class="upd-head">
        <span class="upd-current">
          {{ t("settings.about.updateDialog.currentVersion", { version: updateInfo?.current_version ?? "—" }) }}
        </span>
        <button type="button" class="upd-link" @click="handleOpenRelease">
          <ExternalLink :size="12" />
          {{ t("settings.about.updateDialog.openRelease") }}
        </button>
      </div>

      <LoadingSpinner v-if="planLoading" :text="t('common.action.loading')" />

      <div v-else-if="planError" class="upd-state upd-error">
        <span class="upd-error-title">{{ t("settings.about.updateDialog.loadFailed") }}</span>
        <span class="upd-error-detail">{{ planErrorDetail }}</span>
        <div class="upd-state-actions">
          <button type="button" class="upd-action" @click="refreshUpdatePlan">
            <RotateCcw :size="12" />
            {{ t("common.action.retry") }}
          </button>
          <button type="button" class="upd-action" @click="handleOpenRelease">
            {{ t("settings.about.updateDialog.openRelease") }}
          </button>
        </div>
      </div>

      <template v-else-if="updatePlan">
        <div class="upd-filter">
          <CheckToggle
            :model-value="showAllPlatforms"
            :size="14"
            @update:model-value="setShowAllPlatforms(Boolean($event))"
          >
            {{ t("settings.about.updateDialog.showAll") }}
          </CheckToggle>
          <span class="upd-count">{{ visibleAssets.length }}</span>
        </div>

        <div v-if="visibleAssets.length === 0" class="upd-state">
          {{ t("settings.about.updateDialog.noAssets") }}
        </div>
        <div v-else class="upd-assets" role="radiogroup" :aria-label="title">
          <button
            v-for="asset in visibleAssets"
            :key="asset.url"
            type="button"
            class="upd-asset"
            :class="{ selected: asset.url === selectedAssetUrl }"
            role="radio"
            :aria-checked="asset.url === selectedAssetUrl"
            @click="selectAsset(asset.url)"
          >
            <span class="upd-radio" aria-hidden="true"></span>
            <span class="upd-asset-body">
              <span class="upd-asset-name">{{ asset.name }}</span>
              <span class="upd-asset-meta">
                <span class="upd-badge" :class="{ current: asset.platform === updatePlan.platform }">
                  {{ asset.platform === updatePlan.platform ? t("settings.about.updateDialog.currentPlatform") : labelOf(PLATFORM_KEYS, asset.platform) }}
                </span>
                <span>{{ labelOf(ARCH_KEYS, asset.arch) }}</span>
                <span>{{ labelOf(KIND_KEYS, asset.kind) }}</span>
              </span>
            </span>
            <span class="upd-asset-side">
              <span class="upd-size">{{ asset.size > 0 ? fmtSize(asset.size) : "—" }}</span>
              <span v-if="asset.recommended" class="upd-rec">
                {{ t("settings.about.updateDialog.recommended") }}
              </span>
            </span>
          </button>
        </div>

        <div class="upd-dir">
          <FolderOpen :size="13" class="upd-dir-icon" />
          <span class="upd-dir-label">{{ t("settings.about.updateDialog.downloadDir") }}</span>
          <span class="upd-dir-path">{{ updatePlan.download_dir }}</span>
          <span v-if="updateDownloadDirIsDefault" class="upd-badge">
            {{ t("settings.about.updateDialog.defaultDir") }}
          </span>
          <button type="button" class="upd-action" :disabled="configSaving" @click="handlePickDir">
            {{ t("settings.about.updateDialog.changeDir") }}
          </button>
          <button
            v-if="!updateDownloadDirIsDefault"
            type="button"
            class="upd-action"
            :disabled="configSaving"
            @click="handleResetDir"
          >
            {{ t("settings.about.updateDialog.resetDir") }}
          </button>
        </div>

        <div v-if="updateDownloadTask" class="upd-progress" :class="`is-${updateDownloadTask.status}`">
          <div class="upd-progress-head">
            <span class="upd-state-icon" aria-hidden="true">
              <AlertCircle v-if="updateDownloadTask.status === 'error'" :size="14" />
              <Check v-else-if="updateDownloadTask.status === 'done'" :size="14" />
              <Pause v-else-if="updateDownloadTask.status === 'paused'" :size="14" />
              <Clock v-else-if="updateDownloadTask.status === 'queued'" :size="14" />
              <Download v-else :size="14" />
            </span>
            <span class="upd-status">{{ statusText }}</span>
            <span v-if="updateDownloadTask.total > 0" class="upd-pct">{{ updateDownloadPercent }}%</span>
          </div>
          <div v-if="showProgressBar" class="upd-bar" :class="{ indeterminate: isIndeterminate }">
            <div
              class="upd-bar-fill"
              :style="isIndeterminate ? undefined : { width: `${updateDownloadPercent}%` }"
            ></div>
          </div>
          <div v-if="showProgressMeta" class="upd-progress-meta">
            <span v-if="updateDownloadTask.total > 0">
              {{ sizeText(updateDownloadTask.downloaded) }} / {{ fmtSize(updateDownloadTask.total) }}
            </span>
            <span v-else>{{ sizeText(updateDownloadTask.downloaded) }}</span>
            <span v-if="updateDownloadTask.speed > 0">{{ fmtSpeed(updateDownloadTask.speed) }}</span>
            <span v-if="updateDownloadEta !== null">
              {{ t("settings.about.updateDialog.eta", { eta: fmtEta(updateDownloadEta) }) }}
            </span>
          </div>
          <div v-if="taskError" class="upd-error-box">
            <AlertCircle :size="14" />
            <span class="upd-error-text">
              <span class="upd-error-summary">{{ taskError.summary }}</span>
              <span v-if="taskError.detail" class="upd-error-detail">{{ taskError.detail }}</span>
            </span>
          </div>
          <div class="upd-progress-actions">
            <button
              v-if="updateDownloadTask.status === 'downloading'"
              type="button"
              class="upd-action"
              @click="handlePause"
            >
              <Pause :size="12" />
              {{ t("settings.about.updateDialog.pause") }}
            </button>
            <button
              v-if="updateDownloadTask.status === 'paused'"
              type="button"
              class="upd-action primary"
              @click="handleResume"
            >
              <Play :size="12" />
              {{ t("settings.about.updateDialog.resume") }}
            </button>
            <button
              v-if="updateDownloadTask.status === 'error'"
              type="button"
              class="upd-action primary"
              @click="handleResume"
            >
              <RotateCcw :size="12" />
              {{ t("common.action.retry") }}
            </button>
            <button
              v-if="cancelable"
              type="button"
              class="upd-action danger"
              @click="handleCancel"
            >
              <X :size="12" />
              {{ t("settings.about.updateDialog.cancel") }}
            </button>
            <button
              v-if="updateDownloadTask.status === 'done'"
              type="button"
              class="upd-action"
              @click="handleOpenFolder"
            >
              <FolderOpen :size="12" />
              {{ t("settings.about.updateDialog.openFolder") }}
            </button>
          </div>
        </div>
      </template>

      <p class="upd-hint">
        <Info :size="12" class="upd-hint-icon" />
        <span>{{ t("settings.about.updateDialog.manualHint") }}</span>
      </p>
    </div>

    <template #actions>
      <button
        v-if="!updateDownloadTask"
        type="button"
        class="dialog-btn primary"
        :disabled="!canStart"
        @click="handleStart"
      >
        <Download :size="14" />
        {{ t("settings.about.updateDialog.start") }}
      </button>
      <button type="button" class="dialog-btn" @click="closeUpdateDownloadDialog">
        {{ t("common.action.close") }}
      </button>
    </template>
  </AlertDialog>
</template>

<style scoped>
.upd {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.dialog-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.upd-head {
  display: flex;
  align-items: center;
  gap: 10px;
}

.upd-current {
  padding: 2px 8px;
  border-radius: var(--radius-pill);
  background: var(--surface-hover);
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

.upd-link {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-size: var(--fs-sm);
  cursor: pointer;
}

.upd-link:hover {
  color: var(--color-brand);
}

.upd-filter {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.upd-count {
  min-width: 20px;
  padding: 1px 7px;
  border-radius: var(--radius-pill);
  background: var(--surface-hover);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  text-align: center;
}

.upd-assets {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 264px;
  overflow-y: auto;
  padding-right: 2px;
}

.upd-asset {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-page);
  color: var(--text-primary);
  text-align: left;
  cursor: pointer;
  transition: border-color var(--dur-fast), background var(--dur-fast);
}

.upd-asset:hover {
  border-color: var(--color-brand);
}

.upd-asset.selected {
  border-color: var(--color-brand);
  background: var(--color-brand-bg);
}

.upd-radio {
  flex: none;
  width: 16px;
  height: 16px;
  margin-top: 1px;
  border: 2px solid var(--border-strong);
  border-radius: 50%;
  box-sizing: border-box;
}

.upd-asset.selected .upd-radio {
  border-color: var(--color-brand);
  box-shadow: inset 0 0 0 3px var(--color-brand);
}

.upd-asset-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.upd-asset-name {
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  user-select: text;
}

.upd-asset-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.upd-badge {
  flex: none;
  padding: 1px 6px;
  border-radius: var(--radius-pill);
  background: var(--surface-hover);
  color: var(--text-secondary);
  font-size: var(--fs-2xs);
}

.upd-badge.current {
  color: var(--color-brand);
  background: var(--color-brand-bg);
}

.upd-asset-side {
  flex: none;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 3px;
}

.upd-size {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

.upd-rec {
  padding: 1px 6px;
  border-radius: var(--radius-pill);
  background: var(--color-brand-bg);
  font-size: var(--fs-2xs);
  font-weight: 600;
  color: var(--color-brand);
}

.upd-dir {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-page);
}

.upd-dir-icon {
  flex: none;
  color: var(--text-tertiary);
}

.upd-dir-label {
  flex: none;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

.upd-dir-path {
  flex: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  user-select: text;
}

.upd-action {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 11px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-card);
  color: var(--text-secondary);
  font-size: var(--fs-xs);
  cursor: pointer;
  transition: border-color var(--dur-fast), color var(--dur-fast), background var(--dur-fast);
}

.upd-action:hover:not(:disabled) {
  border-color: var(--color-brand);
  color: var(--color-brand);
}

.upd-action:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.upd-action.primary {
  border-color: var(--color-brand);
  background: var(--color-brand);
  color: var(--text-inverse);
  font-weight: 600;
}

.upd-action.primary:hover:not(:disabled) {
  border-color: var(--color-brand-hover);
  background: var(--color-brand-hover);
  color: var(--text-inverse);
}

.upd-action.danger {
  background: transparent;
}

.upd-action.danger:hover:not(:disabled) {
  border-color: var(--color-danger);
  background: var(--color-danger-bg);
  color: var(--color-danger);
}

.upd-progress {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-page);
  box-shadow: inset 3px 0 0 transparent;
}

/* 左侧色条按状态着色：状态一眼可辨，也避免整卡铺色过重 */
.upd-progress.is-downloading {
  border-color: var(--color-brand-bg);
  box-shadow: inset 3px 0 0 var(--color-brand);
}

.upd-progress.is-paused {
  border-color: var(--color-warning-bg);
  box-shadow: inset 3px 0 0 var(--color-warning);
}

.upd-progress.is-done {
  box-shadow: inset 3px 0 0 var(--color-success);
}

.upd-progress.is-error {
  border-color: var(--color-danger-bg);
  box-shadow: inset 3px 0 0 var(--color-danger);
}

.upd-progress-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.upd-state-icon {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--color-brand-bg);
  color: var(--color-brand);
}

.upd-progress.is-paused .upd-state-icon {
  background: var(--color-warning-bg);
  color: var(--color-warning);
}

.upd-progress.is-done .upd-state-icon {
  background: var(--surface-hover);
  color: var(--color-success);
}

.upd-progress.is-error .upd-state-icon {
  background: var(--color-danger-bg);
  color: var(--color-danger);
}

.upd-progress.is-queued .upd-state-icon {
  background: var(--surface-hover);
  color: var(--text-tertiary);
}

.upd-status {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
}

.upd-progress.is-done .upd-status {
  color: var(--color-success);
}

.upd-progress.is-error .upd-status {
  color: var(--color-danger);
}

.upd-progress.is-paused .upd-status {
  color: var(--color-warning);
}

.upd-pct {
  margin-left: auto;
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--text-primary);
  font-variant-numeric: tabular-nums;
}

.upd-progress.is-downloading .upd-pct {
  color: var(--color-brand);
}

.upd-progress.is-done .upd-pct {
  color: var(--color-success);
}

.upd-progress.is-error .upd-pct {
  color: var(--color-danger);
}

.upd-progress.is-paused .upd-pct {
  color: var(--color-warning);
}

.upd-bar {
  height: 8px;
  border-radius: var(--radius-pill);
  background: var(--surface-hover);
  overflow: hidden;
}

.upd-bar-fill {
  height: 100%;
  border-radius: var(--radius-pill);
  background: var(--color-brand-gradient, var(--color-brand));
  transition: width 0.25s ease;
}

.upd-progress.is-done .upd-bar-fill {
  background: var(--color-success);
}

.upd-progress.is-error .upd-bar-fill {
  background: var(--color-danger);
}

.upd-progress.is-paused .upd-bar-fill {
  background: var(--color-warning);
}

/* 总长未知：往返滑块，避免一直停在 0% 像卡死 */
.upd-bar.indeterminate .upd-bar-fill {
  width: 35%;
  animation: upd-indeterminate 1.2s ease-in-out infinite;
}

@keyframes upd-indeterminate {
  from { transform: translateX(-120%); }
  to { transform: translateX(320%); }
}

.upd-progress-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 10px;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

.upd-progress-meta > span + span::before {
  content: "·";
  margin-right: 10px;
  color: var(--border-strong);
}

.upd-progress-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.upd-error-box {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--color-danger-bg);
  color: var(--color-danger);
}

.upd-error-box > svg {
  flex: none;
  margin-top: 1px;
}

.upd-error-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.upd-error-summary {
  font-size: var(--fs-sm);
  font-weight: 600;
}

.upd-error-box .upd-error-detail {
  font-size: var(--fs-xs);
  line-height: 1.5;
  color: var(--text-secondary);
  white-space: pre-wrap;
}

.upd-error {
  align-items: flex-start;
  text-align: left;
  color: var(--text-primary);
}

.upd-error-title {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--color-danger);
}

.upd-error-detail {
  font-size: var(--fs-sm);
  line-height: 1.5;
  color: var(--color-danger);
  word-break: break-word;
}

.upd-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 20px;
  text-align: center;
  font-size: var(--fs-base);
  color: var(--text-tertiary);
}

.upd-state-actions {
  display: flex;
  gap: 8px;
}

.upd-hint {
  margin: 0;
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: var(--fs-xs);
  line-height: 1.5;
  color: var(--text-tertiary);
}

.upd-hint-icon {
  flex: none;
  margin-top: 1px;
}

@media (prefers-reduced-motion: reduce) {
  .upd-bar-fill {
    transition: none;
  }

  .upd-bar.indeterminate .upd-bar-fill {
    width: 35%;
    animation: none;
    transform: none;
  }
}
</style>
