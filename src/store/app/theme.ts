// 4 套内置主题（grass 默认 / end / nether / ice）+ 浅色/深色/跟随系统 + 自定义色板；
// 持久化 localStorage，向 <html> 注入 data-theme / data-mode（自定义主题固定 light）。

import { ref, watch } from "vue";
import { t } from "../../i18n";

export type ThemeId = "grass" | "end" | "nether" | "ice" | `custom-${string}`;

export const THEMES: { id: ThemeId; labelKey: string; descKey: string; swatch: string[] }[] = [
  { id: "grass", labelKey: "settings.appearance.theme.grass.label", descKey: "settings.appearance.theme.grass.desc", swatch: ["#059669", "#10B981", "#F2F6F3", "#1F2937"] },
  { id: "end", labelKey: "settings.appearance.theme.end.label", descKey: "settings.appearance.theme.end.desc", swatch: ["#7C3AED", "#A855F7", "#F6F3FB", "#241B45"] },
  { id: "nether", labelKey: "settings.appearance.theme.nether.label", descKey: "settings.appearance.theme.nether.desc", swatch: ["#EA580C", "#F97316", "#FBF5F0", "#292524"] },
  { id: "ice", labelKey: "settings.appearance.theme.ice.label", descKey: "settings.appearance.theme.ice.desc", swatch: ["#0EA5E9", "#38BDF8", "#F2F7FA", "#1E293B"] },
];

const CUSTOM_PREFIX = "custom-";

// ── 核心色板变量（品牌 + 文本 + 浅表面 + 边框）──
/** 可调变量键（顺序 = 编辑器调色板顺序）；增删可调变量只改此处。 */
export const EDITABLE_VAR_KEYS = [
  "--color-brand",
  "--text-primary",
  "--text-secondary",
  "--surface-page",
  "--surface-hover",
  "--border-default",
  "--border-strong",
] as const;

const HIDDEN_VAR_KEYS = ["--text-tertiary"] as const;

export type ThemeVarKey =
  | (typeof EDITABLE_VAR_KEYS)[number]
  | (typeof HIDDEN_VAR_KEYS)[number];

export type CustomThemeVars = Record<ThemeVarKey, string>;

export interface CustomTheme {
  id: string;
  name: string;
  desc?: string;
  /** 仅内置来源写入；缺失则按主色反查。品牌衍生物据此取该主题 CSS 值。 */
  basedOn?: ThemeId;
  vars: CustomThemeVars;
}

const STORAGE_KEY = "elytra.theme";
const CUSTOMS_KEY = "elytra.customThemes";
const MODE_STORAGE_KEY = "elytra.appearanceMode";

/** 内置主题变量值（浅色）；色值须与 styles/tokens.css 的 :root / [data-theme] 块同步（CSS 为事实源）。 */
export function builtinVars(id: ThemeId): CustomThemeVars {
  switch (id) {
    case "grass":
      return {
        "--color-brand": "#059669",
        "--text-primary": "#1F2937",
        "--text-secondary": "#6B7280",
        "--text-tertiary": "#9CA3AF",
        "--surface-page": "#F2F6F3",
        "--surface-hover": "rgba(15, 23, 42, 0.05)",
        "--border-default": "#DFE5E1",
        "--border-strong": "#CBD5D1",
      };
    case "end":
      return {
        "--color-brand": "#7C3AED",
        "--text-primary": "#241B45",
        "--text-secondary": "#6B6580",
        "--text-tertiary": "#9A93B0",
        "--surface-page": "#F6F3FB",
        "--surface-hover": "rgba(36, 27, 69, 0.06)",
        "--border-default": "#E3DEF1",
        "--border-strong": "#D2CBEA",
      };
    case "nether":
      return {
        "--color-brand": "#EA580C",
        "--text-primary": "#292524",
        "--text-secondary": "#78716C",
        "--text-tertiary": "#A8A29E",
        "--surface-page": "#FBF5F0",
        "--surface-hover": "rgba(41, 37, 36, 0.05)",
        "--border-default": "#EDE3DC",
        "--border-strong": "#DED2C8",
      };
    default: // ice 及未知回退
      return {
        "--color-brand": "#0EA5E9",
        "--text-primary": "#1E293B",
        "--text-secondary": "#64748B",
        "--text-tertiary": "#94A3B8",
        "--surface-page": "#F2F7FA",
        "--surface-hover": "rgba(15, 23, 42, 0.05)",
        "--border-default": "#DCE7EE",
        "--border-strong": "#C6D6E0",
      };
  }
}

/** 遗留 light/dark/system 及未知值 → grass。 */
function normalize(raw: string | null): ThemeId {
  if (raw && THEMES.some((t) => t.id === raw)) return raw as ThemeId;
  if (raw && raw.startsWith(CUSTOM_PREFIX)) return raw as ThemeId;
  return "grass";
}

export type AppearanceMode = "light" | "dark" | "system";

function readAppearanceMode(): AppearanceMode {
  try {
    const value = localStorage.getItem(MODE_STORAGE_KEY);
    if (value === "light" || value === "dark" || value === "system") return value;
  } catch {
    // 存储不可用时按跟随系统处理
  }
  return "system";
}

export const appearanceMode = ref<AppearanceMode>(readAppearanceMode());

const systemDark = ref(false);
let systemMedia: MediaQueryList | null = null;

function initSystemThemeWatch() {
  if (systemMedia || typeof window === "undefined" || !window.matchMedia) return;
  systemMedia = window.matchMedia("(prefers-color-scheme: dark)");
  systemDark.value = systemMedia.matches;
  systemMedia.addEventListener("change", (event) => {
    systemDark.value = event.matches;
    if (appearanceMode.value === "system") applyThemeAnimated();
  });
}

function resolvedMode(): "light" | "dark" {
  if (appearanceMode.value === "system") return systemDark.value ? "dark" : "light";
  return appearanceMode.value;
}

export function setAppearanceMode(mode: AppearanceMode) {
  appearanceMode.value = mode;
}

// 在任何 applyTheme 之前先取到系统偏好，避免首帧按 light 绘制。
initSystemThemeWatch();

export const themeId = ref<ThemeId>(normalize(localStorage.getItem(STORAGE_KEY)));

// ── 自定义主题集合 ──

export const customThemes = ref<Record<string, CustomTheme>>(loadCustoms());

function loadCustoms(): Record<string, CustomTheme> {
  try {
    const raw = localStorage.getItem(CUSTOMS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, CustomTheme>;
    const required: readonly ThemeVarKey[] = [...EDITABLE_VAR_KEYS, ...HIDDEN_VAR_KEYS];
    const out: Record<string, CustomTheme> = {};
    for (const [id, t] of Object.entries(parsed)) {
      if (!t || typeof t.id !== "string" || typeof t.name !== "string" || typeof t.vars !== "object") continue;
      if (required.every((k) => typeof t.vars[k] === "string")) {
        if (typeof t.desc !== "string") t.desc = "";
        // 旧数据无 basedOn（按主色反查兼容）；非内置来源残留值丢弃
        if (t.basedOn !== undefined && !THEMES.some((b) => b.id === t.basedOn)) delete t.basedOn;
        out[id] = t;
      }
    }
    return out;
  } catch {
    return {};
  }
}

function saveCustoms() {
  localStorage.setItem(CUSTOMS_KEY, JSON.stringify(customThemes.value));
}

export function createCustomTheme(from: ThemeId = themeId.value): CustomTheme {
  // vars 仅含可编辑/隐藏变量；品牌衍生物在 applyCustomThemeStyle 注入，不入库
  const fromBuiltin: ThemeId | null = THEMES.some((t) => t.id === from) ? from : null;
  const base = fromBuiltin
    ? builtinVars(fromBuiltin)
    : (customThemes.value[from]?.vars ?? builtinVars("grass"));
  let n = Object.keys(customThemes.value).length + 1;
  let id = `${CUSTOM_PREFIX}${n}`;
  while (customThemes.value[id]) {
    n += 1;
    id = `${CUSTOM_PREFIX}${n}`;
  }
  const theme: CustomTheme = {
    id,
    name: t("settings.appearance.customName", { n }),
    desc: t("settings.appearance.customDesc"),
    // 仅内置来源记录 basedOn；自定义来源按主色反查兼容
    ...(fromBuiltin ? { basedOn: fromBuiltin } : {}),
    vars: { ...base },
  };
  customThemes.value = { ...customThemes.value, [id]: theme };
  saveCustoms();
  return theme;
}

export function updateCustomTheme(id: string, patch: Partial<CustomTheme>) {
  const theme = customThemes.value[id];
  if (!theme) return;
  customThemes.value = {
    ...customThemes.value,
    [id]: { ...theme, ...patch, vars: patch.vars ? { ...theme.vars, ...patch.vars } : theme.vars },
  };
  saveCustoms();
  if (themeId.value === id) applyTheme(); // 编辑器实时预览：不做过渡，避免拖动取色器时颜色滞后
}

export function deleteCustomTheme(id: string) {
  const { [id]: _, ...rest } = customThemes.value;
  customThemes.value = rest;
  saveCustoms();
  removeCustomStyle(id);
  if (themeId.value === id) {
    themeId.value = "grass"; // watcher 负责带过渡应用
  }
}

export function isCustom(id: string): boolean {
  return id.startsWith(CUSTOM_PREFIX);
}

// ── 主题应用与持久化 ──

/** 品牌衍生变量键（不可编辑：随主色派生或取自内置主题 CSS）。 */
const BRAND_DERIVED_KEYS = [
  "--color-brand-dark",
  "--color-brand-hover",
  "--color-brand-bg",
  "--color-brand-glow",
  "--color-brand-gradient",
] as const;

/** 取内置主题的品牌衍生变量（事实源 styles/tokens.css）：临时切 data-theme 后同帧还原；读不到 → null。 */
function readBuiltinDerived(id: ThemeId): Record<string, string> | null {
  if (typeof document === "undefined") return null;
  const root = document.documentElement;
  const prev = root.dataset.theme;
  const prevMode = root.dataset.mode;
  const out: Record<string, string> = {};
  try {
    // 自定义主题按浅色色板创建，派生值也必须读浅色块，不能受当前深色模式影响。
    root.dataset.theme = id;
    root.dataset.mode = "light";
    const cs = getComputedStyle(root);
    for (const k of BRAND_DERIVED_KEYS) {
      const v = cs.getPropertyValue(k).trim();
      if (!v) return null;
      out[k] = v;
    }
  } finally {
    // 无论成功与否都还原（prev undefined = 原本无属性）
    if (prev === undefined) delete root.dataset.theme;
    else root.dataset.theme = prev;
    if (prevMode === undefined) delete root.dataset.mode;
    else root.dataset.mode = prevMode;
  }
  return out;
}

/** 品牌衍生值来源主题：① 显式 basedOn；② 按 --color-brand 反查；主色被改则 null。 */
function resolveBuiltinSource(brand: string, fromTheme?: string): ThemeId | null {
  const target = brand.trim().toLowerCase();
  const candidate =
    (fromTheme ? THEMES.find((t) => t.id === fromTheme)?.id : undefined) ??
    THEMES.find((t) => builtinVars(t.id)["--color-brand"].toLowerCase() === target)?.id ??
    null;
  return candidate && builtinVars(candidate)["--color-brand"].toLowerCase() === target ? candidate : null;
}

/** 品牌主色 → 衍生物：命中内置来源且主色未改取 CSS 值，否则按算法派生。 */
function deriveBrandVars(brand: string, fromTheme?: string): Record<string, string> {
  const builtin = resolveBuiltinSource(brand, fromTheme);
  if (builtin) {
    const fromCss = readBuiltinDerived(builtin);
    if (fromCss) return fromCss;
  }
  const hex = brand.replace("#", "");
  if (!/^[0-9a-fA-F]{6}$/.test(hex)) {
    // 非法色值回退默认（bg 0.08 / glow 0.22）
    return {
      "--color-brand-dark": brand,
      "--color-brand-hover": brand,
      "--color-brand-bg": "rgba(5, 150, 105, 0.08)",
      "--color-brand-glow": "rgba(5, 150, 105, 0.22)",
      "--color-brand-gradient": `linear-gradient(135deg, ${brand}, ${brand})`,
    };
  }
  const r = parseInt(hex.slice(0, 2), 16);
  const g = parseInt(hex.slice(2, 4), 16);
  const b = parseInt(hex.slice(4, 6), 16);
  const darken = (f: number) => `#${[r, g, b].map((v) => Math.round(v * f)).map((v) => v.toString(16).padStart(2, "0")).join("")}`;
  const lighten = (f: number) =>
    `#${[r, g, b].map((v) => Math.round(v + (255 - v) * f)).map((v) => v.toString(16).padStart(2, "0")).join("")}`;
  return {
    "--color-brand-dark": darken(0.85),
    "--color-brand-hover": darken(0.85),
    "--color-brand-bg": `rgba(${r}, ${g}, ${b}, 0.08)`,
    "--color-brand-glow": `rgba(${r}, ${g}, ${b}, 0.2)`,
    "--color-brand-gradient": `linear-gradient(135deg, ${brand}, ${lighten(0.28)})`,
  };
}

export function applyCustomThemeStyle(id: string) {
  const t = customThemes.value[id];
  if (!t) return;
  const derived = deriveBrandVars(t.vars["--color-brand"], t.basedOn);
  const lines = [
    ...Object.entries(t.vars).map(([k, v]) => `  ${k}: ${v};`),
    ...Object.entries(derived).map(([k, v]) => `  ${k}: ${v};`),
  ].join("\n");
  const css = `[data-theme="${id}"] {\n${lines}\n}`;
  let el = document.getElementById(`custom-theme-${id}`) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = `custom-theme-${id}`;
    document.head.appendChild(el);
  }
  el.textContent = css;
}

function removeCustomStyle(id: string) {
  document.getElementById(`custom-theme-${id}`)?.remove();
}

/** 自定义须先注入动态规则再设属性；挂载前调用一次。 */
export function applyTheme() {
  const custom = isCustom(themeId.value);
  if (custom) {
    applyCustomThemeStyle(themeId.value);
  }
  const root = document.documentElement;
  root.dataset.theme = themeId.value;
  // 自定义主题是显式色板，不参与内置深色覆盖。
  root.dataset.mode = custom ? "light" : resolvedMode();
}

let themeTransitionTimer: number | undefined;

/**
 * 标题栏用的是 background-image 渐变，CSS 无法在渐变之间插值。
 * 在切换前把旧渐变铺成负 z-index 覆盖层，主题变量更新后再淡出，露出新渐变。
 */
function crossfadeTitlebarGradient() {
  for (const bar of document.querySelectorAll<HTMLElement>(".titlebar")) {
    for (const old of bar.querySelectorAll<HTMLElement>(".titlebar-theme-fade")) {
      old.remove();
    }
    const gradient = getComputedStyle(bar).backgroundImage;
    if (!gradient || gradient === "none") continue;
    const overlay = document.createElement("div");
    overlay.className = "titlebar-theme-fade";
    Object.assign(overlay.style, {
      position: "absolute",
      inset: "0",
      zIndex: "-1",
      pointerEvents: "none",
      backgroundImage: gradient,
      opacity: "1",
      willChange: "opacity",
    });
    // 主题过渡期全局 transition 带 !important，覆盖层必须用内联 !important 才能保留自己的淡出。
    overlay.style.setProperty(
      "transition",
      "opacity 0.45s cubic-bezier(0.4, 0, 0.2, 1)",
      "important",
    );
    bar.appendChild(overlay);
    void overlay.offsetWidth; // 先落旧渐变的第一帧，再触发淡出
    overlay.style.opacity = "0";
    window.setTimeout(() => overlay.remove(), 500);
  }
}

/** 用户切换主题时给根节点临时开启颜色过渡；初始 applyTheme 不带动画，避免首屏颜色渐入。 */
function applyThemeAnimated() {
  if (typeof document === "undefined" || typeof window === "undefined") {
    applyTheme();
    return;
  }
  const root = document.documentElement;
  root.classList.add("theme-switching");
  crossfadeTitlebarGradient();
  applyTheme();
  window.clearTimeout(themeTransitionTimer);
  themeTransitionTimer = window.setTimeout(() => {
    root.classList.remove("theme-switching");
  }, 500);
}

export function activateCustomTheme(id: string) {
  if (!customThemes.value[id]) return;
  applyCustomThemeStyle(id);
  themeId.value = id as ThemeId;
}

watch(themeId, (id) => {
  localStorage.setItem(STORAGE_KEY, id);
  applyThemeAnimated();
});

watch(appearanceMode, (mode) => {
  try {
    localStorage.setItem(MODE_STORAGE_KEY, mode);
  } catch {
    // 仅本会话生效
  }
  applyThemeAnimated();
});

/** 挂载时注入已有自定义主题样式（<style> 不持久）。 */
export function ensureCustomThemesApplied() {
  for (const id of Object.keys(customThemes.value)) {
    if (customThemes.value[id]) applyCustomThemeStyle(id);
  }
  if (isCustom(themeId.value)) applyTheme();
}