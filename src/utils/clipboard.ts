import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { t } from "../i18n";
import { showToast } from "./toast";

/** 读剪贴板：优先走 Tauri 原生插件，避免 WebView2 弹出网页级剪贴板授权提示。 */
export async function readClipboardText(): Promise<string> {
  try {
    return await readText();
  } catch {
    return navigator.clipboard.readText();
  }
}

/** 写剪贴板：同上；浏览器调试环境回退到 Web API。 */
export async function writeClipboardText(text: string): Promise<void> {
  try {
    await writeText(text);
  } catch {
    await navigator.clipboard.writeText(text);
  }
}

/** okMsg 为成功提示；失败统一提示手动复制。 */
export async function copyText(text: string, okMsg = t("common.action.copied")) {
  try {
    await writeClipboardText(text);
    showToast(okMsg);
  } catch {
    showToast(t("common.action.copyFailedManual"));
  }
}
