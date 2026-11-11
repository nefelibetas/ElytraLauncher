<script setup lang="ts">
// 双端滑块：两枚原生 range 叠放，仅手柄接收指针（轨道点击不响应，避免上下界归属歧义）。
// 两端至少留一格间距，手柄不会重合；下界不得越过上界。
import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    modelValue: [number, number];
    min: number;
    max: number;
    step?: number;
    disabled?: boolean;
    lowLabel?: string;
    highLabel?: string;
  }>(),
  { step: 128, disabled: false, lowLabel: "", highLabel: "" },
);

const emit = defineEmits<{
  "update:modelValue": [v: [number, number]];
  change: [v: [number, number]];
}>();

const maxValue = computed(() => Math.max(props.max, props.min + props.step));

function snap(v: number): number {
  const n = Math.round((v - props.min) / props.step);
  return Math.min(maxValue.value, Math.max(props.min, props.min + n * props.step));
}

// 两端至少留一格间距；像素上仍可能贴近，靠 z-index 保证两侧都能拖开
const high = computed(() =>
  Math.max(snap(props.modelValue[1]), Math.min(maxValue.value, snap(props.modelValue[0]) + props.step)));
const low = computed(() => Math.min(snap(props.modelValue[0]), high.value - props.step));

function pct(v: number): string {
  return ((v - props.min) / (maxValue.value - props.min)) * 100 + "%";
}

// 手柄像素上可能接触：靠近右半区时把下界抬到上层，保证两侧都能拖开
const lowZ = computed(() => (low.value > (props.min + maxValue.value) / 2 ? 4 : 2));

function onLow(e: Event) {
  const el = e.target as HTMLInputElement;
  const value = Math.min(Number(el.value), high.value - props.step);
  // 原生 input 在拖出边界后会保留指针位置；立即回写，避免 DOM 与模型脱节
  if (Number(el.value) !== value) el.value = String(value);
  emit("update:modelValue", [value, high.value]);
}

function onHigh(e: Event) {
  const el = e.target as HTMLInputElement;
  const value = Math.max(Number(el.value), low.value + props.step);
  if (Number(el.value) !== value) el.value = String(value);
  emit("update:modelValue", [low.value, value]);
}

function emitChange() {
  emit("change", [low.value, high.value]);
}
</script>

<template>
  <div class="rs" :class="{ disabled }">
    <div class="rs-track"></div>
    <div class="rs-fill" :style="{ left: pct(low), right: `calc(100% - ${pct(high)})` }"></div>
    <input
      class="rs-input rs-low"
      type="range"
      :min="min"
      :max="maxValue"
      :step="step"
      :value="low"
      :disabled="disabled"
      :aria-label="lowLabel"
      :style="{ zIndex: lowZ }"
      autocomplete="off"
      @input="onLow"
      @change="emitChange"
    />
    <input
      class="rs-input rs-high"
      type="range"
      :min="min"
      :max="maxValue"
      :step="step"
      :value="high"
      :disabled="disabled"
      :aria-label="highLabel"
      autocomplete="off"
      @input="onHigh"
      @change="emitChange"
    />
  </div>
</template>

<style scoped>
.rs {
  position: relative;
  flex: 1;
  min-width: 0;
  height: 24px;
}

.rs-track,
.rs-fill {
  position: absolute;
  top: 50%;
  height: 6px;
  margin-top: -3px;
  border-radius: var(--radius-2xs);
  pointer-events: none;
}

.rs-track {
  left: 0;
  right: 0;
  background: var(--border-default);
}

.rs-fill {
  background: var(--color-brand);
}

.rs.disabled .rs-fill {
  background: var(--text-tertiary);
}

/* 输入铺满轨道框（用于放置手柄），轨道交给 .rs-track 绘制 */
.rs-input {
  position: absolute;
  left: 0;
  top: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  padding: 0;
  background: transparent;
  -webkit-appearance: none;
  appearance: none;
  pointer-events: none;
}

/* 基础层级：下界在右半区由内联 z-index 抬到上界之上 */
.rs-low {
  z-index: 2;
}

.rs-high {
  z-index: 3;
}

.rs-input::-webkit-slider-runnable-track {
  height: 6px;
  border-radius: var(--radius-2xs);
  background: transparent;
}

.rs-input::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 14px;
  height: 14px;
  margin-top: -4px;
  border-radius: 50%;
  background: var(--color-brand);
  border: 2px solid var(--text-inverse);
  box-shadow: var(--shadow-sm);
  pointer-events: auto;
  transition: transform 0.12s;
}

.rs-input::-moz-range-track {
  height: 6px;
  border-radius: var(--radius-2xs);
  background: transparent;
}

.rs-input::-moz-range-thumb {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--color-brand);
  border: 2px solid var(--text-inverse);
  box-shadow: var(--shadow-sm);
  pointer-events: auto;
}

.rs:not(.disabled) .rs-input:hover::-webkit-slider-thumb {
  transform: scale(1.12);
}

.rs.disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.rs.disabled .rs-input {
  cursor: not-allowed;
}

.rs.disabled .rs-input::-webkit-slider-thumb {
  background: var(--text-tertiary);
  pointer-events: none;
}

.rs.disabled .rs-input::-moz-range-thumb {
  background: var(--text-tertiary);
  pointer-events: none;
}
</style>
