<script setup lang="ts">
// 启动选项 + 内存分配卡片
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { startMemoryMonitor } from "../../store/app/memory";
import DropSelect, { type SelOption } from "../common/DropSelect.vue";
import AutoTasksEditor from "./AutoTasksEditor.vue";
import CheckToggle from "../common/CheckToggle.vue";
import RangeSlider from "../common/RangeSlider.vue";
import { MEM_FLOOR_MB, MEM_STEP_MB } from "../../utils/memory";
import {
  closeAction, setCloseAction, trayAvailable, type CloseAction,
} from "../../store/app/closeBehavior";
import {
  loWindowTitle, loPriority, loWinMode, loWinW, loWinH, loGameLang,
  loMemAuto, loMemMb, loMinMem, loMemOptimize, loVerifySsl, launchError,
  memLimitMb, memAutoMb, memLowerMb, memUpperMb,
  saveLaunchOptions, loadLaunchOptions,
} from "../../store/config/launchOptions";
import { fmtGb } from "../../utils/format";
import MemoryUsageBar from "../launch/MemoryUsageBar.vue";
import { MINECRAFT_LANGUAGES, minecraftLanguageLabel } from "../../utils/minecraftLanguages";

const { t } = useI18n();

onMounted(() => {
  // 内存状态只在本页展示：进页再启动轮询，避免启动器常驻 2s IPC。
  startMemoryMonitor();
  void loadLaunchOptions();
});

const memRange = computed<[number, number]>(() => [memLowerMb.value, memUpperMb.value]);

/** 双端滑块：下界 -Xms / 上界 -Xmx。 */
function setMemRange(value: [number, number]) {
  loMinMem.value = value[0];
  loMemMb.value = value[1];
}

/** 取消自动配置：手动上下界冻结在当前自动值上，随后可自由拖动。 */
function onMemAutoChange() {
  if (!loMemAuto.value) {
    loMemMb.value = memAutoMb.value;
    loMinMem.value = MEM_FLOOR_MB;
  }
  saveLaunchOptions();
}

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
  // 配置里保留了列表外的合法代码时，补一条以免下拉显示为空
  if (loGameLang.value && !options.some(o => o.value === loGameLang.value)) {
    options.unshift({ value: loGameLang.value, label: loGameLang.value });
  }
  return options;
});

/** 关闭窗口行为：托盘不可用时只保留「询问 / 直接退出」。 */
const closeOptions = computed<SelOption[]>(() => [
  { value: "ask", label: t("settings.launch.closeAsk") },
  ...(trayAvailable.value
    ? [{ value: "tray", label: t("settings.launch.closeTray") }]
    : []),
  { value: "exit", label: t("settings.launch.closeExit") },
]);
</script>

<template>
  <div class="stg-card">
    <div class="stg-card-hdr">
      <h3>{{ t("settings.launch.title") }}</h3>
      <div class="stg-card-sub">{{ t("settings.launch.subtitle") }}</div>
    </div>
    <div class="launch-opts">
      <div class="lo-row">
        <label class="lo-label" for="lo-title">{{ t("settings.launch.windowTitle") }}</label>
        <input
          id="lo-title"
          v-model="loWindowTitle"
          class="lo-input"
          type="text"
          :placeholder="t('settings.launch.windowTitlePlaceholder')"
          maxlength="64"
          @change="saveLaunchOptions"
          autocomplete="off"
        />
      </div>

      <div class="lo-row">
        <label class="lo-label" for="lo-prio">{{ t("settings.launch.priority") }}</label>
        <DropSelect
          id="lo-prio"
          :model-value="loPriority"
          :options="priorityOptions"
          width="140px"
          @update:model-value="loPriority = $event as typeof loPriority; saveLaunchOptions()"
        />
      </div>

      <div class="lo-row">
        <label class="lo-label" for="lo-winsize">{{ t("settings.launch.windowSize") }}</label>
        <div class="lo-winsize">
          <DropSelect
            id="lo-winsize"
            :model-value="loWinMode"
            :options="winsizeOptions"
            width="140px"
            @update:model-value="loWinMode = $event as typeof loWinMode; saveLaunchOptions()"
          />
          <Transition name="fade">
            <div v-if="loWinMode === 'Custom'" class="lo-win-custom">
              <input
                v-model.number="loWinW"
                class="lo-input lo-num"
                type="number"
                min="320"
                max="7680"
                :placeholder="t('settings.launch.width')"
                @change="saveLaunchOptions"
                autocomplete="off"
              />
              <span class="lo-times">×</span>
              <input
                v-model.number="loWinH"
                class="lo-input lo-num"
                type="number"
                min="240"
                max="4320"
                :placeholder="t('settings.launch.height')"
                @change="saveLaunchOptions"
                autocomplete="off"
              />
            </div>
          </Transition>
        </div>
      </div>

      <div class="lo-row">
        <label class="lo-label" for="lo-lang">{{ t("settings.launch.gameLanguage") }}</label>
        <DropSelect
          id="lo-lang"
          :model-value="loGameLang"
          :options="gameLangOptions"
          width="200px"
          @update:model-value="loGameLang = $event; saveLaunchOptions()"
        />
      </div>

      <div class="lo-row">
        <CheckToggle v-model="loVerifySsl" @change="saveLaunchOptions">{{ t("settings.launch.verifySsl") }}</CheckToggle>
      </div>
      <div class="lo-row">
        <CheckToggle v-model="loMemOptimize" @change="saveLaunchOptions">{{ t("settings.launch.memoryOptimize") }}</CheckToggle>
      </div>
      <div class="lo-row">
        <label class="lo-label" for="lo-close">{{ t("settings.launch.closeBehavior") }}</label>
        <DropSelect
          id="lo-close"
          :model-value="closeAction"
          :options="closeOptions"
          width="180px"
          @update:model-value="setCloseAction($event as CloseAction)"
        />
      </div>
    </div>
    <div v-if="launchError" class="err">{{ launchError }}</div>
  </div>

  <div class="stg-card">
    <div class="stg-card-hdr">
      <h3>{{ t("settings.launch.memoryTitle") }}</h3>
      <div class="stg-card-sub">{{ t("settings.launch.memorySubtitle") }}</div>
    </div>
    <div class="launch-opts">
      <div class="lo-row">
        <div class="mem-auto">
          <CheckToggle v-model="loMemAuto" @change="onMemAutoChange">{{ t("settings.launch.memoryAuto") }}</CheckToggle>
        </div>
        <span class="mem-bound" :title="t('settings.launch.memoryMin')">{{ fmtGb(memLowerMb) }}</span>
        <RangeSlider
          :model-value="memRange"
          :min="MEM_FLOOR_MB"
          :max="memLimitMb"
          :step="MEM_STEP_MB"
          :disabled="loMemAuto"
          :low-label="t('settings.launch.memoryMin')"
          :high-label="t('settings.launch.memoryMax')"
          @update:model-value="setMemRange"
          @change="saveLaunchOptions"
        />
        <span class="mem-bound" :title="t('settings.launch.memoryMax')">{{ fmtGb(memUpperMb) }}</span>
      </div>
      <MemoryUsageBar />
    </div>
  </div>

  <AutoTasksEditor mode="global" />
</template>

<style>
@import "../../styles/settings-shared.css";
</style>

<style scoped>
/* ===== 内存分配 ===== */
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
</style>
