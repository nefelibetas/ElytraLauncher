<script setup lang="ts">
// 多选操作栏：全选 / 取消选择 / 计数 + 业务按钮槽（删除、更新等）。
import { useI18n } from "vue-i18n";

withDefaults(defineProps<{
  allSelected: boolean;
  count: number;
  variant?: "attached" | "boxed";
}>(), {
  variant: "attached",
});

const emit = defineEmits<{
  "toggle-all": [];
  clear: [];
}>();

const { t } = useI18n();
</script>

<template>
  <div
    class="vs-actions-bar"
    :class="variant === 'boxed' ? 'vs-bar-boxed' : 'vs-bar-attached'"
  >
    <button class="vs-ab-btn" @click="emit('toggle-all')">
      {{ allSelected ? t("settings.version.setup.common.deselectAll") : t("settings.version.setup.common.selectAll") }}
    </button>
    <button class="vs-ab-btn" @click="emit('clear')">{{ t("settings.version.setup.common.deselect") }}</button>
    <span class="vs-ab-count">{{ t("settings.version.setup.common.selectedCount", { count }) }}</span>
    <slot />
  </div>
</template>

<style>
@import "../../styles/version-setup-shared.css";
</style>
