<script setup lang="ts">
// 实例设置-游戏升级：选择目标 MC / ModLoader，检查 Mod 兼容性后原地升级并改名目录。
import { computed, onMounted, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { AlertCircle, ArrowUpCircle, Box, CircleCheck, LoaderCircle, Puzzle, Rocket, X } from "@lucide/vue";
import DropSelect, { type SelOption } from "../common/DropSelect.vue";
import CheckToggle from "../common/CheckToggle.vue";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import { LOADER_ICONS, LOADER_MIN_VERSION, LOADER_TITLES } from "../../utils/versionMeta";
import { releases, vdState } from "../../store/task/versionDownload";
import { dlState } from "../../store/task/download";
import { loadVersionIcon, versionIcons } from "../../store/task/versionIcon";
import {
  checkInstanceUpgrade,
  fetchUpgradeLoaderVersions,
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
const currentLoaderIcon = computed(
  () => LOADER_ICONS[currentLoader.value] ?? LOADER_ICONS.vanilla,
);
const currentInstanceIcon = computed(
  () => versionIcons[versionName.value] ?? currentLoaderIcon.value,
);

const loaderTiles = computed(() => [
  {
    value: "vanilla",
    label: t("settings.version.setup.upgrade.noLoader"),
    icon: LOADER_ICONS.vanilla,
  },
  ...MAIN_LOADERS.map(loader => ({
    value: loader,
    label: LOADER_TITLES[loader],
    icon: LOADER_ICONS[loader],
  })),
]);

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
const targetSummary = computed(() => {
  if (!targetMc.value) return t("settings.version.setup.upgrade.unknown");
  const loader = targetLoader.value === "vanilla"
    ? t("settings.version.setup.upgrade.noLoader")
    : `${LOADER_TITLES[targetLoader.value] ?? targetLoader.value} ${targetLoaderVersion.value}`.trim();
  return `${targetMc.value} · ${loader}`;
});

const canCheck = computed(
  () =>
    !!info.value
    && !info.value.pending
    && !info.value.busy
    && !!targetMc.value
    && loaderSupported.value
    && !loaderLoading.value
    && !loaderError.value
    && (targetLoader.value === "vanilla"
      || (!!targetLoaderVersion.value
        && loaderVersions.value.includes(targetLoaderVersion.value)))
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
function statusIcon(status: UpgradeModStatus) {
  switch (status) {
    case "compatible":
      return CircleCheck;
    case "update":
      return ArrowUpCircle;
    case "incompatible":
      return X;
    default:
      return AlertCircle;
  }
}

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
  void loadVersionIcon(versionName.value);
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
    targetLoaderVersion.value = "";
    return;
  }
  const seq = ++loaderSeq;
  loaderLoading.value = true;
  loaderError.value = "";
  loaderVersions.value = [];
  // 切 Loader / MC 后旧版本（如 NeoForge 26.3.0.113）不能留给 Quilt 使用。
  targetLoaderVersion.value = "";
  try {
    const versions = await fetchUpgradeLoaderVersions(targetLoader.value, targetMc.value);
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

/** 安装已完成（install_done）的备份自动提交；失败保留手动按钮，同一 pending 只自动尝试一次。 */
let autoCommitKey = "";
async function autoCompleteReady() {
  const backup = info.value?.pending;
  if (
    !backup?.install_done
    || info.value?.busy
    || upgradeRunning.value
    || starting.value
    || pendingAction.value
  ) {
    return;
  }
  const key = `${versionName.value}\u0000${backup.to_version ?? ""}\u0000${backup.created_at}`;
  if (autoCommitKey === key) return;
  autoCommitKey = key;
  await completePending();
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

watch(
  [targetMc, targetLoader],
  () => {
    if (!initialized) return;
    if (targetLoader.value === "vanilla") targetLoaderVersion.value = "";
    checkSeq += 1;
    checking.value = false;
    report.value = null;
    checkError.value = "";
    void loadLoaderVersions();
  },
  { flush: "sync" },
);
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
  () => [
    info.value?.pending?.install_done,
    info.value?.pending?.to_version,
    info.value?.pending?.created_at,
    info.value?.busy,
    upgradeRunning.value,
  ],
  () => {
    void autoCompleteReady();
  },
  { immediate: true },
);
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
    <section class="upg-card">
      <header class="upg-card-head">
        <span class="upg-card-icon">
          <Box :size="18" :stroke-width="1.8" />
        </span>
        <span class="upg-card-titles">
          <span class="upg-card-title">{{ t("settings.version.setup.upgrade.currentTitle") }}</span>
          <span class="upg-card-desc">{{ t("settings.version.setup.upgrade.currentDesc") }}</span>
        </span>
      </header>

      <div v-if="loadingInfo" class="upg-state">
        <LoaderCircle :size="16" class="upg-spin" />
        <span>{{ t("settings.version.setup.upgrade.loadingInfo") }}</span>
      </div>
      <button v-else-if="infoError" type="button" class="upg-error" @click="loadInfo">
        <AlertCircle :size="18" />
        <span>{{ infoError }}</span>
      </button>
      <template v-else-if="info">
        <div class="upg-instance">
          <span class="upg-instance-icon">
            <img :src="currentInstanceIcon" :alt="versionName" />
          </span>
          <span class="upg-instance-body">
            <span class="upg-instance-name">{{ versionName }}</span>
            <span class="upg-chips">
              <span class="upg-chip">
                <Box :size="13" />
                {{ info.mc_version }}
              </span>
              <span class="upg-chip">
                <img class="upg-chip-icon" :src="currentLoaderIcon" alt="" />
                {{ currentLoaderLabel }}
                <span class="upg-chip-dim">{{ currentLoaderVersionLabel }}</span>
              </span>
              <span class="upg-chip">
                <Puzzle :size="13" />
                {{ t("settings.version.setup.upgrade.modCount", { count: info.mod_count }) }}
              </span>
            </span>
          </span>
        </div>
        <div v-if="info.loader === 'optifine'" class="upg-note">
          {{ t("settings.version.setup.upgrade.optifineHint") }}
        </div>
        <div v-if="info.busy" class="upg-note upg-warn">
          {{ t("settings.version.setup.upgrade.pendingInProgress") }}
        </div>
      </template>
    </section>

    <section v-if="pending" class="upg-card upg-card--warning">
      <header class="upg-card-head">
        <span class="upg-card-icon upg-card-icon--warning">
          <AlertCircle :size="18" :stroke-width="1.8" />
        </span>
        <span class="upg-card-titles">
          <span class="upg-card-title">{{ t("settings.version.setup.upgrade.pendingTitle") }}</span>
          <span class="upg-card-desc">{{ t("settings.version.setup.upgrade.pendingTarget", { from: pending.from_version, to: pendingTo }) }}</span>
        </span>
      </header>
      <div class="upg-pending-row">
        <span class="upg-note" :class="{ 'upg-warn': !pending.install_done }">
          {{
            upgradeTaskActive || info?.busy
              ? t("settings.version.setup.upgrade.pendingInProgress")
              : pending.install_done
                ? t("settings.version.setup.upgrade.pendingReady")
                : t("settings.version.setup.upgrade.pendingRollbackOnly")
          }}
        </span>
        <span class="upg-card-actions">
          <button
            type="button"
            class="upg-btn upg-btn--primary"
            :disabled="pendingBusy || !pending.install_done"
            @click="completePending"
          >
            {{ pendingAction === "commit" ? t("settings.version.setup.upgrade.starting") : t("settings.version.setup.upgrade.complete") }}
          </button>
          <button
            type="button"
            class="upg-btn upg-btn--danger"
            :disabled="pendingBusy"
            @click="rollbackPending"
          >
            {{ pendingAction === "rollback" ? t("settings.version.setup.upgrade.starting") : t("settings.version.setup.upgrade.rollback") }}
          </button>
        </span>
      </div>
    </section>

    <section class="upg-card">
      <header class="upg-card-head">
        <span class="upg-card-icon">
          <ArrowUpCircle :size="18" :stroke-width="1.8" />
        </span>
        <span class="upg-card-titles">
          <span class="upg-card-title">{{ t("settings.version.setup.upgrade.targetTitle") }}</span>
          <span class="upg-card-desc">{{ t("settings.version.setup.upgrade.targetDesc") }}</span>
        </span>
      </header>

      <div class="upg-fields">
        <div class="upg-field">
          <span class="upg-field-label">
            <Box :size="14" />
            {{ t("settings.version.setup.upgrade.targetMc") }}
          </span>
          <DropSelect
            id="upg-mc"
            v-model="targetMc"
            :options="mcOptions"
            :disabled="loadingInfo || !info"
            class="upg-select"
          />
        </div>

        <div class="upg-field">
          <span class="upg-field-label">
            <Puzzle :size="14" />
            {{ t("settings.version.setup.upgrade.targetLoader") }}
          </span>
          <div class="upg-loader-grid">
            <button
              v-for="tile in loaderTiles"
              :key="tile.value"
              type="button"
              class="upg-loader-tile"
              :class="{ active: targetLoader === tile.value }"
              :disabled="loadingInfo || !info"
              @click="targetLoader = tile.value"
            >
              <img :src="tile.icon" alt="" />
              <span>{{ tile.label }}</span>
            </button>
          </div>
        </div>

        <div v-if="targetLoader !== 'vanilla'" class="upg-field">
          <span class="upg-field-label">
            <Box :size="14" />
            {{ t("settings.version.setup.upgrade.targetLoaderVersion") }}
          </span>
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
        <button type="button" class="upg-btn upg-btn--ghost" @click="vdState.fetchManifest()">
          {{ t("common.action.retry") }}
        </button>
      </div>
      <div v-else-if="loaderLoading" class="upg-state">
        <LoaderCircle :size="16" class="upg-spin" />
        <span>{{ t("settings.version.setup.upgrade.loaderLoading") }}</span>
      </div>
      <div v-else-if="loaderError" class="upg-inline-error">
        <span class="upg-note upg-warn">{{ loaderError }}</span>
        <button type="button" class="upg-btn upg-btn--ghost" @click="loadLoaderVersions">
          {{ t("common.action.retry") }}
        </button>
      </div>

      <div class="upg-messages">
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
      </div>
    </section>

    <section class="upg-card">
      <header class="upg-card-head">
        <span class="upg-card-icon">
          <Puzzle :size="18" :stroke-width="1.8" />
        </span>
        <span class="upg-card-titles">
          <span class="upg-card-title">{{ t("settings.version.setup.upgrade.modsTitle") }}</span>
          <span class="upg-card-desc">{{ t("settings.version.setup.upgrade.modsDesc") }}</span>
        </span>
      </header>

      <div v-if="info && info.mod_count === 0" class="upg-note">
        {{ t("settings.version.setup.upgrade.noMods") }}
      </div>
      <template v-else>
        <div class="upg-toolbar">
          <button
            type="button"
            class="upg-btn upg-btn--ghost"
            :disabled="!canCheck"
            @click="checkCompatibility"
          >
            <LoaderCircle v-if="checking" :size="15" class="upg-spin" />
            <CircleCheck v-else-if="report" :size="15" />
            <Puzzle v-else :size="15" />
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
          <div class="upg-stats">
            <span class="upg-stat upg-stat--compatible">
              <CircleCheck :size="14" />
              <strong>{{ report.compatible }}</strong>
              {{ statusLabel("compatible") }}
            </span>
            <span class="upg-stat upg-stat--update">
              <ArrowUpCircle :size="14" />
              <strong>{{ report.update }}</strong>
              {{ statusLabel("update") }}
            </span>
            <span class="upg-stat upg-stat--incompatible">
              <X :size="14" />
              <strong>{{ report.incompatible }}</strong>
              {{ statusLabel("incompatible") }}
            </span>
            <span class="upg-stat upg-stat--unknown">
              <AlertCircle :size="14" />
              <strong>{{ report.unknown }}</strong>
              {{ statusLabel("unknown") }}
            </span>
          </div>
          <div v-if="report.network_error" class="upg-note upg-warn">
            {{ t("settings.version.setup.upgrade.networkFallback") }}
          </div>
          <div class="upg-mod-list">
            <div v-for="mod in report.mods" :key="mod.file_name" class="upg-mod">
              <span class="upg-status" :class="statusClass(mod.status)">
                <component :is="statusIcon(mod.status)" :size="13" />
                <span>{{ statusLabel(mod.status) }}</span>
              </span>
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
    </section>

    <section class="upg-card upg-action-card">
      <header class="upg-card-head">
        <span class="upg-card-icon">
          <Rocket :size="18" :stroke-width="1.8" />
        </span>
        <span class="upg-card-titles">
          <span class="upg-card-title">{{ t("settings.version.setup.upgrade.actionTitle") }}</span>
          <span class="upg-card-desc">{{ t("settings.version.setup.upgrade.actionDesc") }}</span>
        </span>
      </header>
      <div class="upg-action-body">
        <span class="upg-action-summary">
          <span class="upg-action-target">{{ targetSummary }}</span>
          <span v-if="info && info.mod_count > 0 && !report" class="upg-note">
            {{ t("settings.version.setup.upgrade.needCheck") }}
          </span>
        </span>
        <button
          type="button"
          class="upg-btn upg-btn--primary upg-btn--lg"
          :disabled="!canStart"
          @click="startUpgrade"
        >
          <LoaderCircle v-if="starting" :size="17" class="upg-spin" />
          <Rocket v-else :size="17" />
          <span>
            {{
              starting
                ? t("settings.version.setup.upgrade.starting")
                : t("settings.version.setup.upgrade.start")
            }}
          </span>
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.upg-page {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-bottom: 18px;
}

.upg-card {
  padding: 16px 18px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-card);
}

.upg-card--warning {
  border-left: 3px solid var(--color-warning);
}

.upg-card-head {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 14px;
}

.upg-card-icon {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: 10px;
  color: var(--color-brand);
  background: var(--color-brand-bg);
}

.upg-card-icon--warning {
  color: var(--color-warning);
  background: var(--color-warning-bg);
}

.upg-card-titles {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.upg-card-title {
  font-size: var(--fs-md);
  font-weight: 600;
  line-height: 1.35;
  color: var(--text-primary);
}

.upg-card-desc {
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-secondary);
}

.upg-state {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 0;
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

.upg-instance {
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
}

.upg-instance-icon {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 52px;
  height: 52px;
  overflow: hidden;
  border: 1px solid var(--border-default);
  border-radius: 12px;
  background: var(--surface-page);
}

.upg-instance-icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.upg-instance-body {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.upg-instance-name {
  overflow: hidden;
  color: var(--text-primary);
  font-size: var(--fs-md);
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.upg-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.upg-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 100%;
  padding: 3px 8px;
  border: 1px solid var(--border-default);
  border-radius: 999px;
  background: var(--surface-page);
  color: var(--text-secondary);
  font-size: var(--fs-xs);
  line-height: 1.4;
  white-space: nowrap;
}

.upg-chip-dim {
  color: var(--text-tertiary);
}

.upg-chip-icon {
  width: 14px;
  height: 14px;
  flex: none;
  object-fit: contain;
}

.upg-note {
  margin-top: 6px;
  font-size: var(--fs-sm);
  line-height: 1.65;
  color: var(--text-tertiary);
}

.upg-warn {
  color: var(--color-warning);
}

.upg-messages {
  margin-top: 10px;
}

.upg-messages .upg-note:first-child {
  margin-top: 0;
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

.upg-fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.upg-field {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.upg-field-label {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  font-weight: 500;
}

.upg-select {
  width: 100%;
}

.upg-loader-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(96px, 1fr));
  gap: 8px;
}

.upg-loader-tile {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 8px;
  min-width: 0;
  height: 38px;
  padding: 0 10px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-page);
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: all var(--dur-fast);
}

.upg-loader-tile img {
  width: 20px;
  height: 20px;
  flex: none;
  object-fit: contain;
}

.upg-loader-tile span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.upg-loader-tile:hover:not(:disabled) {
  border-color: var(--border-strong);
  color: var(--text-primary);
}

.upg-loader-tile.active {
  border-color: var(--color-brand);
  background: var(--color-brand-bg);
  color: var(--color-brand);
  font-weight: 600;
}

.upg-loader-tile:disabled {
  opacity: 0.5;
  cursor: not-allowed;
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

.upg-stats {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.upg-stat {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 9px;
  border-radius: 999px;
  font-size: var(--fs-xs);
  font-weight: 500;
}

.upg-stat strong {
  font-size: var(--fs-sm);
}

.upg-stat--compatible {
  color: var(--color-success);
  background: var(--color-brand-bg);
}

.upg-stat--update {
  color: var(--color-warning);
  background: var(--color-warning-bg);
}

.upg-stat--incompatible {
  color: var(--color-danger);
  background: var(--color-danger-bg);
}

.upg-stat--unknown {
  color: var(--text-tertiary);
  background: var(--surface-hover);
}

.upg-mod-list {
  max-height: 340px;
  margin-top: 10px;
  overflow-y: auto;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-page);
}

.upg-mod {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 10px;
}

.upg-mod + .upg-mod {
  border-top: 1px solid var(--border-default);
}

.upg-status {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  min-width: 64px;
  padding: 2px 7px;
  border-radius: 999px;
  font-size: var(--fs-2xs);
  font-weight: 600;
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
  overflow: hidden;
  color: var(--text-primary);
  font-size: var(--fs-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.upg-mod-meta,
.upg-mod-reason {
  font-size: var(--fs-xs);
  line-height: 1.5;
  color: var(--text-tertiary);
}

.upg-mod-reason {
  flex: none;
  max-width: 40%;
  text-align: right;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.upg-toggles {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px solid var(--border-default);
}

.upg-toggle {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.upg-toggle .upg-note {
  margin-top: 0;
}

.upg-pending-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.upg-pending-row .upg-note {
  margin-top: 0;
}

.upg-card-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.upg-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  height: 32px;
  padding: 0 13px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--fs-base);
  font-weight: 500;
  white-space: nowrap;
  cursor: pointer;
  transition: all var(--dur-fast);
}

.upg-btn:hover:not(:disabled) {
  border-color: var(--color-brand);
  color: var(--color-brand);
}

.upg-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.upg-btn--primary {
  border-color: var(--color-brand);
  background: var(--color-brand);
  color: var(--text-inverse);
}

.upg-btn--primary:hover:not(:disabled) {
  filter: brightness(1.1);
  color: var(--text-inverse);
}

.upg-btn--danger {
  border-color: var(--color-danger);
  color: var(--color-danger);
}

.upg-btn--danger:hover:not(:disabled) {
  background: var(--color-danger-bg);
  color: var(--color-danger);
}

.upg-btn--ghost {
  height: 30px;
  border-color: var(--border-default);
  background: var(--surface-page);
}

.upg-btn--lg {
  height: 40px;
  padding: 0 20px;
  font-size: var(--fs-md);
  font-weight: 600;
}

.upg-action-card {
  border-color: var(--border-strong);
}

.upg-action-body {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.upg-action-summary {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.upg-action-target {
  color: var(--text-primary);
  font-size: var(--fs-base);
  font-weight: 600;
}

.upg-action-summary .upg-note {
  margin-top: 0;
}

.upg-spin {
  animation: upg-spin 0.9s linear infinite;
}

@keyframes upg-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (max-width: 760px) {
  .upg-card {
    padding: 14px;
  }

  .upg-instance {
    align-items: flex-start;
  }

  .upg-instance-icon {
    width: 44px;
    height: 44px;
  }

  .upg-loader-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .upg-action-body {
    align-items: stretch;
    flex-direction: column;
  }

  .upg-btn--lg {
    width: 100%;
  }

  .upg-mod {
    flex-wrap: wrap;
  }

  .upg-mod-reason {
    max-width: 100%;
    text-align: left;
  }
}
</style>
