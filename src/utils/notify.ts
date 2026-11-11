// 系统通知：仅在窗口失焦/最小化时发送，避免用户正在操作时重复打断。
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

let granted: boolean | null = null;
let lastSentAt = 0;

/** 返回是否实际发送（权限拒绝/窗口聚焦时 false）。 */
export async function notifyIfUnfocused(title: string, body: string): Promise<boolean> {
  if (typeof document !== "undefined" && document.hasFocus()) return false;
  const text = body.trim();
  if (!text) return false;
  try {
    if (granted === null) granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (!granted) return false;
    // 多个任务同时完成时避免连续弹多条；间隔内的后续通知交给任务列表。
    const now = Date.now();
    if (now - lastSentAt < 2000) return false;
    lastSentAt = now;
    sendNotification({ title, body: text });
    return true;
  } catch {
    return false;
  }
}
