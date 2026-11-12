<script setup lang="ts">
// 本地资源文件行内容：已识别（社区元数据）与未识别（本地文件）双态。
// 外层行容器由各页面负责（选中态、悬停、点击），本组件只渲染图标与文本。
import type { Component } from "vue";
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { fmtCount, fmtSize } from "../../utils/format";
import { communityTranslationEnabled } from "../../store/community/communitySearch";
import IconImage from "./IconImage.vue";

/** 与 Rust `ModFileDetail` 结构兼容；页面可直接传 store 类型。 */
export interface CommunityFileItem {
  file_name: string;
  size: number;
  project_id: string | null;
  title: string | null;
  title_zh: string | null;
  description: string | null;
  description_zh: string | null;
  icon_url: string | null;
  downloads: number | null;
  current_version: string | null;
}

const props = defineProps<{
  item: CommunityFileItem;
  /** 未识别时使用的本地类型图标。 */
  localIcon: Component;
}>();

const { t } = useI18n();

const description = computed(() =>
  communityTranslationEnabled.value
    ? (props.item.description_zh ?? props.item.description)
    : props.item.description,
);
</script>

<template>
  <template v-if="item.project_id">
    <IconImage :src="item.icon_url" :size="32" />
    <span class="cfm-info">
      <span class="cfm-title-row">
        <span class="cfm-title">{{ communityTranslationEnabled && item.title_zh ? item.title_zh : (item.title ?? item.file_name) }}</span>
        <span v-if="communityTranslationEnabled && item.title_zh" class="cfm-file">{{ item.file_name }}</span>
        <span v-if="item.downloads" class="cfm-sub">
          {{ t("common.status.downloads", { count: fmtCount(item.downloads) }, item.downloads) }}
        </span>
      </span>
      <span v-if="item.description" class="cfm-desc">{{ description }}</span>
      <span class="cfm-support">
        <span v-if="item.current_version" class="cfm-ver">v{{ item.current_version }}</span>
        <slot name="meta" />
      </span>
    </span>
  </template>
  <template v-else>
    <component :is="localIcon" :size="20" class="cfm-local-icon" />
    <span class="cfm-info">
      <span class="cfm-local-name">{{ item.file_name }}</span>
      <span class="cfm-local-meta">
        {{ fmtSize(item.size) }}
        <slot name="local-meta" />
      </span>
    </span>
  </template>
</template>

<style scoped>
.cfm-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.cfm-title-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.cfm-title {
  font-weight: 600;
  font-size: var(--fs-base);
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cfm-file {
  flex-shrink: 1;
  font-size: var(--fs-xs);
  font-weight: 400;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cfm-sub {
  flex-shrink: 0;
  margin-left: auto;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.cfm-desc {
  max-width: 100%;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  line-height: 1.4;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cfm-support {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.cfm-ver {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  white-space: nowrap;
}

.cfm-local-icon {
  flex: none;
  color: var(--text-tertiary);
}

.cfm-local-name {
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cfm-local-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}
</style>
