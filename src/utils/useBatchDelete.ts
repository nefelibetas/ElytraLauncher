// 多选删除共享流程：确认弹窗 → 逐个删除 → 成功/失败 toast → 刷新回调。
// 删除是逐项串行执行的：单项失败不影响其余项（与批量重命名的处理一致）。
import { ref } from "vue";
import { errMsg } from "./error";
import { showToast } from "./toast";

export interface BatchDeleteOptions {
  /** 删除单项；抛错时 toast 首行。 */
  remove: (name: string) => Promise<void>;
  /** 确认弹窗正文（单条带名字，多条带数量）。 */
  describe: (names: string[]) => string;
  /** 单项成功 toast。 */
  success: (name: string, names: string[]) => string;
  /** 全部尝试结束后的刷新动作（列表重载、清空选中）。 */
  after?: (names: string[]) => Promise<void> | void;
}

export function useBatchDelete(options: BatchDeleteOptions) {
  const dialog = ref(false);
  const message = ref("");
  const pending = ref<string[]>([]);

  function open(names: string[]) {
    if (names.length === 0) return;
    message.value = options.describe(names);
    pending.value = [...names];
    dialog.value = true;
  }

  async function confirm() {
    const names = [...pending.value];
    dialog.value = false;
    pending.value = [];
    for (const name of names) {
      try {
        await options.remove(name);
        showToast(options.success(name, names));
      } catch (e) {
        showToast(errMsg(e));
      }
    }
    if (names.length > 0) await options.after?.(names);
  }

  return { dialog, message, open, confirm };
}
