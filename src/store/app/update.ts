// 启动器更新检查：只查 GitHub Releases latest，提示新版本并跳转下载页；不自动下载/安装。
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { errMsg } from "../../utils/error";

export interface LauncherUpdateInfo {
  current_version: string;
  latest_version: string;
  update_available: boolean;
  url: string;
  notes: string;
  published_at: string | null;
}

export type UpdateCheckState = "idle" | "checking" | "available" | "latest" | "none" | "error";

const UPDATE_DOT_KEY = "elytra.update.showDot";

export const updateState = ref<UpdateCheckState>("idle");
export const updateInfo = ref<LauncherUpdateInfo | null>(null);
export const updateError = ref("");
/** 是否在设置入口/关于 tab 显示更新红点（ localStorage 持久化，默认开）。 */
export const showUpdateDot = ref(localStorage.getItem(UPDATE_DOT_KEY) !== "0");

export function setShowUpdateDot(value: boolean) {
  showUpdateDot.value = value;
  localStorage.setItem(UPDATE_DOT_KEY, value ? "1" : "0");
}

/** 静默/手动共用：失败只更新状态与错误文案，不抛出（后台检查不影响启动）。 */
export async function checkLauncherUpdate(): Promise<void> {
  if (updateState.value === "checking") return;
  updateState.value = "checking";
  updateError.value = "";
  try {
    const info = await invoke<LauncherUpdateInfo | null>("check_launcher_update");
    updateInfo.value = info;
    updateState.value = info ? (info.update_available ? "available" : "latest") : "none";
  } catch (e) {
    updateInfo.value = null;
    updateError.value = errMsg(e);
    updateState.value = "error";
  }
}
