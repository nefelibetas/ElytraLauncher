// v-tooltip="文本"：主题化气泡替代 UA 原生 title；hover 400ms 显示、移出 100ms 隐藏、跟随鼠标（越界翻转）、空值不显示。
// 未绑定的元素自动检测：计算样式含 text-overflow: ellipsis 且实际被裁剪时，显示自身全文。
import type { Directive } from "vue";

interface TooltipTarget extends HTMLElement {
  __ttCleanup?: () => void;
  /** 最新提示文本：hover 时读它。闭包若直接捕获 `binding.value` 会停在首次绑定快照，故存可变最新值。 */
  __ttText?: string;
}

const TIP_CLASS = "elytra-tooltip";
const SHOW_DELAY = 400;
const HIDE_DELAY = 100;
const OFFSET_X = 12;
const OFFSET_Y = 14;

let tipEl: HTMLElement | null = null;
let showTimer: number | undefined;
let hideTimer: number | undefined;
let currentTarget: TooltipTarget | null = null;
let currentText = "";
let mouseX = 0;
let mouseY = 0;

function ensureTip(): HTMLElement {
  if (!tipEl || !tipEl.isConnected) {
    tipEl = document.createElement("div");
    tipEl.className = TIP_CLASS;
    document.body.appendChild(tipEl);
  }
  return tipEl;
}

function reposition() {
  if (!tipEl || !currentTarget || !tipEl.classList.contains("visible")) return;
  if (!currentTarget.isConnected) {
    tipEl.classList.remove("visible");
    currentTarget = null;
    return;
  }
  const tw = tipEl.offsetWidth;
  const th = tipEl.offsetHeight;
  // 鼠标右下；越界翻转到左/上
  let left = mouseX + OFFSET_X;
  let top = mouseY + OFFSET_Y;
  if (left + tw > window.innerWidth - 4) left = mouseX - tw - OFFSET_X;
  if (top + th > window.innerHeight - 4) top = mouseY - th - OFFSET_Y;
  left = Math.max(4, left);
  top = Math.max(4, top);
  tipEl.style.left = `${left}px`;
  tipEl.style.top = `${top}px`;
}

function show(target: TooltipTarget, text: string | (() => string)) {
  if (!text) return;
  clearTimeout(hideTimer);
  showTimer = setTimeout(() => {
    const value = typeof text === "function" ? text() : text;
    if (!value) return;
    currentTarget = target;
    currentText = value;
    const tip = ensureTip();
    tip.textContent = value;
    tip.classList.add("visible");
    reposition();
  }, SHOW_DELAY);
}

function hide() {
  clearTimeout(showTimer);
  hideTimer = setTimeout(() => {
    tipEl?.classList.remove("visible");
    currentTarget = null;
  }, HIDE_DELAY);
}

/** 全局记录鼠标位置（capture）。 */
window.addEventListener(
  "mousemove",
  (e) => {
    mouseX = e.clientX;
    mouseY = e.clientY;
    if (tipEl?.classList.contains("visible")) reposition();
  },
  true,
);

/** scroll/resize 全局跟随（capture 覆盖所有滚动容器）。 */
window.addEventListener(
  "scroll",
  () => {
    if (currentTarget && tipEl?.classList.contains("visible")) reposition();
  },
  true,
);
window.addEventListener("resize", () => {
  if (currentTarget && tipEl?.classList.contains("visible")) reposition();
});

/** 已显式绑定 v-tooltip 的元素：其子树交给指令，自动检测跳过（避免双重气泡）。 */
const explicitTargets = new WeakSet<HTMLElement>();

const NON_TEXT_TAGS: Record<string, true> = {
  TEXTAREA: true,
  SELECT: true,
  OPTION: true,
};

/** 输入框只在显式写了 text-overflow: ellipsis 时参与；密码/控件类不读值。 */
const SKIP_INPUT_TYPES: Record<string, true> = {
  password: true,
  checkbox: true,
  radio: true,
  color: true,
  range: true,
  file: true,
  hidden: true,
};

function autoText(el: HTMLElement): string {
  const raw = el instanceof HTMLInputElement ? el.value : (el.innerText || el.textContent || "");
  return raw.replace(/\s+/g, " ").trim();
}

/** 从事件目标向上找第一个「单行省略且确实被裁剪」的元素；遇到显式绑定 / 原生 title 即放弃。 */
function autoTipTarget(start: EventTarget | null): HTMLElement | null {
  let el = start instanceof Element ? (start as HTMLElement) : null;
  for (let depth = 0; el && depth < 8; depth += 1, el = el.parentElement) {
    if (el === document.body || el.classList.contains(TIP_CLASS)) return null;
    if (explicitTargets.has(el) || el.hasAttribute("title")) return null;
    if (NON_TEXT_TAGS[el.tagName]) return null;
    if (el instanceof HTMLInputElement && SKIP_INPUT_TYPES[el.type]) return null;
    const style = getComputedStyle(el);
    if (style.textOverflow !== "ellipsis") continue;
    if (style.overflowX === "visible" && style.overflowY === "visible") continue;
    // 单行按宽度判断；多行仅认 -webkit-line-clamp 的高度裁剪
    if (el.scrollWidth > el.clientWidth) return el;
    const lineClamp = style.getPropertyValue("-webkit-line-clamp");
    if (lineClamp && lineClamp !== "none" && el.scrollHeight > el.clientHeight) return el;
  }
  return null;
}

let autoTarget: HTMLElement | null = null;

window.addEventListener(
  "mouseover",
  (e) => {
    if (!(e.target instanceof Element)) return;
    const target = autoTipTarget(e.target);
    if (!target) {
      if (autoTarget) {
        autoTarget = null;
        hide();
      }
      return;
    }
    if (target === autoTarget) return;
    autoTarget = target;
    show(target, () => autoText(target));
  },
  true,
);

window.addEventListener(
  "mouseout",
  (e) => {
    if (!autoTarget) return;
    if (e.relatedTarget instanceof Node && autoTarget.contains(e.relatedTarget)) return;
    autoTarget = null;
    hide();
  },
  true,
);

const tooltip: Directive<HTMLElement, string> = {
  mounted(el: TooltipTarget, binding) {
    el.removeAttribute("title");
    el.__ttText = binding.value;
    explicitTargets.add(el);
    const onEnter = () => show(el, el.__ttText ?? "");
    const onLeave = () => hide();
    el.addEventListener("mouseenter", onEnter);
    el.addEventListener("mouseleave", onLeave);
    el.__ttCleanup = () => {
      el.removeEventListener("mouseenter", onEnter);
      el.removeEventListener("mouseleave", onLeave);
    };
  },
  updated(el: TooltipTarget, binding) {
    el.__ttText = binding.value;
    if (currentTarget === el && binding.value !== currentText) {
      const tip = ensureTip();
      tip.textContent = binding.value;
      currentText = binding.value;
      reposition();
    }
  },
  unmounted(el: TooltipTarget) {
    el.__ttCleanup?.();
    explicitTargets.delete(el);
    if (currentTarget === el) {
      tipEl?.classList.remove("visible");
      currentTarget = null;
    }
  },
};

export default tooltip;
