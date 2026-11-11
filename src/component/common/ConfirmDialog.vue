<script setup lang="ts">
// 通用删除确认弹窗：标题 + 正文由调用方本地化，按钮沿用全局 dialog-btn。
import { useI18n } from "vue-i18n";
import AlertDialog from "./AlertDialog.vue";

defineProps<{
  modelValue: boolean;
  title: string;
  message: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  confirm: [];
}>();

const { t } = useI18n();
</script>

<template>
  <AlertDialog
    :model-value="modelValue"
    :title="title"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <div class="cd-body">{{ message }}</div>
    <template #actions>
      <button class="dialog-btn danger" @click="emit('confirm')">{{ t("common.action.delete") }}</button>
      <button class="dialog-btn" @click="emit('update:modelValue', false)">{{ t("common.action.cancel") }}</button>
    </template>
  </AlertDialog>
</template>

<style scoped>
.cd-body {
  padding: 4px 2px;
  font-size: var(--fs-base);
  line-height: 1.6;
  color: var(--text-primary);
  word-break: break-all;
}
</style>
