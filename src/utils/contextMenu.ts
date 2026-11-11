// 原生文本右键菜单：WebView 自带的 Chromium 菜单太"网页"，改用 Tauri 系统菜单。
// 仅用于输入框/可选中文本；非文本区域仍由 App.vue 屏蔽浏览器菜单。
import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";

function isEditable(el: Element | null): boolean {
  if (!el) return false;
  return (
    el instanceof HTMLInputElement ||
    el instanceof HTMLTextAreaElement ||
    !!el.closest("[contenteditable='true']")
  );
}

/** 在鼠标位置弹出原生菜单；没有可操作项时返回 false。 */
export async function showNativeTextMenu(): Promise<boolean> {
  const active = document.activeElement;
  const editable = isEditable(active);
  const hasSelection = (window.getSelection()?.toString().length ?? 0) > 0;
  if (!editable && !hasSelection) return false;

  const items = [];
  if (editable) {
    items.push(
      await PredefinedMenuItem.new({ item: "Undo" }),
      await PredefinedMenuItem.new({ item: "Redo" }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "Cut" }),
      await PredefinedMenuItem.new({ item: "Copy" }),
      await PredefinedMenuItem.new({ item: "Paste" }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await PredefinedMenuItem.new({ item: "SelectAll" }),
    );
  } else {
    items.push(
      await PredefinedMenuItem.new({ item: "Copy" }),
    );
  }
  const menu = await Menu.new({ items });
  await menu.popup();
  return true;
}

export interface NativeActionItem {
  id: string;
  label: string;
  disabled?: boolean;
  separator?: boolean;
}

/** 行级原生上下文菜单：菜单项由调用方给文案，选中后回调 id。 */
export async function showNativeActionMenu(
  items: NativeActionItem[],
  onSelect: (id: string) => void,
): Promise<void> {
  const menuItems: (MenuItem | PredefinedMenuItem)[] = [];
  for (const item of items) {
    if (item.separator) {
      menuItems.push(await PredefinedMenuItem.new({ item: "Separator" }));
      continue;
    }
    menuItems.push(
      await MenuItem.new({
        id: item.id,
        text: item.label,
        enabled: !item.disabled,
        action: (id) => onSelect(id),
      }),
    );
  }
  const menu = await Menu.new({ items: menuItems });
  await menu.popup();
}
