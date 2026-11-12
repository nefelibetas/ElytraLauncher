<script setup lang="ts">
// 单行参数输入 + 已添加列表：Enter/按钮添加，逐条删除；超过 4 条时列表内部滚动。
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { Plus, X } from "@lucide/vue";

const props = defineProps<{
  modelValue: string[];
  inputId: string;
  placeholder?: string;
}>();

const emit = defineEmits<{ (e: "update:modelValue", v: string[]): void }>();

const { t } = useI18n();
const draft = ref("");
const canAdd = computed(() => !!draft.value.trim());

function add() {
  const value = draft.value.trim();
  if (!value) return;
  if (!props.modelValue.includes(value)) {
    emit("update:modelValue", [...props.modelValue, value]);
  }
  draft.value = "";
}

function onEnter(e: KeyboardEvent) {
  if (e.isComposing) return;
  e.preventDefault();
  add();
}

function remove(index: number) {
  const next = props.modelValue.slice();
  next.splice(index, 1);
  emit("update:modelValue", next);
}
</script>

<template>
  <div class="args-editor">
    <div class="args-line">
      <input
        :id="inputId"
        v-model="draft"
        class="lo-input args-input"
        type="text"
        :placeholder="placeholder"
        spellcheck="false"
        autocomplete="off"
        @keydown.enter="onEnter"
      />
      <button
        class="args-add"
        type="button"
        :disabled="!canAdd"
        :aria-label="t('common.action.add')"
        v-tooltip="t('common.action.add')"
        @click="add"
      >
        <Plus :size="14" />
      </button>
    </div>

    <ul v-if="modelValue.length" class="args-list">
      <li v-for="(arg, index) in modelValue" :key="`${index}:${arg}`" class="args-item">
        <span class="args-text" v-tooltip="arg">{{ arg }}</span>
        <button
          class="args-del"
          type="button"
          :aria-label="t('common.action.delete')"
          v-tooltip="t('common.action.delete')"
          @click="remove(index)"
        >
          <X :size="12" />
        </button>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.args-editor {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.args-line {
  display: flex;
  align-items: center;
  gap: 6px;
}

.args-input {
  min-width: 0;
}

.args-add {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-card);
  color: var(--text-secondary);
  cursor: pointer;
  transition: border-color var(--dur-fast), color var(--dur-fast);
}

.args-add:hover:not(:disabled) {
  border-color: var(--color-brand);
  color: var(--color-brand);
}

.args-add:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* 视口固定 4 行（行高 28 + 间距 4），更多内容在列表内滚动 */
.args-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 124px;
  overflow-y: auto;
  list-style: none;
}

/* WebView2/WebKitGTK 默认可能用悬浮滚动条，列表超出 4 条时显式给出可拖动的条 */
.args-list::-webkit-scrollbar {
  width: 8px;
}

.args-list::-webkit-scrollbar-thumb {
  border-radius: 4px;
  background: var(--border-strong);
}

.args-list::-webkit-scrollbar-track {
  background: transparent;
}

.args-item {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 4px 0 8px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-page);
}

.args-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

.args-del {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: color var(--dur-fast), background-color var(--dur-fast);
}

.args-del:hover {
  color: var(--color-danger);
  background: var(--surface-hover);
}
</style>
