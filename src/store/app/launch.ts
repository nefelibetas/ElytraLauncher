// 启动流程 store：启动阶段/步骤、版本加载、游戏退出监听；页面跳转参数化（navigateToDownload），不依赖 router。
import { reactive, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { StepStatus } from "../task/download";
import type { InstalledVersionInfo } from "../config/gameDirs";
import type { VersionState } from "../../types/game/VersionState";
import { GAME_EXITED, GAME_WINDOW_VISIBLE } from "../app/events";
import { authMode, msAccount, username, externalAccount, refreshExternalAccount, startMsLogin, ensureMsAccount } from "../auth/auth";
import { selectedVersion, setSelected, getSavedDir } from "../app/selectedVersion";
import { showError, type AlertAction } from "../app/alert";
import { t } from "../../i18n";
import { errMsg, errorKey, errorParams, formatError } from "../../utils/error";
import { showToast } from "../../utils/toast";
import { notifyIfUnfocused } from "../../utils/notify";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  applyJdk,
  installJdk,
  jdkList,
  loadConfig as loadJdkConfig,
  parseJdkMajor,
  refreshJdks,
  type JdkInfo,
} from "../config/jdkOptions";
import { repairVersionFiles } from "../task/download";
import { openGameFolder } from "../config/gameDirs";

export const launchState = reactive({
  /** 启动流程："" 空闲 | "running" 启动中 | "done" 完成 */
  phase: "" as "" | "running" | "done",
  /** key 为 i18n 消息 key：语言切换时步骤文案即时更新。 */
  steps: [] as { key: string; status: StepStatus }[],
});

export const isLaunching = computed(() => launchState.phase === "running");

/** 启动代际号：取消启动时自增，作废在飞流程的落点（doLaunch 每个 await 后校验）。 */
let launchSeq = 0;
/** 后端启动代际：取消/拉起进程必须携带，避免旧启动误取消/误登记新进程。 */
let launchGeneration = 0;

/** 已请求取消的启动标签（版本目录名）：被终止进程的 GAME_EXITED（非零退出码）不再按崩溃提示。 */
const cancelledLabels = new Set<string>();

function step(key: string, status: StepStatus) {
  const s = launchState.steps.find(s => s.key === key);
  if (s) s.status = status;
}

function resetLaunch() {
  launchState.phase = "";
  launchState.steps = [];
}

/** 取消本次启动：后端置取消标志（未拉起则阻止启动，已拉起则终止进程），前端立即收回面板。 */
export async function cancelLaunch() {
  if (!isLaunching.value) return;
  launchSeq++;
  if (selectedVersion.value) cancelledLabels.add(selectedVersion.value);
  resetLaunch();
  try {
    await invoke("cancel_launch", { generation: launchGeneration });
  } catch {
    // 取消失败（后端不可达）不阻断 UI：面板已收回，进程状态由后端事件兜底
  }
}

export async function loadVersions() {
  try {
    // 激活目录与版本列表一次取回：启动页少一次 IPC，也避免两次枚举之间目录被切换
    const state: VersionState = await invoke("get_version_state");
    const list: InstalledVersionInfo[] = state.versions;
    // 仅在所属目录仍是当前激活目录时才恢复（版本选择页同时写入两者）
    const savedDir = getSavedDir();
    if (savedDir && state.active_dir !== savedDir) selectedVersion.value = "";
    if (!list.some(v => v.name === selectedVersion.value) || list.length === 0) {
      try {
        const r = await invoke<{ dir_path: string; version: string }>(
          "auto_activate_version",
        );
        const fresh: VersionState = await invoke("get_version_state");
        selectedVersion.value = fresh.versions.some(v => v.name === r.version)
          ? r.version
          : fresh.versions[0]?.name ?? "";
        setSelected(selectedVersion.value, r.dir_path);
      } catch {
        selectedVersion.value = list.some(v => v.name === selectedVersion.value)
          ? selectedVersion.value
          : (list[0]?.name ?? "");
      }
    }
  } catch {
  }
}

/** 崩溃/超时弹窗操作：有崩溃报告时优先打开报告，否则打开版本目录。 */
function crashActions(version: string, report: string | null): AlertAction[] {
  const folder: AlertAction = {
    label: t("launch.alert.openVersionFolder"),
    primary: !report,
    run: () => openGameFolder("version", version),
  };
  if (!report) return [folder];
  return [
    { label: t("launch.alert.openCrashReport"), primary: true, run: () => openPath(report) },
    folder,
  ];
}

/** 在已安装 JDK 中找可用项：exact = 必须同主版本（launchwrapper 只要 8）；否则取 >= 要求的最低版本。 */
async function findInstalledJdk(required: number, exact: boolean): Promise<JdkInfo | null> {
  const pick = () => {
    const candidates = jdkList.value
      .map((j) => ({ j, major: parseJdkMajor(j.version) }))
      .filter((x): x is { j: JdkInfo; major: number } => x.major !== null);
    if (exact) return candidates.find((x) => x.major === required)?.j ?? null;
    if (required > 0) {
      return (
        candidates
          .filter((x) => x.major >= required)
          .sort((a, b) => a.major - b.major)[0]?.j ?? null
      );
    }
    return candidates.sort((a, b) => b.major - a.major)[0]?.j ?? null;
  };
  await loadJdkConfig();
  let match = pick();
  if (!match) {
    // 缓存没有时现场扫描一次（用户可能刚装好 JDK 或缓存过期）
    await refreshJdks();
    match = pick();
  }
  return match;
}

export async function doLaunch(
  navigateToDownload: () => void,
  navigateToSettings?: (tab: "launch" | "external") => void,
  retryJdk = false,
) {
  if (isLaunching.value) return;
  if (!selectedVersion.value) {
    navigateToDownload();
    return;
  }

  const isOffline = authMode.value === "offline";
  const isExternal = authMode.value === "external";

  if (isOffline && !username.value.trim()) {
    showToast(t("launch.error.usernameRequired"));
    return;
  }

  // 正版须已登录；启动前静默刷新 token（界面切换在前，避免点击后无反馈）
  if (!isOffline && !isExternal && !msAccount.value) {
    showToast(t("launch.error.premiumRequired"));
    return;
  }

  if (isExternal && !externalAccount.value) {
    showToast(t("launch.error.externalRequired"));
    return;
  }

  const name = isOffline
    ? username.value.trim()
    : isExternal
      ? (externalAccount.value?.name ?? "")
      : (msAccount.value?.name ?? "");
  launchState.phase = "running";
  launchState.steps = [
    { key: "launch.steps.java", status: "running" },
    { key: "launch.steps.files", status: "pending" },
    { key: "launch.steps.start", status: "pending" },
    { key: "launch.steps.window", status: "pending" },
  ];

  // 本次启动代际号：取消启动会自增，使在飞 await 的落点全部作废（后端同步收到取消请求）
  const seq = ++launchSeq;
  let windowWait: GameWindowWait | null = null;

  try {
    cancelledLabels.clear(); // 上次取消的抑制标记只对那一次启动有效
    const generation = await invoke<number>("begin_launch");
    launchGeneration = generation;
    if (seq !== launchSeq) {
      // 取消发生在 begin_launch 在飞期间：补发新代际取消，避免后端留下“可启动”代际。
      await invoke("cancel_launch", { generation }).catch(() => {});
      return;
    }

    // 启动前确保凭据可用：正版后端按需续签（未过期零网络开销）；第三方每次静默续签（Yggdrasil 无过期时间）；离线跳过
    if (isExternal) {
      const ext = externalAccount.value;
      if (ext) {
        try {
          await refreshExternalAccount(ext.api_url, ext.uuid);
        } catch (e) {
          if (seq !== launchSeq) return;
          externalAccount.value = null;
          resetLaunch();
          showError(t("launch.error.externalExpired", { error: errMsg(e) }), {
            copyable: true,
            title: t("launch.alert.loginFailed"),
            actions: [
              {
                label: t("launch.alert.openLoginSettings"),
                primary: true,
                run: () => navigateToSettings?.("external"),
              },
            ],
          });
          return;
        }
      }
    } else if (!isOffline) {
      try {
        await ensureMsAccount();
      } catch (e) {
        if (seq !== launchSeq) return;
        msAccount.value = null;
        resetLaunch();
        showError(t("launch.error.premiumExpired", { error: errMsg(e) }), {
          copyable: true,
          title: t("launch.alert.loginFailed"),
          actions: [
            {
              label: t("launch.alert.relogin"),
              primary: true,
              run: () => startMsLogin(),
            },
          ],
        });
        return;
      }
    }
    if (seq !== launchSeq) return;

    // ① 环境 + 完整性校验（后端在 Java 兼容性有风险时返回 @i18n 提示，作为 toast 提醒）
    const envNote = await invoke<string>("check_launch_env", { versionDirName: selectedVersion.value });
    if (seq !== launchSeq) return;
    if (envNote.startsWith("@i18n:")) {
      showToast(formatError(envNote).summary);
    }
    step("launch.steps.java", "done");
    step("launch.steps.files", "running");
    await new Promise(r => setTimeout(r, 100)); // 让用户看到完整性检查过程
    if (seq !== launchSeq) return;
    step("launch.steps.files", "done");

    // ② 启动游戏进程（监听先于 spawn 注册：非 Windows 平台首轮轮询即推窗口事件，晚注册会丢）
    step("launch.steps.start", "running");
    windowWait = waitGameWindow(selectedVersion.value);
    await windowWait.ready;
    if (seq !== launchSeq) {
      windowWait.cancel();
      return;
    }
    const pid = await invoke<number>("spawn_game", {
      request: {
        versionDirName: selectedVersion.value,
        authMode: isOffline ? "offline" : isExternal ? "external" : "online",
        username: name,
        accessToken: isOffline
          ? ""
          : isExternal
            ? (externalAccount.value?.access_token ?? "")
            : (msAccount.value?.access_token ?? ""),
        uuid: isOffline
          ? ""
          : isExternal
            ? (externalAccount.value?.uuid ?? "")
            : (msAccount.value?.uuid ?? ""),
        xuid: isOffline || isExternal ? "" : (msAccount.value?.xuid ?? ""),
        launchGeneration,
      },
    });
    if (seq !== launchSeq) {
      windowWait.cancel();
      return; // 已取消：后端已按取消终止该进程
    }
    windowWait.bindPid(pid);
    windowWait.start(); // 90s 窗口计时从进程拉起后开始，不占启动准备时间
    step("launch.steps.start", "done");
    step("launch.steps.window", "running");
    console.log("Game started, PID:", pid);

    // 记录最后离线用户名（与正版隔离）
    if (isOffline) {
      await invoke("save_last_offline_username", { username: name });
    }

    // ③ 等待游戏窗口（后端轮询可见窗口推送事件；90s 兜底防卡死）
    step("launch.steps.window", "running");
    const outcome = await windowWait.promise;
    if (seq !== launchSeq) return;
    step("launch.steps.window", "done");
    launchState.phase = "done";
    if (!outcome.shown) {
      const label = selectedVersion.value;
      // 进程退出与启动超时是两回事：前者带退出码与推断出的原因，后者只是还没出窗口
      if (outcome.exited) {
        showError(
          outcome.exited.code === 0
            ? t("launch.error.exitedNoWindow", { code: 0, version: label })
            : crashMessage(label, outcome.exited.code, outcome.exited.report, outcome.exited.cause),
          { copyable: true, title: crashAlertTitle(outcome.exited.cause), actions: crashActions(label, outcome.exited.report) },
        );
      } else {
        showError(t("launch.error.windowTimeout", { version: label }), {
          copyable: true,
          actions: crashActions(label, null),
        });
      }
    }

    setTimeout(() => {
      resetLaunch();
    }, 1500);
  } catch (e) {
    windowWait?.cancel();
    // 已取消：本次流程的落点全部作废（取消路径不放错误弹窗，后端「启动已取消」亦不提示）
    if (seq !== launchSeq) return;
    resetLaunch();
    const key = errorKey(e);
    const params = errorParams(e);
    const version = selectedVersion.value;
    const needJava =
      Number(params.required ?? params.need) ||
      (key === "errors.jdk.legacy_java" ? 8 : 0);
    if (
      key === "errors.launch.jdkRequired" ||
      key === "errors.jdk.too_old" ||
      key === "errors.jdk.legacy_java" ||
      key === "errors.launch.jdkNotConfigured" ||
      key === "errors.launch.jdkNotFound"
    ) {
      const knownNeed = key !== "errors.launch.jdkNotConfigured" && key !== "errors.launch.jdkNotFound";
      const exact = key === "errors.jdk.legacy_java";
      const required = exact ? 8 : knownNeed ? needJava : 0;
      const installed = await findInstalledJdk(required, exact);
      const actions: AlertAction[] = [];
      if (installed) {
        actions.push({
          label: t("launch.alert.useJava", { version: installed.version }),
          primary: true,
          run: async () => {
            await applyJdk(installed);
            if (!retryJdk) {
              await doLaunch(navigateToDownload, navigateToSettings, true);
            } else {
              showToast(t("launch.alert.javaSelected", { version: installed.version }));
            }
          },
        });
      } else if (knownNeed) {
        const major = required || 21;
        actions.push({
          label: t("launch.alert.downloadJava", { major }),
          primary: true,
          run: () => installJdk(major),
        });
      }
      actions.push({
        label: t("launch.alert.openJavaSettings"),
        primary: !installed && !knownNeed,
        run: () => navigateToSettings?.("launch"),
      });
      showError(formatError(e).full, { copyable: false, actions });
      return;
    }
    if (key === "errors.launch.filesIncomplete" || key === "errors.launch.missing_files") {
      showError(formatError(e).full, {
        copyable: false,
        actions: [
          {
            label: t("launch.alert.repairFiles"),
            primary: true,
            run: async () => {
              const total = await repairVersionFiles(version);
              if (total === 0) showToast(t("launch.version.repairComplete"));
              else showToast(t("launch.version.repairSubmitted", { count: total }));
            },
          },
          {
            label: t("launch.alert.openVersionFolder"),
            run: () => openGameFolder("version", version),
          },
        ],
      });
      return;
    }
    // 传完整错误：弹窗正文取短摘要，细节（当前 JDK、路径等）随复制按钮带走
    showError(formatError(e).full, true);
  }
}

type GameExitedPayload = [number, string, number | null, string | null, string | null];

/** 启动流程正在等待窗口的标签：该标签的退出事件由 doLaunch 统一提示，全局监听跳过（避免两条弹窗）。 */
let waitingLabel: string | null = null;

/** 等待结果：窗口是否出现；未出现时是否因进程退出（带退出码、证据与推断出的原因）。 */
interface WindowWait {
  shown: boolean;
  exited: { code: number | null; report: string | null; cause: string | null } | null;
}

/** 窗口等待句柄：ready 表示监听已注册（必须先于 spawn_game）；bindPid 绑定本次进程；start 在进程拉起后启动 90s 计时；cancel 供失败/取消清理。 */
interface GameWindowWait {
  promise: Promise<WindowWait>;
  ready: Promise<void>;
  bindPid: (pid: number) => void;
  start: () => void;
  cancel: () => void;
}

/** 从 Mixin fatal 行提取失败 Mod id；显示时首字母大写，至少比只写“Mixin 失败”可定位。 */
function failingMixinMod(cause: string | null): string {
  const match = cause?.match(/Mixin apply for mod ([^\s]+)/i);
  if (!match) return "";
  const id = match[1].replace(/[^\w.-]/g, "");
  return id ? id.charAt(0).toUpperCase() + id.slice(1) : "";
}

/** 启动失败弹窗正文：首行给结论，其余为可复制的排查详情（弹窗正文只显示首行）。 */
function crashMessage(
  version: string,
  code: number | null,
  report: string | null,
  cause: string | null,
): string {
  const c = cause ? formatError(cause) : null;
  const failedMod = failingMixinMod(cause);
  const causeSummary = c?.summary ?? t("launch.error.causeUnknown");
  const summary = failedMod
    ? t("launch.error.crashedWithMod", {
        code: code === null ? "?" : code,
        mod: failedMod,
      })
    : t("launch.error.crashed", {
        code: code === null ? "?" : code,
        cause: causeSummary,
      });
  const lines = [summary, t("launch.error.detailVersion", { version })];
  if (c?.detail) lines.push(c.detail);
  lines.push(t("launch.error.detailLog"));
  if (report) lines.push(t("launch.error.detailReport", { report }));
  return lines.join("\n");
}

function crashAlertTitle(cause: string | null): string {
  if (cause?.includes("errors.launch.mod_mixin")) return t("launch.alert.modIncompatible");
  if (cause?.includes("errors.launch.missing_files")) return t("launch.alert.filesIncomplete");
  return t("launch.alert.launchFailed");
}

let gameExitListenerReady = false;

function waitGameWindow(label: string): GameWindowWait {
  let settled = false;
  let windowSeen = false;
  let exited: WindowWait["exited"] = null;
  const unlisteners: Array<() => void> = [];
  let resolveWait!: (w: WindowWait) => void;
  const promise = new Promise<WindowWait>((r) => {
    resolveWait = r;
  });
  let timer: number | undefined;
  let expectedPid: number | null = null;
  let pendingExit: {
    pid: number;
    code: number | null;
    report: string | null;
    cause: string | null;
  } | null = null;
  const finish = () => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    waitingLabel = null;
    for (const u of unlisteners) u();
    resolveWait({ shown: windowSeen, exited });
  };
  const start = () => {
    if (settled || timer !== undefined) return;
    timer = setTimeout(finish, 90_000);
  };
  const acceptExit = (code: number | null, report: string | null, cause: string | null) => {
    if (settled) return;
    exited = { code, report, cause };
    finish();
  };
  /** 绑定本次 spawn 的 pid；绑定前到达的退出事件先缓存，绑定后按 pid 过滤陈旧进程。 */
  const bindPid = (pid: number) => {
    expectedPid = pid;
    if (pendingExit?.pid === pid) {
      const e = pendingExit;
      pendingExit = null;
      acceptExit(e.code, e.report, e.cause);
    }
  };
  waitingLabel = label;
  const register = (u: () => void) => {
    if (settled) u();
    else unlisteners.push(u);
  };
  const ready = Promise.all([
    listen<[number, string]>(GAME_WINDOW_VISIBLE, (ev) => {
      if (ev.payload[1] !== label) return;
      windowSeen = true;
      finish();
    }).then(register),
    listen<GameExitedPayload>(GAME_EXITED, (ev) => {
      const [pid, , code, report, cause] = ev.payload;
      if (ev.payload[1] !== label) return;
      if (expectedPid === null) {
        pendingExit = { pid, code, report, cause };
        return;
      }
      if (pid !== expectedPid) return;
      acceptExit(code, report, cause);
    }).then(register),
  ]).then(() => {});
  return { promise, ready, bindPid, start, cancel: finish };
}

/** 最近一次游戏日志（launcher/process.rs 落盘）。 */
export async function loadGameLog(gameDir: string, versionName: string): Promise<string> {
  return await invoke<string>("get_game_log", { gameDir, versionName });
}

export async function ensureGameExitListener() {
  if (gameExitListenerReady) return;
  gameExitListenerReady = true;
  await listen<GameExitedPayload>(GAME_EXITED, (ev) => {
    console.log("Game exited:", ev.payload);
    const [, label, exitCode, crashReport, cause] = ev.payload;
    // 启动流程还在等窗口时由它统一提示（同一次退出只弹一条）
    if (waitingLabel === label) return;
    // 取消启动会终止已拉起的进程（非零退出码）：不算崩溃
    if (exitCode !== null && exitCode !== 0 && !cancelledLabels.has(label)) {
      const message = crashMessage(label, exitCode, crashReport, cause);
      showError(message, { copyable: true, title: crashAlertTitle(cause), actions: crashActions(label, crashReport) });
      void notifyIfUnfocused(t("common.notify.gameCrashed"), errMsg(message));
    }
  });
}
