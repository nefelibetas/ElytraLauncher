// 启动器更新下载：拉取 Release 文件清单、选择安装包与下载目录，下载复用工具箱下载器（tooldl）。
import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { LauncherUpdateConfig } from "../../types/config/LauncherUpdateConfig";
import type { LauncherUpdatePlan } from "../../types/update/LauncherUpdatePlan";
import {
  cancelToolDl, ensureToolDlListen, loadToolDlTasks, openToolDlFolder,
  pauseToolDl, resumeToolDl, toolDlEta, toolDlState,
} from "../config/toolDownload";
import { checkLauncherUpdate, updateInfo, updateState } from "./update";

/** 更新下载对话框是否打开（关闭不中断下载）。 */
export const updateDownloadOpen = ref(false);
export const updatePlan = ref<LauncherUpdatePlan | null>(null);
export const planLoading = ref(false);
export const planError = ref("");
export const showAllPlatforms = ref(false);
export const selectedAssetUrl = ref("");
export const configSaving = ref(false);
/** 当前展示的下载任务 id；空时按选中 URL 回查（兼容关窗重开）。 */
const downloadTaskId = ref("");

export const selectedAsset = computed(
  () => updatePlan.value?.assets.find((asset) => asset.url === selectedAssetUrl.value) ?? null,
);

export const currentPlatformAssets = computed(() => {
  const plan = updatePlan.value;
  return plan ? plan.assets.filter((asset) => asset.platform === plan.platform) : [];
});

export const visibleAssets = computed(() => {
  const plan = updatePlan.value;
  if (!plan) return [];
  return showAllPlatforms.value ? plan.assets : currentPlatformAssets.value;
});

/** 选中的下载任务：优先按 id，其次按当前选中 URL 找最近一次未取消的任务。 */
export const updateDownloadTask = computed(() => {
  const id = downloadTaskId.value;
  if (id) {
    const task = toolDlState.tasks.find((item) => item.id === id);
    if (task) return task.status === "cancelled" ? null : task;
  }
  const url = selectedAsset.value?.url;
  if (!url) return null;
  for (let i = toolDlState.tasks.length - 1; i >= 0; i -= 1) {
    const task = toolDlState.tasks[i];
    if (task.url === url && task.status !== "cancelled") return task;
  }
  return null;
});

export const updateDownloadPercent = computed(() => {
  const task = updateDownloadTask.value;
  if (!task || task.total <= 0) return 0;
  return Math.min(100, Math.floor((task.downloaded / task.total) * 100));
});

export const updateDownloadEta = computed(() => {
  const task = updateDownloadTask.value;
  return task ? toolDlEta(task) : null;
});

export const updateDownloadDirIsDefault = computed(() => {
  const plan = updatePlan.value;
  return !plan || plan.download_dir === plan.default_download_dir;
});

export const updateDownloadVersion = computed(
  () => updatePlan.value?.version || updateInfo.value?.latest_version || "",
);

/** 选中当前平台推荐包；当前平台无产物时自动展开全部平台。 */
function selectDefaultAsset() {
  const plan = updatePlan.value;
  if (!plan) {
    selectedAssetUrl.value = "";
    return;
  }
  const current = plan.assets.filter((asset) => asset.platform === plan.platform);
  showAllPlatforms.value = current.length === 0;
  const pool = showAllPlatforms.value ? plan.assets : current;
  const preferred = pool.find((asset) => asset.recommended) ?? pool[0] ?? plan.assets[0];
  selectedAssetUrl.value = preferred?.url ?? "";
}

export async function refreshUpdatePlan(): Promise<void> {
  planLoading.value = true;
  planError.value = "";
  try {
    updatePlan.value = await invoke<LauncherUpdatePlan>("get_launcher_update_plan");
    selectDefaultAsset();
  } catch (e) {
    updatePlan.value = null;
    selectedAssetUrl.value = "";
    planError.value = String(e);
  } finally {
    planLoading.value = false;
  }
}

/** 关于页入口：无更新信息时先检查；已有同版本计划则保留当前选择与下载进度。 */
export async function openUpdateDownloadDialog(): Promise<void> {
  if (updateState.value !== "available" || !updateInfo.value) {
    await checkLauncherUpdate();
  }
  if (updateState.value !== "available" || !updateInfo.value) return;

  updateDownloadOpen.value = true;
  ensureToolDlListen();
  const needsPlan = updatePlan.value?.version !== updateInfo.value.latest_version || !!planError.value;
  const planTask = needsPlan ? refreshUpdatePlan() : Promise.resolve();
  await loadToolDlTasks().catch(() => {});
  await planTask;
}

export function closeUpdateDownloadDialog() {
  updateDownloadOpen.value = false;
}

export async function saveUpdateDownloadDir(dir: string): Promise<void> {
  const plan = updatePlan.value;
  if (!plan) return;
  configSaving.value = true;
  try {
    const config: LauncherUpdateConfig = { download_dir: dir.trim() };
    await invoke("save_launcher_update_config", { config });
    plan.download_dir = config.download_dir || plan.default_download_dir;
  } finally {
    configSaving.value = false;
  }
}

export async function pickUpdateDownloadDir(): Promise<void> {
  const plan = updatePlan.value;
  if (!plan) return;
  const picked = await open({
    directory: true,
    multiple: false,
    defaultPath: plan.download_dir,
  });
  if (picked) await saveUpdateDownloadDir(String(picked));
}

export function resetUpdateDownloadDir(): Promise<void> {
  return saveUpdateDownloadDir("");
}

export async function startUpdateDownload(): Promise<void> {
  const plan = updatePlan.value;
  const asset = selectedAsset.value;
  if (!plan || !asset) return;
  ensureToolDlListen();
  const ids = await invoke<string[]>("tooldl_enqueue", {
    urls: [asset.url],
    saveDir: plan.download_dir,
    segments: 8,
  });
  downloadTaskId.value = ids[0] ?? "";
}

export async function pauseUpdateDownload(): Promise<void> {
  const task = updateDownloadTask.value;
  if (task) await pauseToolDl(task.id);
}

export async function resumeUpdateDownload(): Promise<void> {
  const task = updateDownloadTask.value;
  if (task) await resumeToolDl(task.id);
}

export async function cancelUpdateDownload(): Promise<void> {
  const task = updateDownloadTask.value;
  if (!task) return;
  await cancelToolDl(task.id);
  downloadTaskId.value = "";
}

export async function openUpdateDownloadFolder(): Promise<void> {
  const task = updateDownloadTask.value;
  if (task) await openToolDlFolder(task.dest);
}

export async function openUpdateReleasePage(): Promise<void> {
  const url = updatePlan.value?.release_url || updateInfo.value?.url;
  if (url) await openUrl(url);
}

/** 切换平台过滤时兜底：当前选中项被过滤掉则改选推荐/首个。 */
export function setShowAllPlatforms(value: boolean) {
  showAllPlatforms.value = value;
  if (value) return;
  const pool = currentPlatformAssets.value;
  if (pool.some((asset) => asset.url === selectedAssetUrl.value)) return;
  const preferred = pool.find((asset) => asset.recommended) ?? pool[0];
  if (preferred) selectedAssetUrl.value = preferred.url;
}
