// 实例升级 IPC 与任务终态编排：安装任务成功后提交改名，失败/取消回滚备份。
import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import router from "../../router";
import { t } from "../../i18n";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import { setSelected } from "../app/selectedVersion";
import { loadVersions } from "../config/gameDirs";
import { dlState, registerTaskTerminalHook } from "./download";
import { renameVersionIconCache } from "./versionIcon";
import type { InstanceUpgradeInfo } from "../../types/game/InstanceUpgradeInfo";
import type { UpgradeReport } from "../../types/game/UpgradeReport";
import type { UpgradeCommitResult } from "../../types/game/UpgradeCommitResult";
import type { UpgradeRollbackResult } from "../../types/game/UpgradeRollbackResult";

export function getInstanceUpgradeInfo(versionName: string): Promise<InstanceUpgradeInfo> {
  return invoke<InstanceUpgradeInfo>("get_instance_upgrade_info", { versionDirName: versionName });
}

export function checkInstanceUpgrade(
  versionName: string,
  targetMc: string,
  targetLoader: string,
  targetLoaderVersion: string | null,
): Promise<UpgradeReport> {
  return invoke<UpgradeReport>("check_instance_upgrade", {
    versionDirName: versionName,
    targetMc,
    targetLoader,
    targetLoaderVersion,
  });
}

function prepareInstanceUpgrade(
  versionName: string,
  targetMc: string,
  targetLoader: string,
  targetLoaderVersion: string | null,
  disableMods: string[],
): Promise<InstanceUpgradeInfo> {
  return invoke<InstanceUpgradeInfo>("prepare_instance_upgrade", {
    versionDirName: versionName,
    targetMc,
    targetLoader,
    targetLoaderVersion,
    disableMods,
  });
}

export function markInstanceUpgradeReady(versionName: string): Promise<void> {
  return invoke("mark_instance_upgrade_ready", { versionDirName: versionName });
}

export function rollbackInstanceUpgrade(versionName: string): Promise<UpgradeRollbackResult> {
  return invoke<UpgradeRollbackResult>("rollback_instance_upgrade", { versionDirName: versionName });
}

export function commitInstanceUpgrade(versionName: string): Promise<UpgradeCommitResult> {
  return invoke<UpgradeCommitResult>("commit_instance_upgrade", { versionDirName: versionName });
}

export interface StartUpgradePayload {
  versionName: string;
  versionUrl: string;
  targetMc: string;
  targetLoader: string;
  targetLoaderVersion: string | null;
  disableMods: string[];
  displayName: string;
}

/** 当前页面会话内正在运行的升级任务（跨组件重建保留）。 */
export const upgradeRunning = ref(false);
export const upgradeRunningVersion = ref("");

/** 提交升级任务：先备份，再以旧目录名在原目录安装目标版本；终态回调负责提交/回滚。 */
export async function startInstanceUpgrade(payload: StartUpgradePayload): Promise<string> {
  await prepareInstanceUpgrade(
    payload.versionName,
    payload.targetMc,
    payload.targetLoader,
    payload.targetLoaderVersion,
    payload.disableMods,
  );
  let taskId = "";
  try {
    taskId = await dlState.startTask(
      payload.versionName,
      payload.versionUrl,
      payload.targetMc,
      payload.targetLoader === "vanilla" ? undefined : payload.targetLoader,
      payload.targetLoader === "vanilla" ? undefined : payload.targetLoaderVersion ?? undefined,
      undefined,
      payload.versionName,
      undefined,
      undefined,
      payload.displayName,
    );
  } catch (e) {
    await rollbackInstanceUpgrade(payload.versionName).catch(() => undefined);
    throw e;
  }
  upgradeRunning.value = true;
  upgradeRunningVersion.value = payload.versionName;
  registerTaskTerminalHook(taskId, ok => {
    void finalizeUpgrade(ok, payload.versionName);
  });
  return taskId;
}

/** 回滚并刷新列表/选中版本；供任务失败回调与横幅按钮共用。 */
export async function rollbackAndRefresh(versionName: string): Promise<UpgradeRollbackResult> {
  const result = await rollbackInstanceUpgrade(versionName);
  await afterVersionChanged(versionName, result.restored_version);
  return result;
}

/** 完成已就绪的升级；任务成功回调与“上次升级未完成”横幅共用。 */
export async function finishInstanceUpgrade(versionName: string): Promise<UpgradeCommitResult> {
  await markInstanceUpgradeReady(versionName);
  const result = await commitInstanceUpgrade(versionName);
  await afterVersionChanged(result.old_version, result.new_version);
  showToast(t("settings.version.setup.upgrade.done", { version: result.new_version }));
  return result;
}

async function finalizeUpgrade(ok: boolean, versionName: string) {
  try {
    if (!ok) {
      try {
        await rollbackAndRefresh(versionName);
        showToast(t("settings.version.setup.upgrade.rolledBack"));
      } catch (e) {
        showToast(errMsg(e));
      }
      return;
    }
    try {
      await finishInstanceUpgrade(versionName);
    } catch (e) {
      // 备份保留：页面横幅可重试提交或回滚。
      showToast(errMsg(e));
    }
  } finally {
    upgradeRunning.value = false;
    upgradeRunningVersion.value = "";
  }
}

async function afterVersionChanged(from: string, to: string) {
  renameVersionIconCache(from, to);
  setSelected(to);
  try {
    await loadVersions();
  } catch {
    // 目录监听会补发版本列表；这里失败不应把已完成的升级标成失败。
  }
  const route = router.currentRoute.value;
  if (route.name === "version-setup" && String(route.query.name ?? "") === from) {
    await router.replace({ name: "version-setup", query: { name: to } });
  }
}
