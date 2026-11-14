// 全局错误弹窗状态（AlertDialog）：启动/登录/皮肤/配置恢复共用，跨 store 引用。
import { reactive } from "vue";
import { t } from "../../i18n";

/** 弹窗上的直接操作（如「下载 Java 21」「提交修复」）；执行前弹窗会先关闭。 */
export interface AlertAction {
  label: string;
  primary?: boolean;
  run: () => void | Promise<void>;
}

export interface AlertOptions {
  /** 是否显示「复制错误信息」按钮（仅复杂/意外错误需要） */
  copyable?: boolean;
  /** 弹窗标题；缺省按启动失败处理 */
  title?: string;
  actions?: AlertAction[];
}

export const alertState = reactive({
  show: false,
  msg: "",
  copyable: false,
  title: t("launch.alert.launchFailed"),
  actions: [] as AlertAction[],
});

export function showError(msg: string, opts: boolean | AlertOptions = false) {
  const options: AlertOptions = typeof opts === "boolean" ? { copyable: opts } : opts;
  alertState.msg = msg;
  alertState.copyable = options.copyable ?? false;
  alertState.title = options.title ?? t("launch.alert.launchFailed");
  alertState.actions = options.actions ?? [];
  alertState.show = true;
}
