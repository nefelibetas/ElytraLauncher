<script setup lang="ts">
// 版本筛选下拉：触发按钮 + 自动定高的浮层；展开状态由父级统一管理（同屏只开一个）。
import { ref } from "vue";

export interface FilterOption {
  value: string;
  label: string;
}

const props = defineProps<{
  open: boolean;
  label: string;
  allLabel: string;
  options: FilterOption[];
  selected: string;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  select: [value: string];
}>();

/** 浮层最大高度由按钮下方可视空间决定，避免撑大滚动容器。 */
const POP_MAX_H = 260;
const maxHeight = ref(POP_MAX_H);
const root = ref<HTMLElement | null>(null);

function toggle() {
  if (!props.open) {
    const rect = root.value?.getBoundingClientRect();
    if (rect) {
      maxHeight.value = Math.max(80, Math.min(POP_MAX_H, window.innerHeight - rect.bottom - 8));
    }
  }
  emit("update:open", !props.open);
}
</script>

<template>
  <div ref="root" class="mv-drop" @pointerdown.stop @click.stop="toggle">
    <span class="mv-drop-label">{{ label }}</span>
    <span class="mv-drop-arrow" :class="{ open }"></span>
    <Transition name="expand">
      <div v-if="open" class="mv-drop-pop" :style="{ '--expand-h': maxHeight + 'px' }">
        <div class="mv-drop-opt" :class="{ sel: !selected }" @click.stop="emit('select', '')">{{ allLabel }}</div>
        <div
          v-for="o in options"
          :key="o.value"
          class="mv-drop-opt"
          :class="{ sel: o.value === selected }"
          @click.stop="emit('select', o.value)"
        >{{ o.label }}</div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.mv-drop {
  position: relative;
  flex: 0 0 140px;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-card);
  cursor: pointer;
  user-select: none;
}

.mv-drop:hover {
  border-color: var(--color-brand);
}

.mv-drop-label {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-base);
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mv-drop-arrow {
  flex-shrink: 0;
  width: 0;
  height: 0;
  border-left: 4px solid transparent;
  border-right: 4px solid transparent;
  border-top: 5px solid var(--text-tertiary);
  transition: transform var(--dur-fast);
}

.mv-drop-arrow.open {
  transform: rotate(180deg);
}

.mv-drop-pop {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  z-index: 20;
  max-height: var(--expand-h, 260px);
  overflow-y: auto;
  background: var(--surface-card);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-md);
  padding: 4px;
}

.mv-drop-opt {
  padding: 6px 10px;
  border-radius: var(--radius-xs);
  font-size: var(--fs-base);
  color: var(--text-secondary);
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition: background 0.1s, color 0.1s;
}

.mv-drop-opt:hover {
  background: var(--surface-hover);
  color: var(--color-brand);
}

.mv-drop-opt.sel {
  color: var(--color-brand);
  font-weight: 600;
}
</style>
