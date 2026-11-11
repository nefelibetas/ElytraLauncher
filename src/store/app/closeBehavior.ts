// 关闭窗口行为：询问 / 最小化到托盘 / 直接退出；持久化到 localStorage。
// 托盘不可用（如 Linux 缺少 appindicator）时降级为询问/退出，避免隐藏后无法唤回。
import { ref } from "vue";
import { t } from "../../i18n";
import { hasTray, setTrayLabels } from "../../api/tray";

export type CloseAction = "ask" | "tray" | "exit";

const STORAGE_KEY = "elytra.closeAction";

function readAction(): CloseAction {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    if (value === "tray" || value === "exit" || value === "ask") return value;
  } catch {
    // 存储不可用时按询问处理
  }
  return "ask";
}

export const closeAction = ref<CloseAction>(readAction());
export const trayAvailable = ref(false);

export function setCloseAction(action: CloseAction) {
  closeAction.value = action;
  try {
    localStorage.setItem(STORAGE_KEY, action);
  } catch {
    // 仅本会话生效
  }
}

/** 窗口创建后执行一次：探测托盘能力，必要时把“托盘”偏好回退为询问。 */
export async function initCloseBehavior() {
  try {
    trayAvailable.value = await hasTray();
  } catch {
    trayAvailable.value = false;
  }
  if (!trayAvailable.value && closeAction.value === "tray") {
    setCloseAction("ask");
  }
}

/** 语言切换后同步托盘菜单文案。 */
export async function syncTrayLabels() {
  if (!trayAvailable.value) return;
  try {
    await setTrayLabels(t("common.tray.show"), t("common.tray.quit"));
  } catch {
    // 托盘被系统移除（如 Explorer 重启）：保留原菜单，不影响关闭流程
  }
}
