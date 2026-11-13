<script setup lang="ts">
import { onMounted, onUnmounted, nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { X } from "@lucide/vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { initNavHistory, goBack, goForward } from "./store/app/nav";
import { prefetchTabPages } from "./router";
import TitleBar from "./component/common/TitleBar.vue";
import AlertDialog from "./component/common/AlertDialog.vue";
import CheckToggle from "./component/common/CheckToggle.vue";
import { dlState, ensureListen, waitingUser, decideWaitingTask, type TaskKind } from "./store/task/download";
import { ensureAutoTasksListen, lastAutoTasksFinished } from "./store/config/autoTasks";
import { setSelected } from "./store/app/selectedVersion";
import { toasts, dismissToast, pauseToast, resumeToast, showToast } from "./utils/toast";
import { moveFocus, clearFocusRing, focusDialog, restoreFocus } from "./utils/focusNav";
import { errMsg, formatError } from "./utils/error";
import { showTextMenu } from "./utils/contextMenu";
import { currentLocale } from "./i18n";
import { flashTaskbarAttention } from "./store/app/taskbar";
import { notifyIfUnfocused } from "./utils/notify";
import {
  closeAction, setCloseAction, trayAvailable, initCloseBehavior, syncTrayLabels,
} from "./store/app/closeBehavior";
import { ensureGameLanguageDetected } from "./store/config/launchOptions";

const router = useRouter();
const { t } = useI18n();
const appWindow = getCurrentWindow();

/** 窗口焦点：自绘标题栏在失焦时弱化，模拟系统非活动窗口。 */
const windowFocused = ref(true);

/** 关闭确认弹窗：托盘/退出二选一 + 不再询问。 */
const closePrompt = ref(false);
const dontAskClose = ref(false);
let unlistenClose: (() => void) | undefined;
let unlistenFocus: (() => void) | undefined;

watch(currentLocale, () => {
  void syncTrayLabels();
});


/** body 只放短提示，完整错误由复制按钮带走。 */
const errorDialog = ref(false);
const errorMessage = ref("");

/** 自动任务失败（pre_launch / on_crash / post_exit）：跨页面提示，避免用户错过。 */
const autoTaskFailDialog = ref(false);
const autoTaskFailMessage = ref("");
watch(lastAutoTasksFinished, (finished) => {
  if (!finished || !finished.failures.length) return;
  autoTaskFailMessage.value = finished.failures
    .map((f) => `[${f.action}] ${errMsg(f.message)}`)
    .join("\n");
  autoTaskFailDialog.value = true;
});

dlState.onTaskDone = (message: string, kind?: TaskKind, versionDir?: string) => {
  // JDK 任务完成处理由 jdkOptions 监听负责
  if (kind === "jdk") return;
  // 版本下载完成后自动选中该版本，使后续 Mod 下载目标为该版本
  if (versionDir) setSelected(versionDir);
  // 文案由 download.ts 统一格式化（资源名映射）
  showToast(message);
  void flashTaskbarAttention("info");
  void notifyIfUnfocused(
    kind === "export" ? t("common.notify.exportDone") : t("common.notify.downloadDone"),
    message,
  );
  // 完成不自动退出任务中心
};

dlState.onTaskError = (_taskName: string, error: string) => {
  errorMessage.value = error;
  errorDialog.value = true;
  void flashTaskbarAttention("critical");
  void notifyIfUnfocused(t("common.notify.downloadFailed"), errMsg(error));
};

/** 复制挂起任务错误全文（中文首行 + 技术细节）。 */
async function copyWaitingError() {
  const w = waitingUser.value;
  if (!w?.error) return;
  try {
    await navigator.clipboard.writeText(formatError(w.error).full);
    showToast(t("common.action.copied"));
  } catch {
    showToast(t("common.action.copyFailed"));
  }
}

/** 批量下载失败弹窗：出现时焦点移入（Tab 范围随之收窄到弹窗），关闭后归还原焦点。 */
const waitingOverlay = ref<HTMLElement | null>(null);
let waitingRestore: HTMLElement | null = null;
watch(waitingUser, async (w) => {
  if (!w) {
    restoreFocus(waitingRestore);
    waitingRestore = null;
    return;
  }
  void flashTaskbarAttention("critical");
  void notifyIfUnfocused(t("common.notify.downloadFailed"), errMsg(w.error));
  await nextTick();
  if (waitingOverlay.value) waitingRestore = focusDialog(waitingOverlay.value);
});

/** 输入框 / 文本域 / contenteditable。 */
function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || !!el.closest("[contenteditable='true']");
}

/** 日志/代码块等只读文本：Ctrl(⌘)+A 与右键复制按原生文本处理。 */
function isTextTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  return isEditableTarget(target) || !!el.closest("pre, code, [data-selectable]");
}

function onKeyDown(e: KeyboardEvent) {
  // 输入法组合期间的 Esc/方向键属于候选框操作，不能触发全局导航。
  if (e.isComposing || e.keyCode === 229) return;
  const isMac = /Mac|iPhone|iPad/.test(navigator.platform) || /Mac OS X/.test(navigator.userAgent);
  const primary = isMac ? e.metaKey : e.ctrlKey;
  // 鼠标侧键可能被系统映射成 BrowserBack/Forward 或 Alt+←/→，不能依赖 WebView 原生历史。
  const browserBack = e.key === "BrowserBack" || e.keyCode === 166;
  const browserForward = e.key === "BrowserForward" || e.keyCode === 167;
  const altBack =
    e.altKey && !isEditableTarget(e.target) && e.key === "ArrowLeft";
  const altForward =
    e.altKey && !isEditableTarget(e.target) && e.key === "ArrowRight";
  const cmdBack = isMac && e.metaKey && e.key === "[";
  const cmdForward = isMac && e.metaKey && e.key === "]";
  if (browserBack || browserForward || altBack || altForward || cmdBack || cmdForward) {
    e.preventDefault();
    if (e.repeat) return;
    const now = Date.now();
    if (now - lastSideAt < 120) return; // 与鼠标事件共用去重窗口
    lastSideAt = now;
    if (browserBack || altBack || cmdBack) goBack();
    else goForward();
    return;
  }

  // 禁止打开开发者工具：F12 / Ctrl(⌘)+Shift+I/J/C / Ctrl(⌘)+U / macOS ⌘⌥I
  if (
    e.key === "F12" ||
    (primary && e.shiftKey && ["I", "J", "C"].includes(e.key.toUpperCase())) ||
    (isMac && e.metaKey && e.altKey && e.key.toUpperCase() === "I") ||
    (primary && e.key.toUpperCase() === "U")
  ) {
    e.preventDefault();
    return;
  }
  // 禁止刷新页面：F5 / Ctrl(⌘)+R（WebView 内刷新会丢失 Tauri IPC 状态）
  if (e.key === "F5" || (primary && e.key.toUpperCase() === "R")) {
    e.preventDefault();
    return;
  }
  // 输入上下文放行原生编辑快捷键（全选/复制/剪切/粘贴/撤销/重做）
  if (
    isEditableTarget(e.target) &&
    primary &&
    ["A", "C", "X", "V", "Z", "Y"].includes(e.key.toUpperCase())
  ) {
    return;
  }
  // Tab：转发给应用自管的焦点遍历（原生遍历绕过页面语义，直接改 DOM 焦点）
  if (e.key === "Tab") {
    e.preventDefault();
    moveFocus(e.shiftKey ? -1 : 1);
    return;
  }
  // 非文本区域禁止全选；日志/代码块保留原生 Ctrl(⌘)+A。
  if (primary && e.key.toLowerCase() === "a" && !isTextTarget(e.target)) {
    e.preventDefault();
    return;
  }
  // ESC：按应用导航栈返回上一级（设置子页 / 工具回设置，其余回启动页）
  if (e.key === "Escape" && !document.querySelector(".dialog-overlay")) {
    goBack();
  }
}

/** 鼠标侧键（后退/前进）驱动应用内导航栈（store/nav.ts）：WebView 侧键默认直接操作浏览器历史、绕过
 *  vue-router 自身栈导致页面状态不一致，故在捕获阶段拦截（button 3=后退 4=前进）并 preventDefault，
 *  改由自管栈 goBack/goForward 回放；同一按键经 mousedown 与 pointerdown 重复派发，用 120ms 时间窗去重。 */
let pinnedHash = "#/";
let lastSideAt = 0;

function onSideButton(e: MouseEvent) {
  if (e.button === 3 || e.button === 4) {
    e.preventDefault();
    e.stopPropagation();
    const now = Date.now();
    if (now - lastSideAt < 120) return; // mousedown/pointerdown 同键去重
    lastSideAt = now;
    if (e.button === 3) goBack();
    else goForward();
  }
}

function onPopState() {
  const cur = location.hash || "#/";
  if (pinnedHash && cur !== pinnedHash) {
    // 不新增历史条目地拉回 hash 并同步路由
    history.replaceState(null, "", pinnedHash);
    router.replace(pinnedHash.slice(1) || "/").catch(() => {});
  }
}

async function onContextMenu(e: MouseEvent) {
  const target = e.target as HTMLElement | null;
  if (!target) return;
  const selected = (window.getSelection()?.toString().length ?? 0) > 0;
  if (!isEditableTarget(target) && !selected) {
    // 非文本区域不弹浏览器菜单；行级操作菜单由各组件自行处理。
    e.preventDefault();
    return;
  }
  e.preventDefault();
  const editable = target.closest("input, textarea, [contenteditable='true']") as HTMLElement | null;
  editable?.focus();
  try {
    showTextMenu();
  } catch {
    // 菜单构建失败时静默降级，不弹 Chromium 网页菜单。
  }
}

async function minimizeToTray() {
  if (dontAskClose.value) setCloseAction("tray");
  closePrompt.value = false;
  await appWindow.hide();
}

async function exitApplication() {
  if (dontAskClose.value) setCloseAction("exit");
  closePrompt.value = false;
  // destroy 不再触发 close-requested，避免二次弹窗。
  await appWindow.destroy();
}

function cancelClosePrompt() {
  closePrompt.value = false;
}

/** 首帧后执行非关键启动任务；不支持 requestIdleCallback 的 WebView 回退到定时器。 */
function afterFirstPaint(task: () => void) {
  const idle = (window as Window & {
    requestIdleCallback?: (cb: () => void, options?: { timeout: number }) => number;
  }).requestIdleCallback;
  if (typeof idle === "function") idle(task, { timeout: 1500 });
  else window.setTimeout(task, 200);
}

onMounted(async () => {
  // 旧默认中文语言迁移到系统语言（设置页加载前完成，游戏启动即用新值）
  void ensureGameLanguageDetected();
  pinnedHash = location.hash || "#/";
  await initCloseBehavior();
  await syncTrayLabels();
  unlistenFocus = await appWindow.onFocusChanged(({ payload }) => {
    windowFocused.value = payload;
  });
  unlistenClose = await appWindow.onCloseRequested(async (event) => {
    if (closePrompt.value) {
      event.preventDefault();
      return;
    }
    if (closeAction.value === "tray" && trayAvailable.value) {
      event.preventDefault();
      await appWindow.hide();
      return;
    }
    if (closeAction.value === "exit" || !trayAvailable.value) return;
    // 询问模式：先阻止关闭，由前端弹窗决定。
    event.preventDefault();
    dontAskClose.value = false;
    closePrompt.value = true;
  });
  router.afterEach(() => {
    pinnedHash = location.hash || "#/";
  });
  initNavHistory();
  document.addEventListener("contextmenu", onContextMenu);
  document.addEventListener("keydown", onKeyDown);
  // 捕获阶段拦截（WebView2 可能不派发冒泡阶段的侧键事件）
  document.addEventListener("mousedown", onSideButton, true);
  document.addEventListener("mouseup", onSideButton, true);
  document.addEventListener("pointerdown", onSideButton, true);
  document.addEventListener("pointerup", onSideButton, true);
  // 部分 WebView2 版本把 XBUTTON1/2 只投递为 auxclick。
  document.addEventListener("auxclick", onSideButton, true);
  // 鼠标交互后清掉键盘焦点环（键盘遍历打的标记）——用冒泡段，不与侧键捕获冲突
  document.addEventListener("pointerdown", clearFocusRing);
  window.addEventListener("popstate", onPopState);
  ensureListen();
  // 非首屏任务延后到空闲期：配置恢复、自动任务事件、版本目录监听。
  afterFirstPaint(() => {
    void import("./store/config/configRecovery")
      .then((m) => m.ensureConfigRecoveryWatch())
      .catch(() => {});
    ensureAutoTasksListen();
    void import("./store/config/gameDirs")
      .then((m) => m.ensureVersionsWatch())
      .catch(() => {});
    // 顶级 tab 页面在首帧后预取：保持启动包精简，点击设置/资源中心/任务中心时不再等 chunk。
    prefetchTabPages();
  });
  // 崩溃恢复横幅与内存监控分别由 DownloadDetail / 设置页按需启动。
});

onUnmounted(() => {
  unlistenClose?.();
  unlistenFocus?.();
  document.removeEventListener("contextmenu", onContextMenu);
  document.removeEventListener("keydown", onKeyDown);
  document.removeEventListener("mousedown", onSideButton, true);
  document.removeEventListener("mouseup", onSideButton, true);
  document.removeEventListener("pointerdown", onSideButton, true);
  document.removeEventListener("pointerup", onSideButton, true);
  document.removeEventListener("auxclick", onSideButton, true);
  document.removeEventListener("pointerdown", clearFocusRing);
  window.removeEventListener("popstate", onPopState);
});

</script>

<template>
  <!-- 主窗口：TitleBar + 路由动画 + 全局弹层 -->
  <div class="app-layout" :class="{ 'app-inactive': !windowFocused }">
    <TitleBar />
    <main class="main-content">
      <router-view v-slot="{ Component, route }">
        <Transition
          :name="route.meta.transition ?? 'route'"
          :mode="route.meta.transition?.startsWith('slide-') ? undefined : 'out-in'"
        >
          <component :is="Component" />
        </Transition>
      </router-view>
    </main>
    <!-- 提示堆叠：窗口右上角（标题栏下方留出间距）；进入自右向左、离开从左到右，hover 暂停自动关闭。
         读屏经 role=status 播报；data-tabskip 让键盘遍历跳过这条通道（自动消失，入列会打断遍历顺序） -->
    <TransitionGroup name="toast" tag="div" class="toast-stack" role="status" aria-live="polite" data-tabskip>
      <div
        v-for="toast in toasts"
        :key="toast.id"
        class="toast-item"
        @mouseenter="pauseToast(toast.id)"
        @mouseleave="resumeToast(toast.id)"
      >
        <span class="toast-msg">{{ toast.msg }}</span>
        <button class="toast-close" :title="t('common.tooltip.close')" :aria-label="t('common.tooltip.close')" @click="dismissToast(toast.id)">
          <X :size="12" :stroke-width="2.4" />
        </button>
      </div>
    </TransitionGroup>
    <AlertDialog v-model="errorDialog" :title="t('task.dialog.downloadFailed')" :message="errorMessage" copyable>
      <p>{{ t("task.dialog.downloadFailedBody") }}</p>
    </AlertDialog>

    <AlertDialog v-model="autoTaskFailDialog" :title="t('task.dialog.autoTaskFailed')" :message="autoTaskFailMessage" copyable>
      <p>{{ t("task.dialog.autoTaskFailedBody") }}</p>
    </AlertDialog>

    <!-- 关闭窗口：退出 / 最小化到托盘；勾选后记住选择 -->
    <AlertDialog v-model="closePrompt" :title="t('common.closePrompt.title')">
      <div class="close-prompt-body">
        <p>{{ t("common.closePrompt.message") }}</p>
        <CheckToggle v-model="dontAskClose">{{ t("common.closePrompt.dontAsk") }}</CheckToggle>
      </div>
      <template #actions>
        <button class="dialog-btn" @click="cancelClosePrompt">{{ t("common.closePrompt.cancel") }}</button>
        <button v-if="trayAvailable" class="dialog-btn" @click="minimizeToTray">{{ t("common.closePrompt.tray") }}</button>
        <button class="dialog-btn primary" @click="exitApplication">{{ t("common.closePrompt.exit") }}</button>
      </template>
    </AlertDialog>

    <!-- 批量文件下载失败挂起：用户决策 跳过/重试/取消（WaitingForUser） -->
    <Transition name="fade">
      <div
        v-if="waitingUser"
        ref="waitingOverlay"
        class="dialog-overlay"
        role="dialog"
        aria-modal="true"
        :aria-label="t('task.waiting.title')"
        @click.self.stop
      >
        <div class="waiting-dialog">
          <h3 class="wd-title">{{ t("task.waiting.title") }}</h3>
          <p class="wd-file">{{ waitingUser.file_name }}</p>
          <p class="wd-err">{{ errMsg(waitingUser.error) }}</p>
          <div class="wd-actions">
            <button class="wd-btn wd-retry" @click="decideWaitingTask('retry')">{{ t("common.action.retry") }}</button>
            <button class="wd-btn" @click="decideWaitingTask('skip')">{{ t("common.action.skip") }}</button>
            <button class="wd-btn wd-cancel" @click="decideWaitingTask('cancel')">{{ t("task.waiting.cancelTask") }}</button>
            <button class="wd-btn wd-copy" @click="copyWaitingError">{{ t("common.action.copyError") }}</button>
          </div>
        </div>
      </div>
    </Transition>
    <!-- 窗口边缘加深：无边框窗口悬浮于桌面时边界难辨。纯视觉覆盖层（描边 + 内阴影），不拦截交互；
         z 2000 位于内容/弹窗之上、toast(3000)/tooltip(3100) 之下 -->
    <div class="edge-shade" aria-hidden="true"></div>
  </div>
</template>

<style>
html, body {
  margin: 0;
  padding: 0;
  overflow: hidden;
  overscroll-behavior-x: none;
  background: transparent;
  height: 100%;
  width: 100%;
}

/* 指针点击聚焦不显示默认焦点框；点过按钮再按键时 Chrome 会重新判定 :focus-visible 并画出 2px 品牌色轮廓
   （圆角+缩放偏移下会被看成按钮外一圈异色杂边），故键盘焦点环不写在这里：由 utils/focusNav.ts 以内联内描边写入
   （组件内 scoped `outline: none` 特异性高于全局单类规则，样式表写法会被盖掉）。 */
:focus { outline: none; }

/* 输入控件保持可选中/可编辑 */
input, textarea {
  -webkit-user-select: text;
  user-select: text;
}

/* 组件级自定义可覆盖 */
::placeholder {
  color: var(--text-tertiary);
  opacity: 1;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

/* 语言切换期间禁用过渡/动画：文本重排不应被 hover、主题或布局过渡放大成抖动 */
html.i18n-switching *,
html.i18n-switching *::before,
html.i18n-switching *::after {
  transition-duration: 0s !important;
  animation-duration: 0s !important;
}

/* 主题切换瞬间给全局颜色属性统一过渡，避免 CSS 变量整体跳变；只作用于切换窗口期。 */
html.theme-switching:not(.i18n-switching) *,
html.theme-switching:not(.i18n-switching) *::before,
html.theme-switching:not(.i18n-switching) *::after {
  transition:
    background-color 0.45s cubic-bezier(0.4, 0, 0.2, 1),
    border-color 0.45s cubic-bezier(0.4, 0, 0.2, 1),
    color 0.45s cubic-bezier(0.4, 0, 0.2, 1),
    fill 0.45s cubic-bezier(0.4, 0, 0.2, 1),
    stroke 0.45s cubic-bezier(0.4, 0, 0.2, 1),
    box-shadow 0.45s cubic-bezier(0.4, 0, 0.2, 1) !important;
}

@media (prefers-reduced-motion: reduce) {
  html.theme-switching *,
  html.theme-switching *::before,
  html.theme-switching *::after {
    transition: none !important;
  }
}

::selection {
  background: var(--color-brand-bg);
  color: var(--text-primary);
}

/* 窗口失焦：弱化自绘标题栏，模拟系统非活动窗口 */
.app-inactive .titlebar {
  filter: saturate(0.72) brightness(0.92);
}

.app-inactive .win {
  opacity: 0.62;
}

.app-inactive .win:hover {
  opacity: 1;
}

.close-prompt-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.close-prompt-body p {
  margin: 0;
  font-size: var(--fs-base);
  line-height: 1.6;
  color: var(--text-secondary);
}

.app-layout {
  display: flex;
  flex-direction: column;
  height: 100vh;
  min-width: 0;
  overflow: hidden;
  position: relative;
  border-radius: var(--radius-md);
  background-color: var(--surface-page);
  color: var(--text-primary);
}

/* 最大化时去掉自绘圆角/边缘描边，避免屏幕四角露出桌面 */
html[data-maximized="true"] .app-layout,
html[data-maximized="true"] .edge-shade {
  border-radius: 0;
}

html[data-maximized="true"] .edge-shade {
  box-shadow: none;
}

.main-content {
  position: relative;
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
  background-color: var(--surface-page);
}

/* 色相随主题切换；inset box-shadow 三端 WebView 均原生支持，无需 @supports 降级 */
.edge-shade {
  position: absolute;
  inset: 0;
  border-radius: var(--radius-md);
  pointer-events: none;
  z-index: 2000;
  box-shadow:
    inset 0 0 0 1px var(--edge-line),
    inset 0 0 24px var(--edge-shade);
}

/* 提示堆叠：右上角、标题栏下方留白；同屏最多 5 条（store 侧排队补位）。
   用块级排版而非 flex：离开项 position:absolute 需要静态位置保持原位，flex 容器会把绝对定位子项拉回主起点而跳位。 */
.toast-stack {
  position: fixed;
  top: calc(var(--titlebar-h) + 12px);
  right: 12px;
  z-index: var(--z-toast);
  width: 320px;
  pointer-events: none; /* 仅条目本身接管事件，不遮挡下层内容 */
}

.toast-item {
  pointer-events: auto;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-bottom: 8px;
  padding: 9px 8px 9px 13px;
  border: 1px solid var(--border-default);
  border-left: 3px solid var(--color-brand);
  border-radius: var(--radius-md);
  /* 左深右浅：左侧品牌色浓，向右渐浅（底色不透明，故整体始终可见） */
  background-color: var(--surface-card);
  background-image: linear-gradient(90deg, var(--color-brand-bg), rgba(255, 255, 255, 0));
  color: var(--text-primary);
  font-size: var(--fs-base);
  line-height: 1.45;
  box-shadow: var(--shadow-lg);
}

.toast-msg {
  flex: 1;
  min-width: 0;
  padding-top: 1px;
  word-break: break-word;
}

.toast-close {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: color var(--dur-fast), background-color var(--dur-fast);
}

.toast-close:hover {
  color: var(--text-primary);
  background-color: var(--surface-hover);
}

/* 进入自右向左、离开从左到右（同侧进出）；离开项脱离文档流，其余条目随之带动画上移 */
.toast-enter-active,
.toast-leave-active,
.toast-move {
  transition: transform 0.28s cubic-bezier(0.22, 0.61, 0.36, 1), opacity 0.24s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(110%);
}

.toast-leave-active {
  position: absolute;
  left: 0;
  right: 0;
}

/* 文件下载失败等待决策弹窗（WaitingForUser） */
.dialog-overlay {
  position: fixed; inset: 0; z-index: var(--z-modal);
  display: flex; align-items: center; justify-content: center;
  background: var(--overlay);
}

.waiting-dialog {
  width: 480px; max-width: calc(100vw - 48px);
  padding: 22px 24px;
  background: var(--surface-card);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.35);
}

.wd-title { margin: 0 0 10px; font-size: var(--fs-xl); color: var(--text-primary); }

.wd-file {
  margin: 0 0 6px; font-size: var(--fs-base); font-weight: 600;
  color: var(--text-primary); word-break: break-all;
}

.wd-err {
  margin: 0 0 18px; font-size: var(--fs-base); color: var(--text-tertiary);
  word-break: break-all; max-height: 140px; overflow-y: auto;
  line-height: 1.6;
}

.wd-actions { display: flex; gap: 8px; justify-content: flex-start; }

.wd-btn {
  padding: 6px 14px; font-size: var(--fs-base); cursor: pointer;
  border: 1px solid var(--border-strong); border-radius: var(--radius-sm);
  background: var(--surface-page); color: var(--text-primary);
  transition: border-color var(--dur-fast), color var(--dur-fast), background var(--dur-fast);
}

.wd-retry { background: var(--color-brand); border-color: var(--color-brand); color: var(--text-inverse); }
.wd-retry:hover { opacity: 0.9; }
.wd-btn:hover { border-color: var(--color-brand); }
.wd-cancel { color: var(--color-danger); }
.wd-cancel:hover { border-color: var(--color-danger); }
/* AlertDialog #actions slot 全局共享：scoped CSS 无法穿透 slot 内容，故收敛到全局 */
.dialog-btn {
  padding: 6px 14px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-primary);
  font-size: var(--fs-base);
  cursor: pointer;
  transition: all var(--dur-fast);
}

.dialog-btn:hover {
  border-color: var(--color-brand);
  color: var(--color-brand);
}

.dialog-btn.primary {
  background: var(--color-brand);
  color: var(--text-inverse);
}

.dialog-btn.primary:hover {
  opacity: 0.88;
}

.dialog-btn.danger {
  background: var(--color-danger);
  color: var(--text-inverse);
}

.dialog-btn.danger:hover {
  opacity: 0.88;
}

.fade-enter-active, .fade-leave-active { transition: opacity var(--dur-fast) ease; }
.fade-enter-from, .fade-leave-to { opacity: 0; }

/* v-tooltip 气泡（替换 UA 原生 title tooltip） */
.elytra-tooltip {
  position: fixed;
  z-index: var(--z-tooltip);
  max-width: 320px;
  padding: 6px 10px;
  color: var(--tooltip-fg);
  font-size: var(--fs-sm);
  line-height: 1.5;
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-lg);
  background: var(--tooltip-bg);
  opacity: 0;
  visibility: hidden;
  pointer-events: none;
  transition: opacity 0.12s ease;
  word-break: break-word;
  white-space: pre-line;
}

.elytra-tooltip.visible {
  opacity: 1;
  visibility: visible;
}
</style>
