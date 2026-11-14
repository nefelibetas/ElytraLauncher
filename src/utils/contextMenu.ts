// 应用内右键菜单：统一主题样式与 i18n，替代系统原生菜单（后者风格与应用割裂）。
// 文本菜单仅用于输入框/可选中文本；行级操作菜单由调用方传入文案与回调。
import { t } from "../i18n";
import { readClipboardText, writeClipboardText } from "./clipboard";

export interface ActionMenuItem {
  id: string;
  label: string;
  disabled?: boolean;
  separator?: boolean;
}

type MenuAction = (id: string) => void | Promise<void>;

let menuEl: HTMLElement | null = null;
let cleanup: (() => void) | null = null;
let lastPoint = { x: 0, y: 0 };

// 捕获阶段记录坐标：调用方的 @contextmenu.prevent.stop 会阻断冒泡，但仍经过捕获。
if (typeof window !== "undefined") {
  window.addEventListener(
    "contextmenu",
    (event) => {
      lastPoint = { x: event.clientX, y: event.clientY };
    },
    true,
  );
}

function isEditable(el: Element | null): el is HTMLElement {
  if (!el) return false;
  return (
    el instanceof HTMLInputElement ||
    el instanceof HTMLTextAreaElement ||
    !!el.closest("[contenteditable='true']")
  );
}

/** 当前输入框/文本框的选中文本；contenteditable 走全局选区。 */
function selectedTextOf(el: HTMLElement): string {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const start = el.selectionStart ?? 0;
    const end = el.selectionEnd ?? 0;
    return start === end ? "" : el.value.slice(start, end);
  }
  return window.getSelection()?.toString() ?? "";
}

/** 用文本替换当前选区（input/textarea 直接改值并派发 input，contenteditable 走编辑命令）。 */
function replaceSelection(el: HTMLElement, text: string) {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    // number/date 等类型没有选区 API（selectionStart 为 null），直接覆盖值。
    if (el.selectionStart === null) {
      el.value = text;
    } else {
      el.setRangeText(text, el.selectionStart, el.selectionEnd ?? el.selectionStart, "end");
    }
    el.dispatchEvent(new Event("input", { bubbles: true }));
    return;
  }
  document.execCommand("insertText", false, text);
}

function closeMenu() {
  cleanup?.();
  cleanup = null;
  menuEl?.remove();
  menuEl = null;
}

/** 在鼠标位置打开应用内菜单；重复调用会先关闭旧菜单。 */
function openMenu(items: ActionMenuItem[], onSelect: MenuAction, x: number, y: number) {
  closeMenu();
  const menu = document.createElement("div");
  menu.className = "app-context-menu";
  menu.setAttribute("role", "menu");
  menu.tabIndex = -1;

  for (const item of items) {
    if (item.separator) {
      const sep = document.createElement("div");
      sep.className = "app-context-menu-sep";
      sep.setAttribute("role", "separator");
      menu.appendChild(sep);
      continue;
    }
    const row = document.createElement("div");
    row.className = "app-context-menu-item";
    row.classList.toggle("is-disabled", !!item.disabled);
    row.setAttribute("role", "menuitem");
    row.tabIndex = item.disabled ? -1 : 0;
    row.dataset.id = item.id;
    row.textContent = item.label;
    if (!item.disabled) {
      row.addEventListener("click", () => {
        closeMenu();
        void onSelect(item.id);
      });
    }
    menu.appendChild(row);
  }

  // 先按鼠标位置落位，再按实际尺寸夹回视口内，避免贴边时溢出。
  menu.style.left = `${x}px`;
  menu.style.top = `${y}px`;
  document.body.appendChild(menu);
  menuEl = menu;
  const rect = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - rect.width - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - rect.height - 8))}px`;

  const focusables = () => Array.from(menu.querySelectorAll<HTMLElement>(".app-context-menu-item:not(.is-disabled)"));
  focusables()[0]?.focus({ preventScroll: true });

  const onPointerDown = (event: PointerEvent) => {
    if (!menu.contains(event.target as Node)) closeMenu();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopImmediatePropagation();
      closeMenu();
      return;
    }
    if (event.key === "Tab") {
      closeMenu();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp" && event.key !== "Home" && event.key !== "End") {
      if (event.key === "Enter" || event.key === " ") {
        const active = document.activeElement;
        if (active instanceof HTMLElement && menu.contains(active)) {
          event.preventDefault();
          active.click();
        }
      }
      return;
    }
    event.preventDefault();
    const list = focusables();
    if (!list.length) return;
    const index = list.indexOf(document.activeElement as HTMLElement);
    let next: number;
    if (event.key === "Home") next = 0;
    else if (event.key === "End") next = list.length - 1;
    else if (event.key === "ArrowDown") next = index < 0 ? 0 : (index + 1) % list.length;
    else next = index <= 0 ? list.length - 1 : index - 1;
    list[next]?.focus({ preventScroll: true });
  };
  const onBlur = () => closeMenu();

  // 延迟一帧再挂外部关闭，避免本次 contextmenu 的后续事件立刻关掉新菜单。
  window.setTimeout(() => {
    if (menuEl !== menu) return;
    window.addEventListener("pointerdown", onPointerDown, true);
  }, 0);
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("blur", onBlur);
  cleanup = () => {
    window.removeEventListener("pointerdown", onPointerDown, true);
    window.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("blur", onBlur);
  };
}

/** 文本右键菜单：撤销/重做/剪切/复制/粘贴/全选；没有可操作项时返回 false。 */
export function showTextMenu(): boolean {
  const active = document.activeElement;
  const editable = isEditable(active);
  const target = active instanceof HTMLElement ? active : null;
  const selectedText = target && editable ? selectedTextOf(target) : window.getSelection()?.toString() ?? "";
  if (!editable && !selectedText) return false;
  const hasSelection = selectedText.length > 0;

  const items: ActionMenuItem[] = editable
    ? [
        { id: "undo", label: t("common.contextMenu.undo") },
        { id: "redo", label: t("common.contextMenu.redo") },
        { id: "sep-1", label: "", separator: true },
        { id: "cut", label: t("common.contextMenu.cut"), disabled: !hasSelection },
        { id: "copy", label: t("common.contextMenu.copy"), disabled: !hasSelection },
        { id: "paste", label: t("common.contextMenu.paste") },
        { id: "sep-2", label: "", separator: true },
        { id: "selectAll", label: t("common.contextMenu.selectAll") },
      ]
    : [{ id: "copy", label: t("common.contextMenu.copy") }];

  openMenu(items, async (id) => {
    if (!editable) {
      if (id !== "copy" || !selectedText) return;
      try {
        await writeClipboardText(selectedText);
      } catch {
        document.execCommand("copy");
      }
      return;
    }
    if (!target) return;
    target.focus({ preventScroll: true });

    if (id === "paste") {
      try {
        replaceSelection(target, await readClipboardText());
      } catch {
        // 读取失败（权限/空剪贴板）不弹浏览器菜单，保持静默。
      }
      return;
    }

    if (id === "copy" || id === "cut") {
      if (!hasSelection) return;
      const text = selectedTextOf(target);
      if (!text) return;
      try {
        await writeClipboardText(text);
      } catch {
        document.execCommand(id);
        return;
      }
      if (id === "cut") replaceSelection(target, "");
      return;
    }

    if (id === "selectAll") {
      if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
        try {
          target.select();
        } catch {
          // number/date 等类型不支持 select()，忽略。
        }
      } else {
        document.execCommand("selectAll");
      }
      return;
    }

    // 撤销/重做仍交给 WebView 的编辑命令（input/textarea 与 contenteditable 共用）。
    if (id === "undo" || id === "redo") document.execCommand(id);
  }, lastPoint.x, lastPoint.y);
  return true;
}

/** 行级操作菜单：菜单项由调用方给文案，选中后回调 id。 */
export function showActionMenu(items: ActionMenuItem[], onSelect: MenuAction) {
  openMenu(items, onSelect, lastPoint.x, lastPoint.y);
}
