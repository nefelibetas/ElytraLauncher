// 托盘 IPC：是否可用 + 多语言菜单文案同步。
import { invoke } from "@tauri-apps/api/core";

export function hasTray(): Promise<boolean> {
  return invoke<boolean>("has_tray");
}

export function setTrayLabels(show: string, quit: string): Promise<void> {
  return invoke("set_tray_labels", { show, quit });
}
