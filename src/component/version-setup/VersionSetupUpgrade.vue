<script setup lang="ts">
// 实例设置-游戏升级：选择目标 MC / ModLoader，检查 Mod 兼容性后原地升级并改名目录。
import { computed, onMounted, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, ArrowUpCircle, LoaderCircle } from "@lucide/vue";
import SetupCard from "../common/SetupCard.vue";
import DropSelect, { type SelOption } from "../common/DropSelect.vue";
import CheckToggle from "../common/CheckToggle.vue";
import LoadingSpinner from "../common/LoadingSpinner.vue";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import { LOADER_MIN_VERSION, LOADER_TITLES } from "../../utils/versionMeta";
import { releases, vdState } from "../../store/task/versionDownload";
import { dlState } from "../../store/task/download";
import {
  checkInstanceUpgrade,
  finishInstanceUpgrade,
  getInstanceUpgradeInfo,
  rollbackAndRefresh,
  startInstanceUpgrade,
  upgradeRunning,
  upgradeRunningVersion,
} from "../../store/task/instanceUpgrade";
import type { InstanceUpgradeInfo } from "../../types/game/InstanceUpgradeInfo";
import type { UpgradeModReport } from "../../types/game/UpgradeModReport";
import type { UpgradeModStatus } from "../../types/game/UpgradeModStatus";
import type { UpgradeReport } from "../../types/game/UpgradeReport";

const route = useRoute();
const { t } = useI18n();
const versionName = computed(() => String(route.query.name ?? ""));

const MAIN_LOADERS = ["fabric", "forge", "neoforge", "quilt"] as const;

const info = ref<InstanceUpgradeInfo | null>(null);
const loadingInfo = ref(true);
const infoError = ref("");

const targetMc = ref("");
const targetLoader = ref("vanilla");
const targetLoaderVersion = ref("");
const loaderVersions = ref<string[]>([]);
const loaderLoading = ref(false);
const loaderError = ref("");
let loaderSeq = 0;

const report = ref<UpgradeReport | null>(null);
const checking = ref(false);
const checkError = ref("");
let checkSeq = 0;
const disableIncompatible = ref(true);
const disableUnknown = ref(false);

const starting = ref(false);
const pendingAction = ref<"" | "commit" | "rollback">("");
let infoSeq = 0;
let initialized = false;

const targetMeta = computed(() =>
  vdState.manifest?.versions.find(version => version.id === targetMc.value) ?? null,
);
const currentLoader = computed(() => {
  const loader = info.value?.loader;
  return loader && loader !== "vanilla" ? loader : "vanilla";
});
const currentLoaderLabel = computed(() =>
  currentLoader.value === "vanilla"
    ? t("settings.version.setup.upgrade.noLoader")
    : LOADER_TITLES[currentLoader.value] ?? currentLoader.value,
);
const currentLoaderVersionLabel = computed(() =>
  info.value?.loader_version ?? t("settings.version.setup.upgrade.unknown"),
);

const mcOptions = computed<SelOption[]>(() => {
  const list = releases.value.map(version => ({ value: version.id, label: version.id }));
  const current = info.value?.mc_version;
  if (
    current
    && !list.some(option => option.value === current)
    && vdState.manifest?.versions.some(version => version.id === current)
  ) {
    list.unshift({ value: current, label: current });
  }
  return list;
});
const loaderOptions = computed<SelOption[]>(() => [
  {
    value: "vanilla",
    label: t("settings.version.setup.upgrade.loaderVanilla"),
  },
  ...MAIN_LOADERS.map(loader => ({ value: loader, label: LOADER_TITLES[loader] })),
]);
const loaderVersionOptions = computed<SelOption[]>(() =>
  loaderVersions.value.map(version => ({ value: version, label: version })),
);

function versionTuple(value: string): number[] {
  return value
    .split(/[^\d]+/)
    .filter(Boolean)
    .map(Number);
}

function versionGte(value: string, min: string): boolean {
  const a = versionTuple(value);
  const b = versionTuple(min);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (x !== y) return x > y;
  }
  return true;
}

const loaderSupported = computed(() => {
  if (targetLoader.value === "vanilla") return true;
  const min = LOADER_MIN_VERSION[targetLoader.value];
  return !min || !targetMc.value || versionGte(targetMc.value, min);
});

const sameTarget = computed(() => {
  const current = info.value;
  if (!current) return false;
  const targetVersion = targetLoader.value === "vanilla" ? "" : targetLoaderVersion.value;
  return (
    current.mc_version === targetMc.value
    && currentLoader.value === targetLoader.value
    && (current.loader_version ?? "") === targetVersion
  );
});

const loaderChanged = computed(
  () => !!info.value && currentLoader.value !== targetLoader.value,
);
const loaderDowngrade = computed(() => {
  const current = info.value;
  if (
    !current?.loader_version
    || targetLoader.value === "vanilla"
    || currentLoader.value !== targetLoader.value
    || !targetLoaderVersion.value
  ) {
    return false;
  }
  const a = versionTuple(targetLoaderVersion.value);
  const b = versionTuple(current.loader_version);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (x !== y) return x < y;
  }
  return false;
});

const hasIssues = computed(
  () => !!report.value && (report.value.incompatible > 0 || report.value.update > 0),
);
const pending = computed(() => info.value?.pending ?? null);
const upgradeTaskActive = computed(
  () => upgradeRunning.value && upgradeRunningVersion.value === versionName.value,
);
const pendingTo = computed(() => {
  const meta = pending.value;
  if (!meta) return "";
  if (meta.to_version && meta.to_version !== meta.from_version) return meta.to_version;
  return meta.target_loader === "vanilla"
    ? meta.target_mc
    : `${meta.target_mc}-${meta.target_loader}-${meta.target_loader_version ?? ""}`;
});
const pendingBusy = computed(
  () => !!info.value?.busy || upgradeTaskActive.value || pendingAction.value !== "" || starting.value,
);

const canCheck = computed(
  () =>
    !!info.value
    && !info.value.pending
    && !info.value.busy
    && !!targetMc.value
    && loaderSupported.value
    && (targetLoader.value === "vanilla" || !!targetLoaderVersion.value)
    && !checking.value,
);
const canStart = computed(() => {
  const current = info.value;
  if (!current || current.pending || current.busy) return false;
  if (starting.value || checking.value || !canCheck.value) return false;
  if (!targetMeta.value || sameTarget.value) return false;
  if (current.mod_count > 0 && !report.value) return false;
  return true;
});

const statusClass = (status: UpgradeModStatus) => `upg-status--${status}`;
const statusLabel = (status: UpgradeModStatus) =>
  t(`settings.version.setup.upgrade.status.${status}`);

function reasonText(mod: UpgradeModReport): string {
  if (mod.status === "update" && mod.compatible_version) {
    return t("settings.version.setup.upgrade.reasons.updateAvailable", {
      version: mod.compatible_version,
    });
  }
  if (!mod.reason) return "";
  return t(`settings.version.setup.upgrade.reasons.${mod.reason}`, {
    detail: mod.reason_detail ?? "",
  });
}

function metaText(mod: UpgradeModReport): string {
  const parts: string[] = [];
  if (mod.mod_id) parts.push(mod.mod_id);
  if (mod.current_version) {
    parts.push(
      t("settings.version.setup.upgrade.currentVersion", { version: mod.current_version }),
    );
  }
  if (mod.status !== "update" && mod.compatible_version) {
    parts.push(
      t("settings.version.setup.upgrade.compatibleVersion", {
        version: mod.compatible_version,
      }),
    );
  }
  return parts.join(" · ");
}

async function loadInfo() {
  if (!versionName.value) {
    loadingInfo.value = false;
    return;
  }
  const seq = ++infoSeq;
  loadingInfo.value = true;
  infoError.value = "";
  try {
    const data = await getInstanceUpgradeInfo(versionName.value);
    if (seq !== infoSeq) return;
    info.value = data;
  } catch (e) {
    if (seq !== infoSeq) return;
    info.value = null;
    infoError.value = errMsg(e);
  } finally {
    if (seq === infoSeq) loadingInfo.value = false;
  }
}

function initializeTarget() {
  const current = info.value;
  if (!current) return;
  const latest = releases.value[0]?.id;
  targetMc.value = latest ?? current.mc_version;
  targetLoader.value = MAIN_LOADERS.includes(current.loader as (typeof MAIN_LOADERS)[number])
    ? current.loader
    : "vanilla";
  targetLoaderVersion.value = current.loader_version ?? "";
  report.value = null;
  checkError.value = "";
  initialized = true;
}

async function loadLoaderVersions() {
  if (targetLoader.value === "vanilla" || !targetMc.value) {
    loaderVersions.value = [];
    loaderError.value = "";
    loaderLoading.value = false;
    return;
  }
  const seq = ++loaderSeq;
  loaderLoading.value = true;
  loaderError.value = "";
  loaderVersions.value = [];
  const command = {
    fabric: "get_fabric_versions",
    forge: "get_forge_versions",
    neoforge: "get_neoforge_versions",
    quilt: "get_quilt_versions",
  }[targetLoader.value];
  try {
    const versions = command
      ? await invoke<string[]>(command, { mcVersion: targetMc.value })
      : [];
    if (seq !== loaderSeq) return;
    loaderVersions.value = versions;
    if (versions.length === 0) {
      loaderError.value = t("settings.version.setup.upgrade.loaderEmpty");
      targetLoaderVersion.value = "";
      return;
    }
    const preferred = info.value?.loader === targetLoader.value
      ? info.value.loader_version ?? ""
      : "";
    targetLoaderVersion.value = versions.includes(preferred) ? preferred : versions[0];
  } catch (e) {
    if (seq !== loaderSeq) return;
    loaderError.value = errMsg(e);
    targetLoaderVersion.value = "";
  } finally {
    if (seq === loaderSeq) loaderLoading.value = false;
  }
}

async function checkCompatibility() {
  if (!canCheck.value) return;
  const seq = ++checkSeq;
  checking.value = true;
  checkError.value = "";
  try {
    const data = await checkInstanceUpgrade(
      versionName.value,
      targetMc.value,
      targetLoader.value,
      targetLoader.value === "vanilla" ? null : targetLoaderVersion.value || null,
    );
    if (seq !== checkSeq) return;
    report.value = data;
  } catch (e) {
    if (seq !== checkSeq) return;
    report.value = null;
    checkError.value = errMsg(e);
  } finally {
    if (seq === checkSeq) checking.value = false;
  }
}

function disableList(): string[] {
  const names = new Set<string>();
  for (const mod of report.value?.mods ?? []) {
    if (
      disableIncompatible.value
      && (mod.status === "incompatible" || mod.status === "update")
    ) {
      names.add(mod.file_name);
    } else if (disableUnknown.value && mod.status === "unknown") {
      names.add(mod.file_name);
    }
  }
  return [...names];
}

async function startUpgrade() {
  if (!canStart.value || !targetMeta.value) return;
  starting.value = true;
  try {
    await startInstanceUpgrade({
      versionName: versionName.value,
      versionUrl: targetMeta.value.url,
      targetMc: targetMc.value,
      targetLoader: targetLoader.value,
      targetLoaderVersion:
        targetLoader.value === "vanilla" ? null : targetLoaderVersion.value,
      disableMods: disableList(),
      displayName: t("settings.version.setup.upgrade.taskName", {
        from: info.value?.mc_version ?? versionName.value,
        to: targetMc.value,
      }),
    });
    showToast(t("settings.version.setup.upgrade.submitted"));
    await loadInfo();
  } catch (e) {
    await loadInfo();
    showToast(errMsg(e));
  } finally {
    starting.value = false;
  }
}

async function completePending() {
  if (pendingBusy.value || !pending.value?.install_done) return;
  pendingAction.value = "commit";
  try {
    await finishInstanceUpgrade(versionName.value);
    await loadInfo();
  } catch (e) {
    showToast(errMsg(e));
    await loadInfo();
  } finally {
    pendingAction.value = "";
  }
}

async function rollbackPending() {
  if (pendingBusy.value) return;
  pendingAction.value = "rollback";
  try {
    await rollbackAndRefresh(versionName.value);
    showToast(t("settings.version.setup.upgrade.rolledBack"));
    await loadInfo();
  } catch (e) {
    showToast(errMsg(e));
    await loadInfo();
  } finally {
    pendingAction.value = "";
  }
}

watch([targetMc, targetLoader], () => {
  if (!initialized) return;
  if (targetLoader.value === "vanilla") targetLoaderVersion.value = "";
  checkSeq += 1;
  checking.value = false;
  report.value = null;
  checkError.value = "";
  void loadLoaderVersions();
});
watch([targetMc, targetLoader, targetLoaderVersion], () => {
  if (!initialized) return;
  checkSeq += 1;
  checking.value = false;
  report.value = null;
  checkError.value = "";
});
watch(versionName, async (name, old) => {
  if (!name || name === old) return;
  initialized = false;
  checkSeq += 1;
  checking.value = false;
  report.value = null;
  info.value = null;
  await Promise.all([loadInfo(), vdState.fetchManifest()]);
  initializeTarget();
});
watch(upgradeRunning, running => {
  if (!running) void loadInfo();
});
watch(
  () => dlState.tasks.map(task => `${task.taskId}:${task.status}`).join("|"),
  () => {
    if (pending.value && !upgradeRunning.value) void loadInfo();
  },
);

onMounted(async () => {
  await Promise.all([loadInfo(), vdState.fetchManifest()]);
  initializeTarget();
});
</script>

<template>
  <div class="vs-page upg-page">
    <SetupCard
      :title="t('settings.version.setup.upgrade.currentTitle')"
      :desc="t('settings.version.setup.upgrade.currentDesc')"
    >
      <div v-if="loadingInfo" class="upg-state">
        <LoaderCircle :size="16" class="upg-spin" />
        <span>{{ t("settings.version.setup.upgrade.loadingInfo") }}</span>
      </div>
      <button v-else-if="infoError" type="button" class="upg-error" @click="loadInfo">
        <AlertCircle :size="18" />
        <span>{{ infoError }}</span>
      </button>
      <template v-else-if="info">
        <div class="upg-grid">
          <div class="upg-kv">
            <span class="upg-k">{{ t("settings.version.setup.upgrade.currentMc") }}</span>
            <span class="upg-v">{{ info.mc_version }}</span>
          </div>
          <div class="upg-kv">
            <span class="upg-k">{{ t("settings.version.setup.upgrade.currentLoader") }}</span>
            <span class="upg-v">{{ currentLoaderLabel }}</span>
          </div>
          <div class="upg-kv">
            <span class="upg-k">{{ t("settings.version.setup.upgrade.currentLoaderVersion") }}</span>
            <span class="upg-v">{{ currentLoaderVersionLabel }}</span>
          </div>
          <div class="upg-kv">
            <span class="upg-k">{{ t("settings.version.setup.upgrade.currentMods") }}</span>
            <span class="upg-v">{{ t("settings.version.setup.upgrade.modCount", { count: info.mod_count }) }}</span>
          </div>
        </div>
        <div v-if="info.loader === 'optifine'" class="upg-note">
          {{ t("settings.version.setup.upgrade.optifineHint") }}
        </div>
        <div v-if="info.busy" class="upg-note upg-warn">
          {{ t("settings.version.setup.upgrade.pendingInProgress") }}
        </div>
      </template>
    </SetupCard>

    <SetupCard
      v-if="pending"
      :title="t('settings.version.setup.upgrade.pendingTitle')"
      :desc="t('settings.version.setup.upgrade.pendingTarget', { from: pending.from_version, to: pendingTo })"
    >
      <div class="upg-pending">
        <span class="upg-note" :class="{ 'upg-warn': !pending.install_done }">
          {{
            upgradeTaskActive || info?.busy
              ? t("settings.version.setup.upgrade.pendingInProgress")
              : pending.install_done
                ? t("settings.version.setup.upgrade.pendingReady")
                : t("settings.version.setup.upgrade.pendingRollbackOnly")
          }}
        </span>
        <div class="upg-actions">
          <button
            type="button"
            class="setup-btn"
            :disabled="pendingBusy || !pending.install_done"
            @click="completePending"
          >
            {{ pendingAction === "commit" ? t("settings.version.setup.upgrade.starting") : t("settings.version.setup.upgrade.complete") }}
          </button>
          <button
            type="button"
            class="setup-btn upg-danger"
            :disabled="pendingBusy"
            @click="rollbackPending"
          >
            {{ pendingAction === "rollback" ? t("settings.version.setup.upgrade.starting") : t("settings.version.setup.upgrade.rollback") }}
          </button>
        </div>
      </div>
    </SetupCard>

    <SetupCard
      :title="t('settings.version.setup.upgrade.targetTitle')"
      :desc="t('settings.version.setup.upgrade.targetDesc')"
    >
      <div class="upg-form">
        <div class="upg-form-row">
          <label class="upg-label" for="upg-mc">{{ t("settings.version.setup.upgrade.targetMc") }}</label>
          <DropSelect
            id="upg-mc"
            v-model="targetMc"
            :options="mcOptions"
            :disabled="loadingInfo || !info"
            class="upg-select"
          />
        </div>
        <div class="upg-form-row">
          <label class="upg-label" for="upg-loader">{{ t("settings.version.setup.upgrade.targetLoader") }}</label>
          <DropSelect
            id="upg-loader"
            v-model="targetLoader"
            :options="loaderOptions"
            :disabled="loadingInfo || !info"
            class="upg-select"
          />
        </div>
        <div v-if="targetLoader !== 'vanilla'" class="upg-form-row">
          <label class="upg-label" for="upg-loader-version">{{ t("settings.version.setup.upgrade.targetLoaderVersion") }}</label>
          <DropSelect
            id="upg-loader-version"
            v-model="targetLoaderVersion"
            :options="loaderVersionOptions"
            :disabled="loaderLoading || !!loaderError"
            class="upg-select"
            :placeholder="t('settings.version.setup.upgrade.loaderLoading')"
          />
        </div>
      </div>

      <div v-if="vdState.loading" class="upg-state">
        <LoaderCircle :size="16" class="upg-spin" />
        <span>{{ t("settings.version.setup.upgrade.manifestLoading") }}</span>
      </div>
      <div v-else-if="vdState.error" class="upg-inline-error">
        <span class="upg-note upg-warn" :title="vdState.error">
          {{ t("settings.version.setup.upgrade.manifestFailed") }}
        </span>
        <button type="button" class="setup-btn" @click="vdState.fetchManifest()">
          {{ t("common.action.retry") }}
        </button>
      </div>
      <div v-else-if="loaderLoading" class="upg-state">
        <LoaderCircle :size="16" class="upg-spin" />
        <span>{{ t("settings.version.setup.upgrade.loaderLoading") }}</span>
      </div>
      <div v-else-if="loaderError" class="upg-inline-error">
        <span class="upg-note upg-warn">{{ loaderError }}</span>
        <button type="button" class="setup-btn" @click="loadLoaderVersions">
          {{ t("common.action.retry") }}
        </button>
      </div>
      <div v-if="!loaderSupported" class="upg-note upg-warn">
        {{ t("settings.version.setup.upgrade.loaderUnsupported") }}
      </div>
      <div v-if="sameTarget" class="upg-note upg-warn">
        {{ t("settings.version.setup.upgrade.sameTarget") }}
      </div>
      <div v-if="!targetMeta" class="upg-note upg-warn">
        {{ t("settings.version.setup.upgrade.noTargetUrl") }}
      </div>
      <div v-if="loaderChanged" class="upg-note upg-warn">
        {{ t("settings.version.setup.upgrade.warningLoaderChange") }}
      </div>
      <div v-if="loaderDowngrade" class="upg-note upg-warn">
        {{ t("settings.version.setup.upgrade.warningDowngrade") }}
      </div>
      <div class="upg-note">{{ t("settings.version.setup.upgrade.startHint") }}</div>
      <div v-if="info && targetMc && info.mc_version !== targetMc" class="upg-note upg-warn">
        {{ t("settings.version.setup.upgrade.warningWorlds") }}
      </div>
      <div class="upg-note">{{ t("settings.version.setup.upgrade.closeGameHint") }}</div>
    </SetupCard>

    <SetupCard
      :title="t('settings.version.setup.upgrade.modsTitle')"
      :desc="t('settings.version.setup.upgrade.modsDesc')"
    >
      <div v-if="info && info.mod_count === 0" class="upg-note">
        {{ t("settings.version.setup.upgrade.noMods") }}
      </div>
      <template v-else>
        <div class="upg-toolbar">
          <button
            type="button"
            class="setup-btn"
            :disabled="!canCheck"
            @click="checkCompatibility"
          >
            <LoaderCircle v-if="checking" :size="15" class="upg-spin" />
            <span>
              {{
                checking
                  ? t("settings.version.setup.upgrade.checking")
                  : report
                    ? t("settings.version.setup.upgrade.recheck")
                    : t("settings.version.setup.upgrade.check")
              }}
            </span>
          </button>
          <span class="upg-note">{{ t("settings.version.setup.upgrade.checkHint") }}</span>
        </div>
        <div v-if="checkError" class="upg-note upg-warn">{{ checkError }}</div>
        <div v-if="report" class="upg-report">
          <div class="upg-summary">
            {{ t("settings.version.setup.upgrade.reportSummary", {
              compatible: report.compatible,
              update: report.update,
              incompatible: report.incompatible,
              unknown: report.unknown,
            }) }}
          </div>
          <div v-if="report.network_error" class="upg-note upg-warn">
            {{ t("settings.version.setup.upgrade.networkFallback") }}
          </div>
          <div class="upg-mod-list">
            <div v-for="mod in report.mods" :key="mod.file_name" class="upg-mod">
              <span class="upg-status" :class="statusClass(mod.status)">{{ statusLabel(mod.status) }}</span>
              <span class="upg-mod-main">
                <span class="upg-mod-name">{{ mod.file_name }}</span>
                <span class="upg-mod-meta">{{ metaText(mod) }}</span>
              </span>
              <span class="upg-mod-reason">{{ reasonText(mod) }}</span>
            </div>
          </div>
        </div>
        <div class="upg-toggles">
          <div class="upg-toggle">
            <CheckToggle v-model="disableIncompatible">
              {{ t("settings.version.setup.upgrade.disableIncompatible") }}
            </CheckToggle>
            <span class="upg-note">{{ t("settings.version.setup.upgrade.disableIncompatibleDesc") }}</span>
          </div>
          <div class="upg-toggle">
            <CheckToggle v-model="disableUnknown">
              {{ t("settings.version.setup.upgrade.disableUnknown") }}
            </CheckToggle>
            <span class="upg-note">{{ t("settings.version.setup.upgrade.disableUnknownDesc") }}</span>
          </div>
        </div>
        <div v-if="hasIssues && !disableIncompatible" class="upg-note upg-warn">
          {{ t("settings.version.setup.upgrade.warningNoDisable") }}
        </div>
      </template>
    </SetupCard>

    <div class="upg-submit">
      <span v-if="info && info.mod_count > 0 && !report" class="upg-note">
        {{ t("settings.version.setup.upgrade.needCheck") }}
      </span>
      <button
        type="button"
        class="setup-btn upg-primary"
        :disabled="!canStart"
        @click="startUpgrade"
      >
        <ArrowUpCircle :size="16" />
        <span>
          {{
            starting
              ? t("settings.version.setup.upgrade.starting")
              : t("settings.version.setup.upgrade.start")
          }}
        </span>
      </button>
    </div>

    <div v-if="starting" class="upg-overlay" role="status">
      <LoadingSpinner :text="t('settings.version.setup.upgrade.starting')" />
    </div>
  </div>
</template>

<style scoped>
.upg-page {
  padding-bottom: 16px;
}

.upg-state {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 0;
  color: var(--text-tertiary);
  font-size: var(--fs-sm);
}

.upg-error {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--color-danger);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-danger);
  font-size: var(--fs-sm);
  text-align: left;
  cursor: pointer;
}

.upg-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px 18px;
}

.upg-kv {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.upg-k {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.upg-v {
  font-size: var(--fs-base);
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.upg-note {
  margin-top: 8px;
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-tertiary);
}

.upg-warn {
  color: var(--color-warning);
}

.upg-inline-error {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  margin-top: 8px;
}

.upg-inline-error .upg-note {
  margin-top: 0;
}

.upg-form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.upg-form-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.upg-label {
  flex: none;
  width: 140px;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

.upg-select {
  flex: 1;
  min-width: 0;
}

.upg-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.upg-toolbar .upg-note {
  margin-top: 0;
}

.upg-report {
  margin-top: 10px;
}

.upg-summary {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

.upg-mod-list {
  margin-top: 8px;
  max-height: 360px;
  overflow-y: auto;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
}

.upg-mod {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 7px 10px;
}

.upg-mod + .upg-mod {
  border-top: 1px solid var(--border-default);
}

.upg-status {
  flex: none;
  min-width: 56px;
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  font-size: var(--fs-2xs);
  font-weight: 600;
  text-align: center;
  white-space: nowrap;
}

.upg-status--compatible {
  color: var(--color-success);
  background: var(--color-brand-bg);
}

.upg-status--update {
  color: var(--color-warning);
  background: var(--color-warning-bg);
}

.upg-status--incompatible {
  color: var(--color-danger);
  background: var(--color-danger-bg);
}

.upg-status--unknown {
  color: var(--text-tertiary);
  background: var(--surface-hover);
}

.upg-mod-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.upg-mod-name {
  font-size: var(--fs-sm);
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.upg-mod-meta,
.upg-mod-reason {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}

.upg-mod-reason {
  flex: none;
  max-width: 42%;
  text-align: right;
}

.upg-toggles {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
}

.upg-toggle {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.upg-toggle .upg-note {
  margin-top: 0;
}

.upg-pending {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.upg-pending .upg-note {
  margin-top: 0;
}

.upg-actions {
  display: flex;
  gap: 8px;
}

.upg-danger {
  border-color: var(--color-danger);
  color: var(--color-danger);
}

.upg-submit {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  margin-top: 12px;
}

.upg-submit .upg-note {
  margin-top: 0;
}

.upg-primary {
  border-color: var(--color-brand);
  background: var(--color-brand);
  color: var(--text-inverse);
  font-weight: 600;
}

.upg-primary:hover:not(:disabled) {
  filter: brightness(1.12);
}

.upg-spin {
  animation: upg-spin 0.9s linear infinite;
}

@keyframes upg-spin {
  to {
    transform: rotate(360deg);
  }
}

.upg-overlay {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--overlay);
}

@media (max-width: 760px) {
  .upg-grid {
    grid-template-columns: 1fr;
  }

  .upg-form-row {
    align-items: stretch;
    flex-direction: column;
    gap: 4px;
  }

  .upg-label {
    width: auto;
  }
}
</style>
