<script setup lang="ts">
// 版本设置-启动选项：实例级启动参数快照（内存/Java/JVM/窗口等），修改即保存。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { RefreshCw, Undo2 } from "@lucide/vue";
import DropSelect, { type SelOption } from "../common/DropSelect.vue";
import MemoryUsageBar from "../launch/MemoryUsageBar.vue";
import CheckToggle from "../common/CheckToggle.vue";
import RangeSlider from "../common/RangeSlider.vue";
import LoadingSpinner from "../common/LoadingSpinner.vue";
import ArgsListInput from "../settings/ArgsListInput.vue";
import { memState, startMemoryMonitor } from "../../store/app/memory";
import { fmtGb } from "../../utils/format";
import {
  autoAllocMb, memCeilMb, snapMemMb, MEM_FLOOR_MB, MEM_STEP_MB,
} from "../../utils/memory";
import { jdkList, loadConfig, refreshJdks } from "../../store/config/jdkOptions";
import { gcOptions } from "../../store/config/launchOptions";
import {
  getInstanceLaunchOptions,
  saveInstanceLaunchOptions,
} from "../../store/task/versionSetup";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import {
  MINECRAFT_LANGUAGES, minecraftLanguageLabel, normalizeGameLanguage,
} from "../../utils/minecraftLanguages";
import type { InstanceLaunchOptions } from "../../types/launcher/InstanceLaunchOptions";
import type { InstanceLaunchState } from "../../types/launcher/InstanceLaunchState";
import type { GcType } from "../../types/launcher/GcType";
import type { ProcessPriority } from "../../types/launcher/ProcessPriority";

const route = useRoute();
const { t } = useI18n();
// 组件卸载时 route.query 可能先清空；固定本次编辑的实例 id，避免延迟保存写到空目录。
const versionId = String(route.query.name ?? "");

const AUTO_VALUE = "__auto__";

const loading = ref(true);
const error = ref("");
const state = ref<InstanceLaunchState | null>(null);
const draft = ref<InstanceLaunchOptions | null>(null);
const jvmArgs = ref<string[]>([]);
const gameArgs = ref<string[]>([]);
const saving = ref(false);
const saved = ref(false);
const refreshing = ref(false);

let saveQueue: Promise<void> = Promise.resolve();
let saveTimer: number | undefined;
let savedTimer: number | undefined;
let pending = false;

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function setDraft(instance: InstanceLaunchOptions | null) {
  draft.value = instance ? clone(instance) : null;
  let languageChanged = false;
  if (draft.value) {
    const normalized = normalizeGameLanguage(draft.value.game_language);
    languageChanged = normalized !== draft.value.game_language;
    draft.value.game_language = normalized;
  }
  jvmArgs.value = [...(instance?.jvm_args ?? [])];
  gameArgs.value = [...(instance?.game_args ?? [])];
  // 旧默认 auto_zh_CN 已迁移为系统语言：落盘保证启动时与界面一致
  if (languageChanged) scheduleSave();
}

async function load() {
  if (!versionId) {
    loading.value = false;
    return;
  }
  loading.value = true;
  error.value = "";
  try {
    state.value = await getInstanceLaunchOptions(versionId);
    setDraft(state.value.instance);
    await loadConfig();
  } catch (e) {
    error.value = errMsg(e);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  // 内存状态只在本页展示：进页再启动轮询，避免启动器常驻 2s IPC。
  startMemoryMonitor();
  void load();
});
onBeforeUnmount(() => {
  window.clearTimeout(saveTimer);
  window.clearTimeout(savedTimer);
  if (pending) void saveNow();
});

/** 所有写入串行排队，避免快速切换字段时旧值后落盘。 */
function enqueueSave(options: InstanceLaunchOptions | null): Promise<void> {
  if (!versionId) return saveQueue;
  const next = saveQueue.catch(() => undefined).then(() =>
    saveInstanceLaunchOptions(versionId, options),
  );
  saveQueue = next;
  return next;
}

function scheduleSave() {
  if (!draft.value) return;
  pending = true;
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void saveNow(), 150);
}

function updateJvmArgs(next: string[]) {
  jvmArgs.value = next;
  scheduleSave();
}

function updateGameArgs(next: string[]) {
  gameArgs.value = next;
  scheduleSave();
}

async function saveNow() {
  if (!draft.value) return;
  pending = false;
  draft.value.jvm_args = [...jvmArgs.value];
  draft.value.game_args = [...gameArgs.value];
  const snapshot = clone(draft.value);
  saving.value = true;
  try {
    await enqueueSave(snapshot);
    if (state.value) {
      state.value.instance = snapshot;
      state.value.effective = clone(snapshot);
    }
    saved.value = true;
    window.clearTimeout(savedTimer);
    savedTimer = window.setTimeout(() => (saved.value = false), 1500);
  } catch (e) {
    showToast(errMsg(e));
  } finally {
    saving.value = false;
    if (pending) void saveNow();
  }
}

function customize() {
  if (!state.value) return;
  setDraft(state.value.global);
  scheduleSave();
}

async function reset() {
  if (!state.value) return;
  pending = false;
  window.clearTimeout(saveTimer);
  saving.value = true;
  try {
    await enqueueSave(null);
    state.value.instance = null;
    state.value.effective = clone(state.value.global);
    setDraft(null);
  } catch (e) {
    showToast(errMsg(e));
  } finally {
    saving.value = false;
  }
}

async function refreshJava() {
  refreshing.value = true;
  try {
    await refreshJdks();
  } finally {
    refreshing.value = false;
  }
}

// ── Java ──

const javaOptions = computed<SelOption[]>(() => {
  const options: SelOption[] = [
    { value: AUTO_VALUE, label: t("settings.jdk.autoLabel"), desc: t("settings.jdk.autoDesc") },
    ...jdkList.value.map(jdk => ({
      value: jdk.path,
      label: jdk.version,
      desc: t("settings.jdk.optionDesc", { vendor: jdk.vendor, bits: jdk.bits, path: jdk.path }),
    })),
  ];
  const manual = draft.value?.java;
  if (manual?.mode === "manual" && !jdkList.value.some(jdk => jdk.path === manual.path)) {
    options.push({ value: manual.path, label: manual.path, desc: "" });
  }
  return options;
});

const javaValue = computed(() =>
  draft.value?.java.mode === "manual" ? draft.value.java.path : AUTO_VALUE,
);

function pickJava(value: string) {
  if (!draft.value) return;
  draft.value.java = value === AUTO_VALUE
    ? { mode: "auto" }
    : { mode: "manual", path: value };
  scheduleSave();
}

// ── 内存 ──

const memCeil = computed(() => memCeilMb(memState.availMb));

/** 上界（-Xmx）：自动模式取 0 mod 估算值。 */
const memUpperMb = computed(() => {
  const mem = draft.value?.memory;
  const raw = !mem || mem === "Auto" ? autoAllocMb(memState.availMb) : mem.Custom;
  return Math.max(MEM_FLOOR_MB + MEM_STEP_MB, snapMemMb(raw, memCeil.value));
});

/** 下界（-Xms）。 */
const memLowerMb = computed(() => {
  const raw = snapMemMb(draft.value?.min_memory ?? MEM_FLOOR_MB, memCeil.value);
  return Math.min(memUpperMb.value - MEM_STEP_MB, raw);
});

const memAuto = computed<boolean>({
  get: () => draft.value?.memory === "Auto",
  set: (auto) => {
    if (!draft.value) return;
    // 取消自动：上下界冻结在当前自动值上，下界交还用户拖动
    draft.value.memory = auto ? "Auto" : { Custom: memUpperMb.value };
    if (!auto) draft.value.min_memory = MEM_FLOOR_MB;
  },
});

const memRange = computed<[number, number]>({
  get: (): [number, number] => [memLowerMb.value, memUpperMb.value],
  set: (value: [number, number]) => {
    if (!draft.value) return;
    draft.value.min_memory = value[0];
    draft.value.memory = { Custom: value[1] };
  },
});

// ── 窗口 / 下拉 ──

const priorityOptions = computed<SelOption[]>(() => [
  { value: "Low", label: t("settings.launch.priorityLow") },
  { value: "Normal", label: t("settings.launch.priorityNormal") },
  { value: "High", label: t("settings.launch.priorityHigh") },
  { value: "RealTime", label: t("settings.launch.priorityRealtime") },
]);

const winsizeOptions = computed<SelOption[]>(() => [
  { value: "Default", label: t("settings.launch.windowDefault") },
  { value: "Custom", label: t("settings.launch.windowCustom") },
]);

const gameLangOptions = computed<SelOption[]>(() => {
  const options = MINECRAFT_LANGUAGES.map(l => ({
    value: l.code,
    label: minecraftLanguageLabel(l.code),
  }));
  const current = draft.value?.game_language;
  if (current && !options.some(o => o.value === current)) {
    options.unshift({ value: current, label: current });
  }
  return options;
});

const gcSelectOptions = computed<SelOption[]>(() =>
  gcOptions.map(option => ({ value: option.value, label: t(option.labelKey) })),
);

const winMode = computed<"Default" | "Custom">({
  get: () => (draft.value?.window_size === "Default" ? "Default" : "Custom"),
  set: (mode) => {
    if (!draft.value) return;
    draft.value.window_size = mode === "Default"
      ? "Default"
      : { Custom: [winW.value, winH.value] };
  },
});

const winW = computed<number>({
  get: () => (draft.value && draft.value.window_size !== "Default"
    ? draft.value.window_size.Custom[0]
    : 1280),
  set: (value) => {
    if (draft.value && draft.value.window_size !== "Default") {
      draft.value.window_size = { Custom: [value, winH.value] };
    }
  },
});

const winH = computed<number>({
  get: () => (draft.value && draft.value.window_size !== "Default"
    ? draft.value.window_size.Custom[1]
    : 720),
  set: (value) => {
    if (draft.value && draft.value.window_size !== "Default") {
      draft.value.window_size = { Custom: [winW.value, value] };
    }
  },
});

function setPriority(value: string) {
  if (!draft.value) return;
  draft.value.process_priority = value as ProcessPriority;
  scheduleSave();
}

function setGc(value: string) {
  if (!draft.value) return;
  draft.value.gc_type = value as GcType;
  scheduleSave();
}

function setWindowMode(value: string) {
  winMode.value = value as "Default" | "Custom";
  scheduleSave();
}
</script>

<template>
  <div class="vs-page">
    <div v-if="loading" class="stg-card">
      <LoadingSpinner :text="t('settings.version.setup.launch.loading')" />
    </div>

    <div v-else-if="error" class="stg-card err">{{ error }}</div>

    <template v-else-if="state">
      <div class="stg-card">
        <div class="stg-card-hdr">
          <h3>{{ t("settings.version.setup.launch.title") }}</h3>
          <div class="stg-card-sub">
            {{ draft ? t("settings.version.setup.launch.customized") : t("settings.version.setup.launch.followGlobalDesc") }}
          </div>
        </div>

        <div v-if="!draft" class="inst-follow">
          <span>{{ t("settings.version.setup.launch.followGlobal") }}</span>
          <button class="tool-btn" @click="customize">
            {{ t("settings.version.setup.launch.customize") }}
          </button>
        </div>

        <template v-else>
          <div class="inst-toolbar">
            <span class="inst-status">
              <span v-if="saving">{{ t("settings.version.setup.launch.saving") }}</span>
              <span v-else-if="saved" class="inst-saved">{{ t("settings.version.setup.launch.saved") }}</span>
            </span>
            <button class="tool-btn" @click="reset">
              <Undo2 :size="14" />
              {{ t("settings.version.setup.launch.reset") }}
            </button>
          </div>
          <p class="inst-note">{{ t("settings.version.setup.launch.note") }}</p>
        </template>
      </div>

      <template v-if="draft">
        <div class="stg-card">
          <div class="stg-card-hdr">
            <h3>{{ t("settings.version.setup.launch.javaTitle") }}</h3>
            <div class="stg-card-sub">{{ t("settings.version.setup.launch.javaDesc") }}</div>
          </div>
          <div class="lo-row">
            <div class="inst-java-select">
              <DropSelect
                :model-value="javaValue"
                :options="javaOptions"
                width="100%"
                @update:model-value="pickJava"
              />
            </div>
            <button class="tool-btn" :disabled="refreshing" @click="refreshJava">
              <RefreshCw :size="14" />
              {{ t("settings.jdk.refresh") }}
            </button>
          </div>
        </div>

        <div class="stg-card">
          <div class="stg-card-hdr">
            <h3>{{ t("settings.version.setup.launch.gameTitle") }}</h3>
            <div class="stg-card-sub">{{ t("settings.launch.subtitle") }}</div>
          </div>
          <div class="launch-opts">
            <div class="lo-row">
              <label class="lo-label" for="inst-title">{{ t("settings.launch.windowTitle") }}</label>
              <input
                id="inst-title"
                v-model="draft.window_title"
                class="lo-input"
                type="text"
                :placeholder="t('settings.launch.windowTitlePlaceholder')"
                maxlength="64"
                autocomplete="off"
                @change="scheduleSave"
              />
            </div>

            <div class="lo-row">
              <label class="lo-label" for="inst-prio">{{ t("settings.launch.priority") }}</label>
              <DropSelect
                id="inst-prio"
                :model-value="draft.process_priority"
                :options="priorityOptions"
                width="140px"
                @update:model-value="setPriority"
              />
            </div>

            <div class="lo-row">
              <label class="lo-label" for="inst-winsize">{{ t("settings.launch.windowSize") }}</label>
              <div class="lo-winsize">
                <DropSelect
                  id="inst-winsize"
                  :model-value="winMode"
                  :options="winsizeOptions"
                  width="140px"
                  @update:model-value="setWindowMode"
                />
                <Transition name="fade">
                  <div v-if="winMode === 'Custom'" class="lo-win-custom">
                    <input v-model.number="winW" class="lo-input lo-num" type="number" min="320" max="7680" autocomplete="off" @change="scheduleSave" />
                    <span class="lo-times">×</span>
                    <input v-model.number="winH" class="lo-input lo-num" type="number" min="240" max="4320" autocomplete="off" @change="scheduleSave" />
                  </div>
                </Transition>
              </div>
            </div>

            <div class="lo-row">
              <label class="lo-label" for="inst-lang">{{ t("settings.launch.gameLanguage") }}</label>
              <DropSelect
                id="inst-lang"
                :model-value="draft.game_language"
                :options="gameLangOptions"
                width="200px"
                @update:model-value="draft.game_language = $event; scheduleSave()"
              />
            </div>

            <div class="lo-row">
              <CheckToggle v-model="draft.verify_ssl" @change="scheduleSave">
                {{ t("settings.launch.verifySsl") }}
              </CheckToggle>
            </div>
            <div class="lo-row">
              <CheckToggle v-model="draft.pre_launch_memory_optimize" @change="scheduleSave">
                {{ t("settings.launch.memoryOptimize") }}
              </CheckToggle>
            </div>
          </div>
        </div>

        <div class="stg-card">
          <div class="stg-card-hdr">
            <h3>{{ t("settings.launch.memoryTitle") }}</h3>
            <div class="stg-card-sub">{{ t("settings.launch.memorySubtitle") }}</div>
          </div>
          <div class="launch-opts">
            <div class="lo-row">
              <div class="mem-auto">
                <CheckToggle v-model="memAuto" @change="scheduleSave">
                  {{ t("settings.launch.memoryAuto") }}
                </CheckToggle>
              </div>
              <span class="mem-bound" :title="t('settings.launch.memoryMin')">{{ fmtGb(memLowerMb) }}</span>
              <RangeSlider
                :model-value="memRange"
                :min="MEM_FLOOR_MB"
                :max="memCeil"
                :step="MEM_STEP_MB"
                :disabled="memAuto"
                :low-label="t('settings.launch.memoryMin')"
                :high-label="t('settings.launch.memoryMax')"
                @update:model-value="memRange = $event"
                @change="scheduleSave"
              />
              <span class="mem-bound" :title="t('settings.launch.memoryMax')">{{ fmtGb(memUpperMb) }}</span>
            </div>
            <MemoryUsageBar />
          </div>
        </div>

        <div class="stg-card">
          <div class="stg-card-hdr">
            <h3>{{ t("settings.version.setup.launch.advancedTitle") }}</h3>
            <div class="stg-card-sub">{{ t("settings.version.setup.launch.advancedDesc") }}</div>
          </div>
          <div class="launch-opts">
            <div class="lo-row lo-top">
              <label class="lo-label" for="inst-jvm">{{ t("settings.jdk.jvmArgs") }}</label>
              <ArgsListInput
                input-id="inst-jvm"
                :model-value="jvmArgs"
                :placeholder="t('settings.jdk.jvmArgsPlaceholder')"
                @update:model-value="updateJvmArgs"
              />
            </div>
            <div class="lo-row lo-top">
              <label class="lo-label" for="inst-game">{{ t("settings.jdk.gameArgs") }}</label>
              <ArgsListInput
                input-id="inst-game"
                :model-value="gameArgs"
                :placeholder="t('settings.jdk.gameArgsPlaceholder')"
                @update:model-value="updateGameArgs"
              />
            </div>
            <div class="lo-row">
              <label class="lo-label" for="inst-gc">{{ t("settings.jdk.memoryManagement") }}</label>
              <DropSelect
                id="inst-gc"
                :model-value="draft.gc_type"
                :options="gcSelectOptions"
                width="180px"
                @update:model-value="setGc"
              />
            </div>
            <div class="lo-checks">
              <CheckToggle v-model="draft.disable_launch_wrapper" @change="scheduleSave">
                {{ t("settings.jdk.disableWrapper") }}
              </CheckToggle>
              <CheckToggle v-model="draft.disable_lwjgl_unsafe_agent" @change="scheduleSave">
                {{ t("settings.jdk.disableAgent") }}
              </CheckToggle>
              <CheckToggle v-model="draft.use_high_performance_gpu" @change="scheduleSave">
                {{ t("settings.jdk.highGpu") }}
              </CheckToggle>
            </div>
          </div>
        </div>
      </template>
    </template>
  </div>
</template>

<style>
@import "../../styles/settings-shared.css";
</style>

<style scoped>

.inst-follow,
.inst-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.inst-follow {
  padding: 4px 0 2px;
  color: var(--text-secondary);
  font-size: var(--fs-base);
}

.inst-toolbar {
  min-height: 34px;
}

.inst-status {
  min-width: 0;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}

.inst-saved {
  color: var(--color-success);
}

.inst-note {
  margin: 8px 0 0;
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-tertiary);
}

.mem-auto {
  flex: none;
  width: 96px;
}

.mem-bound {
  flex: none;
  width: 56px;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  text-align: center;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.lo-win-custom {
  display: flex;
  align-items: center;
  gap: 6px;
}

.inst-java-select {
  flex: 1;
  min-width: 0;
}
</style>
