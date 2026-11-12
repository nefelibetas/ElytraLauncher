<script setup lang="ts">
// 工具箱：左侧工具列表 + 右侧当前工具（布局与实例管理页同构）；工具本体按需异步加载。
import type { Component } from "vue";
import { computed, defineAsyncComponent, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  Server, Type, Sword, PawPrint, Terminal, Paintbrush, Download, Map, MapPin,
} from "@lucide/vue";
import ScrollTopFab from "../common/ScrollTopFab.vue";

const ServerStatusTool = defineAsyncComponent(() => import("./ServerStatusTool.vue"));
const MotdGeneratorTool = defineAsyncComponent(() => import("./MotdGeneratorTool.vue"));
const GiveGeneratorTool = defineAsyncComponent(() => import("./GiveGeneratorTool.vue"));
const EntityNbtTool = defineAsyncComponent(() => import("./EntityNbtTool.vue"));
const CommandGeneratorTool = defineAsyncComponent(() => import("./CommandGeneratorTool.vue"));
const SkinPainterTool = defineAsyncComponent(() => import("./SkinPainterTool.vue"));
const DownloadTool = defineAsyncComponent(() => import("./DownloadTool.vue"));
const SeedMapTool = defineAsyncComponent(() => import("./SeedMapTool.vue"));
const MapFolderTool = defineAsyncComponent(() => import("./MapFolderTool.vue"));

const { t } = useI18n();

interface ToolCard {
  key: string;
  titleKey: string;
  descKey: string;
  icon: Component;
  component: Component;
}

const tools: ToolCard[] = [
  { key: "status", titleKey: "tools.tabs.status", descKey: "tools.status.subtitle", icon: Server, component: ServerStatusTool },
  { key: "motd", titleKey: "tools.tabs.motd", descKey: "tools.motd.subtitle", icon: Type, component: MotdGeneratorTool },
  { key: "give", titleKey: "tools.tabs.give", descKey: "tools.give.subtitle", icon: Sword, component: GiveGeneratorTool },
  { key: "entity", titleKey: "tools.tabs.entity", descKey: "tools.entity.subtitle", icon: PawPrint, component: EntityNbtTool },
  { key: "command", titleKey: "tools.tabs.command", descKey: "tools.command.subtitle", icon: Terminal, component: CommandGeneratorTool },
  { key: "skinpaint", titleKey: "tools.tabs.skinpaint", descKey: "tools.skinpainter.subtitle", icon: Paintbrush, component: SkinPainterTool },
  { key: "seedmap", titleKey: "tools.tabs.seedmap", descKey: "tools.seedmap.subtitle", icon: Map, component: SeedMapTool },
  { key: "worldpreview", titleKey: "tools.tabs.worldpreview", descKey: "tools.worldpreview.subtitle", icon: MapPin, component: MapFolderTool },
  { key: "download", titleKey: "tools.tabs.download", descKey: "tools.download.subtitle", icon: Download, component: DownloadTool },
];

/** 默认打开首个工具，右侧不留空；切换只改右侧内容。 */
const selected = ref(tools[0].key);
const active = computed(() => tools.find(tool => tool.key === selected.value) ?? tools[0]);
</script>

<template>
  <div class="tb">
    <aside class="tb-side">
      <div class="tb-group-label">{{ t("tools.tabs.title") }}</div>
      <div class="tb-list">
        <button
          v-for="tool in tools"
          :key="tool.key"
          type="button"
          class="tb-item"
          :class="{ active: tool.key === selected }"
          @click="selected = tool.key"
        >
          <span class="tb-icon">
            <component :is="tool.icon" :size="16" />
          </span>
          <span class="tb-item-text">
            <span class="tb-item-title">{{ t(tool.titleKey) }}</span>
            <span class="tb-item-desc" v-tooltip="t(tool.descKey)">{{ t(tool.descKey) }}</span>
          </span>
        </button>
      </div>
    </aside>

    <section class="tb-main">
      <Transition name="tb-fade" mode="out-in">
        <component :is="active.component" :key="active.key" />
      </Transition>
      <ScrollTopFab />
    </section>
  </div>
</template>

<style scoped>
.tb {
  display: flex;
  height: 100%;
}

.tb-side {
  flex: 0 0 240px;
  min-width: 220px;
  background-color: var(--surface-elevated);
  border-right: 1px solid var(--border-default);
  padding: 16px 12px;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  overflow-x: hidden;
}

.tb-group-label {
  font-size: var(--fs-xs);
  font-weight: 500;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.8px;
  padding: 8px 8px 6px;
}

.tb-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tb-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  text-align: left;
  cursor: pointer;
  transition: background 0.12s;
}

.tb-item:hover {
  background: var(--surface-hover);
}

.tb-item.active {
  background: var(--color-brand-bg);
}

.tb-icon {
  flex: none;
  display: flex;
  color: var(--text-tertiary);
}

.tb-item.active .tb-icon {
  color: var(--color-brand);
}

.tb-item-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tb-item-title {
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--text-primary);
}

.tb-item.active .tb-item-title {
  color: var(--color-brand);
}

.tb-item-desc {
  min-width: 0;
  font-size: var(--fs-xs);
  line-height: 1.4;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tb-main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  overflow-x: hidden;
  background: var(--surface-page);
}

/* 与实例卡片内的 tab 切换一致：只做淡入淡出，不做整页横滑 */
.tb-fade-enter-active,
.tb-fade-leave-active {
  transition: opacity var(--dur-fast) ease;
}

.tb-fade-enter-from,
.tb-fade-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .tb-fade-enter-active,
  .tb-fade-leave-active {
    transition: none;
  }
}
</style>
