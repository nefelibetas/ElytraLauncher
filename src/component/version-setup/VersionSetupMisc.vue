<script setup lang="ts">
// 版本设置-其他 tab：版本图标管理。
// 图标优先级：自定义（version-icon.png）> 整合包项目图（pack-icon.png）> loader 图标。
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { ImagePlus, Undo2 } from "@lucide/vue";
import {
  versionIcons, iconSources,
  loadVersionIcon, loadIconSource, setVersionIcon, resetVersionIcon,
  type VersionIconSource,
} from "../../store/task/versionIcon";
import { open } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import SetupCard from "../common/SetupCard.vue";

const route = useRoute();
const { t } = useI18n();
const versionName = computed(() => String(route.query.name ?? ""));

const currentIcon = computed(() => versionIcons[versionName.value]);
const source = computed<VersionIconSource>(() => iconSources[versionName.value] ?? "default");

const SOURCE_LABEL_KEYS: Record<VersionIconSource, string> = {
  custom: "settings.version.setup.misc.sourceCustom",
  pack: "settings.version.setup.misc.sourcePack",
  default: "settings.version.setup.misc.sourceDefault",
};

const sourceLabel = computed(() => t(SOURCE_LABEL_KEYS[source.value]));

onMounted(() => {
  void loadVersionIcon(versionName.value);
  void loadIconSource(versionName.value);
});

/** 自定义图标：选图 → 后端魔数校验并复制，成功后缓存刷新。 */
async function pickCustomIcon() {
  const path = await open({
    multiple: false,
    filters: [{ name: t("settings.version.setup.misc.imageFilter"), extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
  });
  if (!path) return;
  try {
    await setVersionIcon(versionName.value, String(path));
    showToast(t("settings.version.setup.misc.updated"));
  } catch (e) {
    showToast(errMsg(e));
  }
}

/** 还原为默认：删除自定义图；无自定义时按钮禁用（幂等保护）。 */
async function restoreDefault() {
  try {
    await resetVersionIcon(versionName.value);
    showToast(t("settings.version.setup.misc.resetDone"));
  } catch (e) {
    showToast(errMsg(e));
  }
}
</script>

<template>
  <div class="vs-page">
    <SetupCard :padded="false" :title="t('settings.version.setup.misc.title')">
      <div class="vs-icon-row">
        <img v-if="currentIcon" :src="currentIcon" class="vs-icon-preview" :alt="t('settings.version.setup.misc.alt')" />
        <div v-else class="vs-icon-preview vs-icon-empty">{{ t("settings.version.setup.misc.none") }}</div>
        <span class="setup-desc">{{ sourceLabel }}</span>
      </div>
    </SetupCard>

    <SetupCard
      :padded="false"
      :title="t('settings.version.setup.misc.customTitle')"
      :desc="t('settings.version.setup.misc.customDesc')"
    >
      <div class="setup-btn-row">
        <button class="setup-btn" @click="pickCustomIcon">
          <ImagePlus :size="15" />
          <span>{{ t("settings.version.setup.misc.pickImage") }}</span>
        </button>
      </div>
    </SetupCard>

    <SetupCard
      :padded="false"
      :title="t('settings.version.setup.misc.resetTitle')"
      :desc="t('settings.version.setup.misc.resetDesc')"
    >
      <div class="setup-btn-row">
        <button
          class="setup-btn"
          :disabled="source !== 'custom'"
          @click="restoreDefault"
        >
          <Undo2 :size="15" />
          <span>{{ t("settings.version.setup.misc.resetButton") }}</span>
        </button>
      </div>
    </SetupCard>
  </div>
</template>

<style scoped>

.setup-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.vs-icon-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 16px 14px;
}

.vs-icon-preview {
  width: 48px;
  height: 48px;
  border-radius: var(--radius-sm);
  object-fit: contain;
  background: var(--surface-page);
  border: 1px solid var(--border-default);
  flex-shrink: 0;
}

.vs-icon-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
</style>
