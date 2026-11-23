<script setup lang="ts">
// 通用空态容器：占满父容器完全居中，默认插槽自定义表现（推荐：图标 + 文本，颜色由插槽或 --text-tertiary 决定）。
</script>

<template>
  <div class="es-box">
    <slot />
  </div>
</template>

<style scoped>
.es-box {
  flex: 1;
  height: 100%;
  min-height: 120px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  padding: 20px;
  box-sizing: border-box;
  color: var(--text-tertiary);
  text-align: center;
  user-select: none;
}

.es-box :deep(p) {
  font-size: var(--fs-base);
  line-height: 1.7;
  margin: 0;
  /* 文案含 \n（标题行 + 说明行），默认 HTML 折叠换行会连成一句 */
  white-space: pre-line;
}
</style>
