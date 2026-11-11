<script setup lang="ts">
// 自定义外观编辑器（子路由页）：点击骨架部位弹出内嵌调色板调色；保存后返回设置页。
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { goBack } from "../store/app/nav";
import { Check, RotateCcw } from "@lucide/vue";
import {
  THEMES, customThemes, builtinVars, builtinDarkVars, deriveDarkVars, darkSurfaceVars, EDITABLE_VAR_KEYS,
  appearanceMode, resolvedAppearanceMode, createCustomTheme, updateCustomTheme, activateCustomTheme,
  type AppearanceMode, type CustomThemeVars, type ThemeId,
} from "../store/app/theme";

const route = useRoute();
const { t } = useI18n();

/** query.id：存在 = 编辑已有主题；缺失 = 新建。 */
const editingId = computed(() => (route.query.id as string) || "");

type EditorMode = AppearanceMode;

const editName = ref("");
const editDesc = ref("");
const editMode = ref<EditorMode>(appearanceMode.value);
const resolvedEditMode = computed(() => (editMode.value === "system" ? resolvedAppearanceMode.value : editMode.value));
const lightVars = ref<CustomThemeVars>(builtinVars("grass"));
const darkVars = ref<CustomThemeVars>(builtinDarkVars("grass"));
const editVars = computed<CustomThemeVars>(() =>
  resolvedEditMode.value === "light" ? lightVars.value : darkVars.value,
);

/** query.from：显式指定且存在的起点 id（内置主题或已存自定义）。 */
function fromParam(): ThemeId | undefined {
  const from = route.query.from as string | undefined;
  if (!from) return undefined;
  if (THEMES.some((t) => t.id === from)) return from as ThemeId;
  if (customThemes.value[from]) return from as ThemeId;
  return undefined;
}

function initVars() {
  editMode.value = appearanceMode.value;
  if (editingId.value && customThemes.value[editingId.value]) {
    const theme = customThemes.value[editingId.value];
    editName.value = theme.name;
    editDesc.value = theme.desc ?? "";
    lightVars.value = { ...theme.vars };
    darkVars.value = { ...(theme.darkVars ?? deriveDarkVars(theme.vars, theme.basedOn)) };
    return;
  }
  const from = fromParam();
  editName.value = t("settings.appearance.editor.defaultName");
  editDesc.value = "";
  if (!from) {
    lightVars.value = builtinVars("grass");
    darkVars.value = builtinDarkVars("grass");
    return;
  }
  if (THEMES.some((t) => t.id === from)) {
    lightVars.value = builtinVars(from);
    darkVars.value = builtinDarkVars(from);
    return;
  }
  const source = customThemes.value[from];
  lightVars.value = { ...source.vars };
  darkVars.value = { ...(source.darkVars ?? deriveDarkVars(source.vars, source.basedOn)) };
}

initVars();

/** 同实例下 query 变化（新建 ↔ 编辑）：重建表单与色板状态。 */
watch(
  () => route.query,
  () => {
    initVars();
    activeField.value = null;
    paletteOpen.value = false;
  },
);

type EditableVarKey = (typeof EDITABLE_VAR_KEYS)[number];

const PART_LABEL_KEYS: Record<EditableVarKey, string> = {
  "--color-brand": "settings.appearance.editor.partBrand",
  "--text-primary": "settings.appearance.editor.partTextPrimary",
  "--text-secondary": "settings.appearance.editor.partTextSecondary",
  "--surface-page": "settings.appearance.editor.partSurfacePage",
  "--surface-hover": "settings.appearance.editor.partSurfaceHover",
  "--border-default": "settings.appearance.editor.partBorderDefault",
  "--border-strong": "settings.appearance.editor.partBorderStrong",
};

/** 预览变量透传（局部作用域，不污染全局主题）。 */
const previewStyle = computed(() => {
  const v = editVars.value;
  const brand = v["--color-brand"];
  const dark = resolvedEditMode.value === "dark";
  return {
    "--pv-color-brand": brand,
    "--pv-color-brand-bg": colorMix(brand, dark ? 14 : 10),
    "--pv-text-primary": v["--text-primary"],
    "--pv-text-secondary": v["--text-secondary"],
    "--pv-text-tertiary": v["--text-tertiary"],
    "--pv-surface-page": v["--surface-page"],
    "--pv-surface-card": dark ? darkSurfaceVars(v["--surface-page"])["--surface-card"] : "#FFFFFF",
    "--pv-surface-hover": visibleHover(v["--surface-hover"]),
    "--pv-border-default": v["--border-default"],
    "--pv-border-strong": v["--border-strong"],
  };
});

/** 预览专用：悬停 alpha 过低时提升到可见下限；不透明值原样返回（真实应用不改）。 */
function visibleHover(v: string): string {
  const m = v.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
  if (m) {
    const alpha = m[4] === undefined ? 1 : parseFloat(m[4]);
    const MIN = 0.14; // 预览可辨识下限（真实 0.05 太淡）
    if (alpha < MIN) {
      return `rgba(${m[1]}, ${m[2]}, ${m[3]}, ${MIN})`;
    }
  }
  return v;
}

function colorMix(hex: string, alphaPct: number): string {
  const m = hex.match(/^#([0-9a-fA-F]{6})$/);
  if (!m) return hex;
  const r = parseInt(m[1].slice(0, 2), 16);
  const g = parseInt(m[1].slice(2, 4), 16);
  const b = parseInt(m[1].slice(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alphaPct / 100})`;
}

// ── 调色板状态 ──

const activeField = ref<EditableVarKey | null>(null);

const paletteOpen = ref(false);

function pickField(key: EditableVarKey) {
  activeField.value = key;
  paletteOpen.value = true;
}

const currentVal = computed(() =>
  activeField.value ? editVars.value[activeField.value] : "",
);

const PRESET_COLORS = [
  "#059669", "#10B981", "#34D399", "#0EA5E9", "#38BDF8", "#7C3AED",
  "#A855F7", "#EA580C", "#F97316", "#F59E0B", "#EAB308", "#84CC16",
  "#DC2626", "#EF4444", "#EC4899", "#F43F5E", "#14B8A6", "#0891B2",
  "#1F2937", "#374151", "#6B7280", "#9CA3AF", "#D1D5DB", "#F3F4F6",
];

function pickPreset(hex: string) {
  if (!activeField.value) return;
  applyColor(hex);
}

/** 应用颜色：原值 rgba 保留 alpha。 */
function applyColor(hex: string) {
  if (!activeField.value) return;
  if (!/^#[0-9a-fA-F]{6}$/.test(hex)) return;
  const key = activeField.value;
  const old = editVars.value[key];
  const m = old.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
  if (m) {
    const alpha = m[4] ?? "1";
    const r = parseInt(hex.slice(1, 3), 16);
    const g = parseInt(hex.slice(3, 5), 16);
    const b = parseInt(hex.slice(5, 7), 16);
    editVars.value[key] = `rgba(${r}, ${g}, ${b}, ${alpha})`;
  } else {
    editVars.value[key] = hex;
  }
}

function onHexText(e: Event) {
  const el = e.target as HTMLInputElement;
  // # 前缀在样式层展示，此处补全后再校验
  let val = el.value.trim();
  if (!val.startsWith("#")) val = "#" + val;
  if (/^#[0-9a-fA-F]{6}$/.test(val)) {
    applyColor(val);
    el.value = val.slice(1);
  } else {
    el.value = currentHex.value.replace(/^#/, "");
  }
}

const currentHex = computed(() => {
  const v = currentVal.value;
  const m = v.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
  if (m) {
    const toHex = (n: string) => (+n).toString(16).padStart(2, "0");
    return `#${toHex(m[1])}${toHex(m[2])}${toHex(m[3])}`;
  }
  return /^#[0-9a-fA-F]{6}$/.test(v) ? v : "#000000";
});

/** 重置为当前模式的 grass 默认值。 */
function resetField() {
  if (!activeField.value) return;
  const defaults = resolvedEditMode.value === "dark" ? builtinDarkVars("grass") : builtinVars("grass");
  editVars.value[activeField.value] = defaults[activeField.value];
}

// ── 保存 / 返回 ──

/** 保存：新建/更新并激活，返回设置页。 */
function save() {
  const name = editName.value.trim() || t("settings.appearance.editor.defaultName");
  const desc = editDesc.value.trim();
  const patch = {
    name,
    desc: desc || undefined,
    vars: { ...lightVars.value },
    darkVars: { ...darkVars.value },
  };
  if (editingId.value && customThemes.value[editingId.value]) {
    updateCustomTheme(editingId.value, patch);
    activateCustomTheme(editingId.value as ThemeId);
  } else {
    // 起点与 initVars 一致：内置起点由 createCustomTheme 写 basedOn，品牌衍生物直接取其 CSS 值
    const from = fromParam();
    const created = createCustomTheme(from);
    updateCustomTheme(created.id, patch);
    activateCustomTheme(created.id as ThemeId);
  }
  goBack();
}
</script>

<template>
  <div class="cte">
    <div class="cte-body">
      <!-- 左：骨架预览 -->
      <div class="cte-preview" :style="previewStyle">
        <div class="pv-titlebar">
          <span
            class="pv-part pv-logo-wrap"
            :class="{ active: activeField === '--color-brand' }"
            @click="pickField('--color-brand')"
          >
            <span class="pv-logo"></span>
            <span class="pv-tag">{{ t(PART_LABEL_KEYS["--color-brand"]) }}</span>
          </span>
          <span
            class="pv-part pv-title"
            :class="{ active: activeField === '--text-primary' }"
            @click="pickField('--text-primary')"
          >
            <span class="pv-title-text">Elytra</span>
            <span class="pv-tag">{{ t(PART_LABEL_KEYS["--text-primary"]) }}</span>
          </span>
          <span
            class="pv-part pv-window-btn"
            :class="{ active: activeField === '--border-strong' }"
            @click="pickField('--border-strong')"
          ></span>
          <span class="pv-part pv-window-btn pv-static"></span>
        </div>
        <div class="pv-main">
          <div class="pv-sidebar">
            <span
              class="pv-part pv-side-item pv-side-active pv-static"
            ></span>
            <span
              class="pv-part pv-side-item"
              :class="{ active: activeField === '--text-secondary' }"
              @click="pickField('--text-secondary')"
            ></span>
            <span
              class="pv-part pv-side-item pv-static"
            ></span>
          </div>
          <div class="pv-content">
            <div class="pv-card pv-card-head">
              <span
                class="pv-part pv-card-title pv-static"
              ></span>
              <span
                class="pv-part pv-card-btn pv-static"
              ></span>
            </div>
            <!-- 内容区分隔线：--border-default 的可点载体 -->
            <span
              class="pv-part pv-divider"
              :class="{ active: activeField === '--border-default' }"
              @click="pickField('--border-default')"
            >
              <span class="pv-divider-line"></span>
              <span class="pv-tag">{{ t(PART_LABEL_KEYS["--border-default"]) }}</span>
            </span>
            <div class="pv-card pv-card-list">
              <span
                class="pv-part pv-list-item"
                :class="{ active: activeField === '--surface-hover' }"
                @click="pickField('--surface-hover')"
              ></span>
              <span
                class="pv-part pv-list-item pv-list-sel pv-static"
              ></span>
              <span class="pv-part pv-list-item pv-list-plain"></span>
            </div>
          </div>
        </div>
        <div class="pv-foot">
          <span
            class="pv-part pv-foot-btn pv-static"
          ></span>
          <span
            class="pv-part pv-foot-btn pv-foot-primary pv-static"
          ></span>
        </div>
        <span
          class="pv-part pv-page-dot"
          :class="{ active: activeField === '--surface-page' }"
          @click="pickField('--surface-page')"
        ></span>
      </div>

      <!-- 右栏：上调色板 + 下标题/保存 -->
      <div class="cte-right">
      <div class="cte-mode-switch" role="group" :aria-label="t('settings.appearance.editor.modeLabel')">
        <button
          type="button"
          class="cte-mode-btn"
          :class="{ active: editMode === 'system' }"
          :aria-pressed="editMode === 'system'"
          @click="editMode = 'system'"
        >{{ t("settings.appearance.mode.system") }}</button>
        <button
          type="button"
          class="cte-mode-btn"
          :class="{ active: editMode === 'light' }"
          :aria-pressed="editMode === 'light'"
          @click="editMode = 'light'"
        >{{ t("settings.appearance.editor.light") }}</button>
        <button
          type="button"
          class="cte-mode-btn"
          :class="{ active: editMode === 'dark' }"
          :aria-pressed="editMode === 'dark'"
          @click="editMode = 'dark'"
        >{{ t("settings.appearance.editor.dark") }}</button>
      </div>
      <!-- 上：内嵌调色板 -->
      <div class="cte-palette" :class="{ open: paletteOpen }">
        <template v-if="activeField && paletteOpen">
          <div class="cte-palette-head">
            <span class="cte-palette-title">
              <span class="cte-title-dot" :style="{ background: currentHex }"></span>
              {{ t(PART_LABEL_KEYS[activeField]) }}
            </span>
            <button class="cte-palette-reset" :title="t('settings.appearance.editor.reset')" @click="resetField">
              <RotateCcw :size="13" />
            </button>
          </div>
          <div class="cte-palette-grid">
            <button
              v-for="c in PRESET_COLORS"
              :key="c"
              class="cte-preset"
              :class="{ picked: c.toLowerCase() === currentHex.toLowerCase() }"
              :style="{ background: c }"
              @click="pickPreset(c)"
            ></button>
          </div>
          <div class="cte-palette-custom">
            <input
              type="color"
              class="cte-custom-color"
              :value="currentHex"
              @input="pickPreset(($event.target as HTMLInputElement).value)"
              autocomplete="off"
            />
            <div class="cte-hex-box">
              <span class="cte-hex-prefix">#</span>
              <input
                class="cte-custom-text"
                :value="currentHex.replace(/^#/, '')"
                @change="onHexText"
                spellcheck="false"
                autocomplete="off"
              />
            </div>
          </div>
        </template>
        <div v-else class="cte-palette-empty">
          {{ t("settings.appearance.editor.emptyHint") }}
        </div>
      </div>

      <!-- 下：标题 / 副标题 / 保存 -->
      <div class="cte-meta">
        <div class="cte-titles">
          <input
            v-model="editName"
            class="cte-name"
            type="text"
            :placeholder="t('settings.appearance.editor.namePlaceholder')"
            maxlength="24"
            autocomplete="off"
          />
          <input
            v-model="editDesc"
            class="cte-desc"
            type="text"
            :placeholder="t('settings.appearance.editor.descPlaceholder')"
            maxlength="60"
            autocomplete="off"
          />
        </div>
        <button class="cte-save" @click="save">
          <Check :size="15" />
          <span>{{ t("common.action.save") }}</span>
        </button>
      </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.cte {
  width: 100%;
  height: 100%;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--surface-page);
}

/* 右栏：上（调色板）下（标题/保存） */
.cte-right {
  flex: 0 0 280px;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.cte-mode-switch {
  flex: none;
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 4px;
  padding: 4px;
  border: 1px solid var(--border-default);
  border-radius: 10px;
  background: var(--surface-input);
}

.cte-mode-btn {
  padding: 7px 10px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  font-weight: 600;
  cursor: pointer;
  transition: background var(--dur-fast), color var(--dur-fast), box-shadow var(--dur-fast);
}

.cte-mode-btn:hover {
  color: var(--text-primary);
}

.cte-mode-btn.active {
  background: var(--surface-elevated);
  color: var(--color-brand);
  box-shadow: var(--shadow-sm);
}

.cte-titles {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 6px 0;
}

.cte-name {
  width: 100%;
  padding: 6px 2px 8px;
  border: none;
  border-bottom: 2px solid var(--border-default);
  background: transparent;
  color: var(--text-primary);
  font-size: var(--fs-xl);
  font-weight: 700;
  outline: none;
  transition: border-color var(--dur-fast);
}

.cte-name::placeholder {
  color: var(--text-tertiary);
}

.cte-name:focus {
  border-bottom-color: var(--color-brand);
}

.cte-desc {
  width: 100%;
  padding: 2px 2px 6px;
  border: none;
  border-bottom: 1px solid var(--border-subtle, var(--border-default));
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--fs-base);
  outline: none;
  transition: border-color var(--dur-fast);
}

.cte-desc::placeholder {
  color: var(--text-tertiary);
}

.cte-desc:focus {
  border-bottom-color: var(--color-brand);
}


.cte-meta {
  flex: none;
  display: flex;
  align-items: stretch;
  gap: 12px;
  padding: 16px;
  background: var(--surface-elevated);
  border: 1px solid var(--border-default);
  border-radius: 14px;
  box-shadow: var(--shadow-md);
}

.cte-meta .cte-titles {
  justify-content: center;
}
.cte-save {
  flex: none;
  align-self: stretch;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 9px 20px;
  border: none;
  border-radius: 10px;
  background: var(--color-brand-gradient);
  color: var(--text-inverse);
  font-size: var(--fs-base);
  font-weight: 600;
  cursor: pointer;
  box-shadow: 0 2px 8px var(--color-brand-glow);
  transition: all var(--dur-fast);
}

.cte-save:hover {
  transform: translateY(-1px);
  box-shadow: 0 4px 14px var(--color-brand-glow);
  filter: brightness(1.05);
}

.cte-save:active {
  transform: translateY(0);
  box-shadow: 0 1px 4px var(--color-brand-glow);
}

.cte-body {
  flex: 1;
  width: 100%;
  min-width: 0;
  min-height: 0;
  display: flex;
  gap: 20px;
  padding: 20px;
  overflow: hidden;
}

.cte-preview {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--pv-border-default, var(--border-default));
  border-radius: 14px;
  box-shadow: var(--shadow-md);
  background: var(--pv-surface-page, var(--surface-page));
  user-select: none;
}

.pv-part {
  position: relative;
  cursor: pointer;
  transition: outline 0.12s;
}

.pv-static {
  cursor: default;
}

/* 特异性 0,3,0 高于 .pv-part:hover（0,2,0），保证不可配置部位不显示 hover 提示 */
.pv-part.pv-static:hover {
  outline: none;
  z-index: auto;
}

.pv-part.pv-static:hover .pv-tag {
  opacity: 0;
}

.pv-part:hover {
  outline: 1.5px solid var(--color-brand);
  outline-offset: 1px;
  z-index: 2;
}

.pv-part.active {
  outline: 2px solid var(--color-brand);
  outline-offset: 1px;
  z-index: 3;
}

.pv-tag {
  position: absolute;
  left: 50%;
  top: 100%;
  transform: translateX(-50%);
  margin-top: 4px;
  padding: 2px 8px;
  border-radius: var(--radius-xs);
  background: var(--color-brand);
  color: var(--text-inverse);
  font-size: var(--fs-xs);
  white-space: nowrap;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.12s;
  z-index: 5;
}

.pv-part:hover .pv-tag,
.pv-part.active .pv-tag {
  opacity: 1;
}

.pv-titlebar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 14px 16px;
  background: var(--pv-surface-card, var(--surface-elevated));
  border-bottom: 1px solid var(--pv-border-default, var(--border-default));
}

.pv-logo-wrap {
  flex: none;
  display: flex;
}

.pv-logo {
  width: 18px;
  height: 18px;
  border-radius: var(--radius-xs);
  background: var(--pv-color-brand, var(--color-brand));
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15);
}

.pv-title {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  height: 22px;
  border-radius: var(--radius-xs);
  padding: 0 8px;
}

.pv-title-text {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--pv-text-primary, var(--text-primary));
  opacity: 0.8;
  letter-spacing: 0.5px;
}

.pv-window-btn {
  flex: none;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--pv-border-strong, var(--border-strong));
}

.pv-main {
  flex: 1;
  display: flex;
  min-height: 0;
}

.pv-sidebar {
  width: 44px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 16px 0;
  background: var(--pv-surface-card, var(--surface-elevated));
  border-right: 1px solid var(--pv-border-default, var(--border-default));
}

.pv-side-item {
  width: 18px;
  height: 18px;
  border-radius: var(--radius-sm);
  background: var(--pv-text-secondary, var(--text-secondary));
  opacity: 0.6;
}

.pv-side-active {
  background: var(--pv-color-brand, var(--color-brand));
  opacity: 1;
}

.pv-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  min-width: 0;
}

.pv-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  border: 1px solid var(--pv-border-default, var(--border-default));
  border-radius: 10px;
  background: var(--pv-surface-card, var(--surface-elevated));
  box-shadow: var(--shadow-sm);
}

.pv-card-head {
  flex: none;
}

.pv-card-title {
  flex: 1;
  height: 14px;
  border-radius: var(--radius-2xs);
  background: var(--pv-text-primary, var(--text-primary));
  opacity: 0.8;
}

.pv-card-btn {
  flex: none;
  width: 52px;
  height: 22px;
  border-radius: var(--radius-sm);
  background: var(--pv-color-brand, var(--color-brand));
}

.pv-card-list {
  flex: 1;
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
  overflow: hidden;
}

/* 内容区分隔线：--border-default 的可点载体；9px 高为 1px 线留点击热区 */
.pv-divider {
  flex: none;
  display: flex;
  align-items: center;
  height: 9px;
}

.pv-divider-line {
  flex: 1;
  height: 1px;
  background: var(--pv-border-default, var(--border-default));
}

.pv-list-item {
  display: block;
  height: 20px;
  border-radius: var(--radius-sm);
  background: var(--pv-surface-hover, var(--surface-hover));
}

.pv-list-sel {
  background: var(--pv-color-brand-bg, var(--color-brand-bg));
  box-shadow: inset 0 0 0 1px var(--pv-color-brand, var(--color-brand));
}

/* 普通列表项：白底无强调（不可点击，仅占位） */
.pv-list-plain {
  background: var(--pv-surface-card, #FFFFFF);
  box-shadow: inset 0 0 0 1px var(--pv-border-default, var(--border-default));
  cursor: default;
}

.pv-list-plain:hover,
.pv-list-plain.active {
  outline: none;
}

.pv-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--pv-border-default, var(--border-default));
  background: var(--pv-surface-card, var(--surface-elevated));
}

.pv-foot-btn {
  width: 64px;
  height: 20px;
  border-radius: var(--radius-xs);
  background: var(--pv-text-secondary, var(--text-secondary));
  opacity: 0.55;
}

.pv-foot-primary {
  background: var(--pv-color-brand, var(--color-brand));
  opacity: 1;
}

.pv-page-dot {
  position: absolute;
  right: 12px;
  bottom: 50px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  border: 1px solid var(--pv-border-strong, var(--border-strong));
  background: var(--pv-surface-page, var(--surface-page));
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
}

.cte-palette {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--surface-elevated);
  border: 1px solid var(--border-default);
  border-radius: 14px;
  padding: 16px;
  box-shadow: var(--shadow-md);
  overflow-y: auto;
}

.cte-palette-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  font-size: var(--fs-base);
  color: var(--text-tertiary);
  line-height: 1.8;
}

.cte-palette-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.cte-palette-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: 0.3px;
}

.cte-title-dot {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1px solid var(--border-strong);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.15);
  flex-shrink: 0;
}

.cte-palette-reset {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all var(--dur-fast);
}

.cte-palette-reset:hover {
  color: var(--color-brand);
  border-color: var(--color-brand);
}

.cte-palette-grid {
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  gap: 8px;
  margin-bottom: 14px;
}

.cte-preset {
  aspect-ratio: 1;
  border: 2px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all 0.12s;
  padding: 0;
}

.cte-preset:hover {
  transform: scale(1.1);
}

.cte-preset.picked {
  border-color: var(--text-primary);
  box-shadow: 0 0 0 2px var(--surface-elevated) inset;
}

.cte-palette-custom {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid var(--border-default);
}

.cte-custom-color {
  flex: none;
  width: 36px;
  height: 36px;
  padding: 0;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  cursor: pointer;
}

.cte-hex-box {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 10px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-page);
  transition: border-color var(--dur-fast);
}

.cte-hex-box:focus-within {
  border-color: var(--color-brand);
}

.cte-hex-prefix {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-tertiary);
  font-family: Consolas, "SF Mono", Menlo, "DejaVu Sans Mono", monospace;
  user-select: none;
}

.cte-custom-text {
  flex: 1;
  min-width: 0;
  padding: 8px 0;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: var(--fs-base);
  font-family: Consolas, "SF Mono", Menlo, "DejaVu Sans Mono", monospace;
  letter-spacing: 0.5px;
  outline: none;
}
</style>
