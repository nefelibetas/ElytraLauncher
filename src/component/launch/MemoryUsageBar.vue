<script setup lang="ts">
// 内存占用条：已用 / 游戏分配 / 剩余三段 + 总量。
// 全局启动设置与实例启动设置共用；数据直接取 memory / launchOptions store。
import { useI18n } from "vue-i18n";
import { memState } from "../../store/app/memory";
import {
  memGameMb,
  memGamePct,
  memUsedMb,
  memUsedPct,
} from "../../store/config/launchOptions";
import { fmtGb, memLabelLeft } from "../../utils/format";

const { t } = useI18n();
</script>

<template>
  <div class="mem-status">
    <div class="mem-bar-labels">
      <span class="mem-bar-label">{{ t("settings.launch.memoryUsed", { size: fmtGb(memUsedMb) }) }}</span>
      <span
        v-if="memGamePct > 0"
        class="mem-bar-label"
        :style="{ left: memLabelLeft(memUsedPct) }"
      >{{ t("settings.launch.memoryGame", { size: fmtGb(memGameMb) }) }}</span>
    </div>
    <div class="mem-bar">
      <div class="mem-seg mem-used" :style="{ width: memUsedPct + '%' }"></div>
      <div class="mem-seg mem-game" :style="{ width: memGamePct + '%' }"></div>
      <div class="mem-seg mem-free"></div>
    </div>
    <div class="mem-total">{{ fmtGb(memUsedMb) }} / {{ fmtGb(memState.totalMb) }}</div>
  </div>
</template>

<style scoped>
.mem-status {
  margin-top: 2px;
}

.mem-bar-labels {
  position: relative;
  height: 16px;
}

.mem-bar-label {
  position: absolute;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.mem-bar {
  display: flex;
  height: 10px;
  border-radius: var(--radius-xs);
  overflow: hidden;
  background: var(--border-default);
}

.mem-seg {
  height: 100%;
  transition: width 0.4s ease;
}

.mem-used {
  background: var(--color-danger);
}

.mem-game {
  background: var(--color-success);
}

.mem-free {
  flex: 1;
}

.mem-total {
  margin-top: 6px;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}
</style>
