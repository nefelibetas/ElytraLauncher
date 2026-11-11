<script setup lang="ts">
// 左侧垂直导航 + 内容区骨架，Settings / Tools / VersionSetup 三页共用：
// 导航项 v-for + active 高亮 + 可选刷新按钮 + 内容区过渡。tab 根用 div（可内嵌刷新 button）。
import { computed, ref, watch } from "vue";
import { RefreshCw } from "@lucide/vue";
import { useI18n } from "vue-i18n";
import type { Component } from "vue";

export interface NavTab {
  key: string;
  label: string;
  icon: Component;
}

const props = withDefaults(
  defineProps<{
    tabs: NavTab[];
    modelValue: string;
    /** 左侧分组标题（如 "设置" / "工具"） */
    title?: string;
    /** 导航宽度（Settings/Tools=180，VersionSetup=240） */
    width?: number;
    /** 显示 hover 刷新按钮（VersionSetup） */
    refreshable?: boolean;
    /** 内容区内部滚动：子组件根 flex:1 + 自身滚动（VersionSetup 卡片布局） */
    internalScroll?: boolean;
    /** 不显示刷新按钮的 tab key（如 VersionSetup 的 export） */
    noRefreshKeys?: string[];
  }>(),
  { width: 180, refreshable: false, internalScroll: false, noRefreshKeys: () => [] },
);

const { t } = useI18n();

const emit = defineEmits<{
  "update:modelValue": [k: string];
  refresh: [k: string];
}>();

function onRefresh(e: MouseEvent, k: string) {
  e.stopPropagation();
  emit("refresh", k);
}

/** 内容切换方向按 tab 顺序：向下切 → 内容从上往下移动；向上切 → 从下往上移动。 */
const slideDir = ref<"down" | "up">("down");
let lastIndex = Math.max(0, props.tabs.findIndex((tab) => tab.key === props.modelValue));
watch(
  () => props.modelValue,
  (next) => {
    const nextIndex = props.tabs.findIndex((tab) => tab.key === next);
    if (nextIndex < 0 || nextIndex === lastIndex) return;
    slideDir.value = nextIndex > lastIndex ? "down" : "up";
    lastIndex = nextIndex;
  },
);
const switchName = computed(() => `sn-slide-${slideDir.value}`);
</script>

<template>
  <div class="sn">
    <nav class="sn-nav" :style="{ flex: `0 0 ${width}px`, minWidth: `${width - 20}px` }">
      <div v-if="title" class="sn-divider"><span>{{ title }}</span></div>
      <div
        v-for="tab in tabs"
        :key="tab.key"
        class="sn-tab"
        :class="{ active: modelValue === tab.key }"
        @click="emit('update:modelValue', tab.key)"
      >
        <component :is="tab.icon" :size="16" />
        <span>{{ tab.label }}</span>
        <button
          v-if="refreshable && !noRefreshKeys.includes(tab.key)"
          class="sn-refresh"
          v-tooltip="t('common.tooltip.refresh')"
          @click="onRefresh($event, tab.key)"
        >
          <RefreshCw :size="12" />
        </button>
      </div>
    </nav>

    <div class="sn-body" :class="{ 'sn-body-scroll': !internalScroll }">
      <Transition :name="switchName">
        <slot />
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.sn {
  display: flex;
  height: 100%;
}

/* ===== 左侧导航 ===== */
.sn-nav {
  background-color: var(--surface-elevated);
  border-right: 1px solid var(--border-default);
  padding: 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
  overflow-x: hidden;
}

.sn-divider {
  display: flex;
  align-items: center;
  padding: 8px 12px 6px;
  user-select: none;
}

.sn-divider span {
  font-size: 11px;
  font-weight: 500;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.8px;
}

.sn-tab {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
  user-select: none;
  text-align: left;
}

.sn-tab:hover {
  color: var(--text-primary);
  background-color: var(--surface-hover);
}

.sn-tab.active {
  color: var(--color-brand);
  background-color: var(--color-brand-bg);
}

/* hover 显示的刷新按钮（VersionSetup） */
.sn-refresh {
  flex: none;
  margin-left: auto;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s, color 0.15s, background 0.15s;
}

.sn-tab:hover .sn-refresh,
.sn-refresh:focus-visible {
  opacity: 1;
}

.sn-refresh:hover {
  color: var(--color-brand);
  background: var(--color-brand-bg);
}

/* ===== 右侧内容 ===== */
/* 滚动宿主保持全宽（滚动条贴窗口右缘）；内容宽度上界由子元素承担 */
.sn-body {
  flex: 1;
  position: relative;
  min-width: 0;
  padding: 20px 24px;
  background: var(--surface-page);
}

.sn-body > :slotted(*) {
  max-width: var(--content-max);
}

/* 默认：body 自身滚动（Settings/Tools 长表单页） */
.sn-body-scroll {
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
}

/* internalScroll：子组件根 flex:1 + 内部滚动（VersionSetup 卡片布局） */
.sn-body:not(.sn-body-scroll) {
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* tab 内容切换：按导航顺序做上下位移，旧内容绝对定位退出，避免 out-in 空窗。 */
.sn-slide-down-enter-active,
.sn-slide-down-leave-active,
.sn-slide-up-enter-active,
.sn-slide-up-leave-active {
  transition: transform 0.22s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.18s ease;
}

/* 向下切：新内容从上方落下，旧内容向下方退出 */
.sn-slide-down-enter-from {
  opacity: 0;
  transform: translateY(-24px);
}
.sn-slide-down-leave-to {
  opacity: 0;
  transform: translateY(24px);
}
/* 向上切：反向 */
.sn-slide-up-enter-from {
  opacity: 0;
  transform: translateY(24px);
}
.sn-slide-up-leave-to {
  opacity: 0;
  transform: translateY(-24px);
}

.sn-slide-down-leave-active,
.sn-slide-up-leave-active {
  position: absolute;
  inset: 0;
  padding: 20px 24px;
  overflow: hidden;
  pointer-events: none;
  z-index: 0;
}

.sn-slide-down-enter-active,
.sn-slide-up-enter-active {
  position: relative;
  z-index: 1;
}

@media (prefers-reduced-motion: reduce) {
  .sn-slide-down-enter-active,
  .sn-slide-down-leave-active,
  .sn-slide-up-enter-active,
  .sn-slide-up-leave-active {
    transition: none;
  }
}
</style>
