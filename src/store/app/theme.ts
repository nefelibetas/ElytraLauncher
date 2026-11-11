// 4 套内置主题（grass 默认 / end / nether / ice）+ 浅色/深色/跟随系统 + 自定义双色板；
// 持久化 localStorage，向 <html> 注入 data-theme / data-mode（自定义主题分别使用浅色/深色色板）。

import { computed, ref, watch } from "vue";
import { t } from "../../i18n";

export type ThemeId = "grass" | "end" | "nether" | "ice" | `custom-${string}`;

export const THEMES: { id: ThemeId; labelKey: string; descKey: string; swatch: string[] }[] = [
  { id: "grass", labelKey: "settings.appearance.theme.grass.label", descKey: "settings.appearance.theme.grass.desc", swatch: ["#059669", "#10B981", "#F2F6F3", "#1F2937"] },
  { id: "end", labelKey: "settings.appearance.theme.end.label", descKey: "settings.appearance.theme.end.desc", swatch: ["#7C3AED", "#A855F7", "#F6F3FB", "#241B45"] },
  { id: "nether", labelKey: "settings.appearance.theme.nether.label", descKey: "settings.appearance.theme.nether.desc", swatch: ["#EA580C", "#F97316", "#FBF5F0", "#292524"] },
  { id: "ice", labelKey: "settings.appearance.theme.ice.label", descKey: "settings.appearance.theme.ice.desc", swatch: ["#0EA5E9", "#38BDF8", "#F2F7FA", "#1E293B"] },
];

const CUSTOM_PREFIX = "custom-";

// ── 核心色板变量（品牌 + 文本 + 页面/悬停表面 + 边框）──
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
  /** 深色色板；旧数据缺失时按 vars/basedOn 派生，不阻塞使用。 */
  darkVars?: CustomThemeVars;
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

/** 内置主题的深色核心色板；须与 styles/tokens.css 的 dark [data-theme] 块同步。 */
export function builtinDarkVars(id: ThemeId): CustomThemeVars {
  const text = {
    "--text-primary": "#E8F0EB",
    "--text-secondary": "#A7B5AC",
    "--text-tertiary": "#7C8B82",
  };
  switch (id) {
    case "end":
      return {
        "--color-brand": "#A78BFA",
        ...text,
        "--surface-page": "#12101C",
        "--surface-hover": "rgba(255, 255, 255, 0.06)",
        "--border-default": "#2E2845",
        "--border-strong": "#423A5E",
      };
    case "nether":
      return {
        "--color-brand": "#FB923C",
        ...text,
        "--surface-page": "#171210",
        "--surface-hover": "rgba(255, 255, 255, 0.06)",
        "--border-default": "#3A2C24",
        "--border-strong": "#533E32",
      };
    case "ice":
      return {
        "--color-brand": "#38BDF8",
        ...text,
        "--surface-page": "#0E141A",
        "--surface-hover": "rgba(255, 255, 255, 0.06)",
        "--border-default": "#26323D",
        "--border-strong": "#36485A",
      };
    default: // grass 及未知回退
      return {
        "--color-brand": "#34D399",
        ...text,
        "--surface-page": "#0D1511",
        "--surface-hover": "rgba(255, 255, 255, 0.06)",
        "--border-default": "#26382F",
        "--border-strong": "#375345",
      };
  }
}

/** 把浅色色板映射为可用的深色色板（旧自定义主题/自定义来源的兜底）。 */
export function deriveDarkVars(vars: CustomThemeVars, fromTheme?: ThemeId): CustomThemeVars {
  const builtin = fromTheme && THEMES.some((t) => t.id === fromTheme) ? builtinDarkVars(fromTheme) : null;
  if (builtin) {
    // 浅色主色被改过时保留其色相，只提亮到深色底上可读的亮度。
    const lightBrand = vars["--color-brand"].toLowerCase();
    const baseBrand = builtinVars(fromTheme!)["--color-brand"].toLowerCase();
    if (lightBrand !== baseBrand) {
      builtin["--color-brand"] = lightenHex(vars["--color-brand"], 0.2) ?? vars["--color-brand"];
    }
    return builtin;
  }
  return {
    "--color-brand": lightenHex(vars["--color-brand"], 0.2) ?? vars["--color-brand"],
    "--text-primary": "#E8F0EB",
    "--text-secondary": "#A7B5AC",
    "--text-tertiary": "#7C8B82",
    "--surface-page": "#0E1511",
    "--surface-hover": "rgba(255, 255, 255, 0.06)",
    "--border-default": "#2A3731",
    "--border-strong": "#3A4A42",
  };
}

/** 深色自定义主题的卡片/浮层色调：从页面底色提亮，避免继承内置主题的色相。 */
export function darkSurfaceVars(page: string): Record<string, string> {
  const mix = (target: string, ratio: number) => mixHex(page, target, ratio) ?? page;
  return {
    "--surface-card": mix("#FFFFFF", 0.08),
    "--surface-elevated": mix("#FFFFFF", 0.13),
    "--surface-input": mix("#FFFFFF", 0.04),
    "--border-subtle": "rgba(255, 255, 255, 0.08)",
    "--edge-line": "rgba(255, 255, 255, 0.10)",
    "--edge-shade": "rgba(0, 0, 0, 0.30)",
    "--text-inverse": "#0B100D",
  };
}

/** 把 #RRGGBB 向目标色混合；非法值返回 null。 */
export function mixHex(hex: string, target: string, ratio: number): string | null {
  const parse = (value: string) => {
    const m = value.trim().match(/^#([0-9a-fA-F]{6})$/);
    return m ? [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16)) : null;
  };
  const from = parse(hex);
  const to = parse(target);
  if (!from || !to) return null;
  const mix = (a: number, b: number) => Math.round(a + (b - a) * ratio);
  return `#${[mix(from[0], to[0]), mix(from[1], to[1]), mix(from[2], to[2])]
    .map((v) => v.toString(16).padStart(2, "0"))
    .join("")}`;
}

/** #RRGGBB 提亮；非法值返回 null。 */
function lightenHex(value: string, factor: number): string | null {
  const match = value.trim().match(/^#([0-9a-fA-F]{6})$/);
  if (!match) return null;
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(match[1].slice(i, i + 2), 16));
  const mix = (v: number) => Math.round(v + (255 - v) * factor);
  return `#${[mix(r), mix(g), mix(b)].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
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

/** 当前实际生效的明暗模式（system 已解析），供编辑器预览等使用。 */
export const resolvedAppearanceMode = computed<"light" | "dark">(() => resolvedMode());

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
        // 旧数据无 darkVars：保留缺失，应用时按 vars/basedOn 派生
        if (t.darkVars !== undefined) {
          if (
            typeof t.darkVars !== "object" ||
            t.darkVars === null ||
            !required.every((k) => typeof t.darkVars?.[k] === "string")
          ) {
            delete t.darkVars;
          }
        }
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
  // vars/darkVars 仅含可编辑/隐藏变量；品牌衍生物在 applyCustomThemeStyle 注入，不入库
  const fromBuiltin: ThemeId | null = THEMES.some((t) => t.id === from) ? from : null;
  const source = !fromBuiltin ? customThemes.value[from] : undefined;
  const base = fromBuiltin ? builtinVars(fromBuiltin) : (source?.vars ?? builtinVars("grass"));
  const baseDark = fromBuiltin
    ? builtinDarkVars(fromBuiltin)
    : (source?.darkVars ?? deriveDarkVars(base, source?.basedOn));
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
    darkVars: { ...baseDark },
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
    [id]: {
      ...theme,
      ...patch,
      vars: patch.vars ? { ...theme.vars, ...patch.vars } : theme.vars,
      darkVars: patch.darkVars ? { ...theme.darkVars, ...patch.darkVars } : theme.darkVars,
    },
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

/** 取内置主题在指定模式下的品牌衍生变量（事实源 styles/tokens.css）：临时切 data-theme 后同帧还原；读不到 → null。 */
function readBuiltinDerived(id: ThemeId, mode: "light" | "dark"): Record<string, string> | null {
  if (typeof document === "undefined") return null;
  const root = document.documentElement;
  const prev = root.dataset.theme;
  const prevMode = root.dataset.mode;
  const out: Record<string, string> = {};
  try {
    // 自定义主题按对应模式的色板派生，不能被当前模式或其它主题变量污染。
    root.dataset.theme = id;
    root.dataset.mode = mode;
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

/** 内置主题在指定模式下的品牌主色。 */
function builtinBrand(id: ThemeId, mode: "light" | "dark"): string {
  return (mode === "dark" ? builtinDarkVars(id) : builtinVars(id))["--color-brand"].toLowerCase();
}

/** 品牌衍生值来源主题：① 显式 basedOn；② 按 --color-brand 反查；主色被改则 null。 */
function resolveBuiltinSource(brand: string, fromTheme: string | undefined, mode: "light" | "dark"): ThemeId | null {
  const target = brand.trim().toLowerCase();
  const candidate =
    (fromTheme ? THEMES.find((t) => t.id === fromTheme)?.id : undefined) ??
    THEMES.find((t) => builtinBrand(t.id, mode) === target)?.id ??
    null;
  return candidate && builtinBrand(candidate, mode) === target ? candidate : null;
}

/** 品牌主色 → 衍生物：命中内置来源且主色未改取 CSS 值，否则按算法派生。 */
function deriveBrandVars(brand: string, fromTheme: string | undefined, mode: "light" | "dark"): Record<string, string> {
  const builtin = resolveBuiltinSource(brand, fromTheme, mode);
  if (builtin) {
    const fromCss = readBuiltinDerived(builtin, mode);
    if (fromCss) return fromCss;
  }
  const darkMode = mode === "dark";
  const hex = brand.replace("#", "");
  if (!/^[0-9a-fA-F]{6}$/.test(hex)) {
    return {
      "--color-brand-dark": brand,
      "--color-brand-hover": brand,
      "--color-brand-bg": `rgba(5, 150, 105, ${darkMode ? 0.14 : 0.08})`,
      "--color-brand-glow": `rgba(5, 150, 105, ${darkMode ? 0.28 : 0.22})`,
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
    "--color-brand-dark": darken(darkMode ? 0.8 : 0.85),
    "--color-brand-hover": darkMode ? lighten(0.2) : darken(0.85),
    "--color-brand-bg": `rgba(${r}, ${g}, ${b}, ${darkMode ? 0.14 : 0.08})`,
    "--color-brand-glow": `rgba(${r}, ${g}, ${b}, ${darkMode ? 0.28 : 0.2})`,
    "--color-brand-gradient": darkMode
      ? `linear-gradient(135deg, ${darken(0.8)}, ${brand})`
      : `linear-gradient(135deg, ${brand}, ${lighten(0.28)})`,
  };
}

/** 注入自定义主题的浅色与深色规则；深色块额外派生卡片/浮层色调。 */
export function applyCustomThemeStyle(id: string) {
  const t = customThemes.value[id];
  if (!t) return;
  const darkVars = t.darkVars ?? deriveDarkVars(t.vars, t.basedOn);
  const lightDerived = deriveBrandVars(t.vars["--color-brand"], t.basedOn, "light");
  const darkDerived = deriveBrandVars(darkVars["--color-brand"], t.basedOn, "dark");
  const darkSurfaces = darkSurfaceVars(darkVars["--surface-page"]);
  const block = (vars: CustomThemeVars, derived: Record<string, string>, extra?: Record<string, string>) =>
    [
      ...Object.entries(vars).map(([k, v]) => `  ${k}: ${v};`),
      ...Object.entries(derived).map(([k, v]) => `  ${k}: ${v};`),
      ...Object.entries(extra ?? {}).map(([k, v]) => `  ${k}: ${v};`),
    ].join("\n");
  const css = [
    `[data-theme="${id}"] {\n${block(t.vars, lightDerived)}\n}`,
    `html[data-mode="dark"][data-theme="${id}"] {\n${block(darkVars, darkDerived, darkSurfaces)}\n}`,
  ].join("\n\n");
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
  // 自定义主题也跟随外观模式，只是色板来自 darkVars（缺失时派生）。
  root.dataset.mode = resolvedMode();
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