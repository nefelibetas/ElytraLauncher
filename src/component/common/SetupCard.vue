<script setup lang="ts">
// 侧栏设置页卡片骨架：标题 / 副标题 / 右侧操作槽 + 内容。
// padded=false 供「标题与内容共用内边距」的旧版式（导出 / 外观等卡片）使用。
withDefaults(defineProps<{
  title?: string;
  desc?: string;
  padded?: boolean;
}>(), {
  title: "",
  desc: "",
  padded: true,
});
</script>

<template>
  <div class="setup-card" :class="{ 'setup-card--flush': !padded }">
    <div v-if="title || $slots.actions" class="setup-card-hdr">
      <div class="setup-card-titles">
        <div v-if="title" class="setup-card-title">{{ title }}</div>
        <div v-if="desc" class="setup-desc">{{ desc }}</div>
      </div>
      <slot name="actions" />
    </div>
    <slot />
  </div>
</template>

<style>
@import "../../styles/version-setup-shared.css";
</style>
