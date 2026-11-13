<script setup lang="ts">
// 版本设置-导出 tab：启动脚本（PCL 风格 .bat/.sh）+ 整合包（MRPACK zip）。
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { CodeXml, Package } from "@lucide/vue";
import { loadLastUsername } from "../../store/auth/auth";
import {
  exportLaunchScript,
  exportModpack as exportModpackCommand,
  type ModpackExportFormat,
} from "../../store/task/versionSetup";
import { dlState } from "../../store/task/download";
import { save } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import SetupCard from "../common/SetupCard.vue";
import {
  authMode, msAccount, loadMsAccount, username as offlineUsername,
} from "../../store/auth/auth";

const route = useRoute();
const { t } = useI18n();

/** 平台判定：Windows → .bat，其余 → .sh（模板展示与导出共用）。 */
const isWin = /windows/i.test(navigator.userAgent);
const scriptExt = isWin ? "bat" : "sh";

/** 游戏 id（版本目录名），由路由 query 传入。 */
const versionName = computed(() => String(route.query.name ?? ""));

async function exportScript() {
  try {
    const path = await save({
      defaultPath: t("settings.version.setup.export.scriptFileName", { version: versionName.value, ext: scriptExt }),
      filters: [{ name: t("settings.version.setup.export.scriptFilter"), extensions: [scriptExt] }],
    });
    if (!path) return; // 用户取消

    // auth store 的 AuthMode 为 premium|offline；后端命令参数用 online|offline
    let username = "";
    let accessToken = "";
    let uuid = "";
    let xuid = "";
    if (authMode.value === "premium") {
      if (!msAccount.value) await loadMsAccount();
      const acc = msAccount.value;
      if (!acc) {
        showToast(t("settings.version.setup.export.notLoggedIn"));
        return;
      }
      username = acc.name;
      accessToken = acc.access_token;
      uuid = acc.uuid;
      xuid = acc.xuid;
    } else {
      username =
        offlineUsername.value.trim() ||
        (await loadLastUsername()) ||
        "Steve";
    }

    await exportLaunchScript({
      savePath: path,
      versionDirName: versionName.value,
      authMode: authMode.value === "premium" ? "online" : "offline",
      username,
      accessToken,
      uuid,
      xuid,
    });
    showToast(t("settings.version.setup.export.scriptExported"));
  } catch (e) {
    showToast(errMsg(e));
  }
}

/** 导出整合包：MRPACK（Modrinth 命中走 files[]）或 CurseForge（指纹命中走 files[]）；未命中均进 overrides，存档不打包。 */
async function exportModpack(format: ModpackExportFormat) {
  try {
    const mrpack = format === "mrpack";
    const path = await save({
      defaultPath: `${versionName.value}.${mrpack ? "mrpack" : "zip"}`,
      filters: mrpack
        ? [{ name: t("settings.version.setup.export.modpackFilter"), extensions: ["mrpack"] }]
        : [{ name: t("settings.version.setup.export.curseforgeFilter"), extensions: ["zip"] }],
    });
    if (!path) return; // 用户取消
    const taskId = await exportModpackCommand(versionName.value, path, format);
    showToast(t("settings.version.setup.export.modpackSubmitted"));
    // 预注册任务卡片；极快终态由 adoptTask 的终态快照校正，避免漏掉完成/失败提示。
    await dlState.adoptTask(taskId, {
      name: versionName.value,
      nameKey: mrpack
        ? "task.backend.name.exportMrpack"
        : "task.backend.name.exportCurseforge",
      nameParams: { version: versionName.value },
      kind: "export",
    });
  } catch (e) {
    showToast(errMsg(e));
  }
}
</script>

<template>
  <div class="vs-page">
    <SetupCard
      :padded="false"
      :title="t('settings.version.setup.export.scriptTitle')"
      :desc="t('settings.version.setup.export.scriptDesc', { ext: scriptExt.toUpperCase() })"
    >
      <div class="setup-btn-row">
        <button class="setup-btn" @click="exportScript">
          <CodeXml :size="15" />
          <span>{{ t("settings.version.setup.export.scriptButton") }}</span>
        </button>
      </div>
    </SetupCard>

    <SetupCard
      :padded="false"
      :title="t('settings.version.setup.export.modpackTitle')"
      :desc="t('settings.version.setup.export.modpackDesc')"
    >
      <div class="setup-desc">{{ t("settings.version.setup.export.curseforgeHint") }}</div>
      <div class="setup-desc">{{ t("settings.version.setup.export.modpackTaskHint") }}</div>
      <div class="setup-btn-row">
        <button class="setup-btn" @click="exportModpack('mrpack')">
          <Package :size="15" />
          <span>{{ t("settings.version.setup.export.modpackButton") }}</span>
        </button>
        <button class="setup-btn" @click="exportModpack('curseforge')">
          <Package :size="15" />
          <span>{{ t("settings.version.setup.export.modpackCurseforgeButton") }}</span>
        </button>
      </div>
    </SetupCard>
  </div>
</template>

<style scoped>
</style>
