// 任务栏/程序坞进度与用户注意：汇总「启动器下载任务」和「工具箱下载器」两路状态。
// 只做节流后的 IPC，避免每个进度帧都调用窗口 API。
import {
  getCurrentWindow,
  ProgressBarStatus,
  UserAttentionType,
} from "@tauri-apps/api/window";

export interface TaskbarSourceState {
  running: number;
  paused: number;
  /** 0–100；无活跃任务时忽略。 */
  progress: number;
}

type Source = "task" | "tool";

const sources: Record<Source, TaskbarSourceState> = {
  task: { running: 0, paused: 0, progress: 0 },
  tool: { running: 0, paused: 0, progress: 0 },
};

let timer: number | null = null;
let appliedKey = "";
let lastAttentionAt = 0;

export function setTaskbarSource(source: Source, state: TaskbarSourceState) {
  sources[source] = state;
  if (timer !== null) return;
  timer = window.setTimeout(() => {
    timer = null;
    void flush();
  }, 300);
}

function aggregate(): { status: ProgressBarStatus; progress?: number } {
  const running = [sources.task, sources.tool].filter(s => s.running > 0);
  if (running.length > 0) {
    const count = running.reduce((sum, s) => sum + s.running, 0);
    const progress = running.reduce((sum, s) => sum + s.progress * s.running, 0) / count;
    return { status: ProgressBarStatus.Normal, progress: Math.min(100, Math.max(0, progress)) };
  }
  const paused = [sources.task, sources.tool].filter(s => s.paused > 0);
  if (paused.length > 0) {
    const count = paused.reduce((sum, s) => sum + s.paused, 0);
    const progress = paused.reduce((sum, s) => sum + s.progress * s.paused, 0) / count;
    return { status: ProgressBarStatus.Paused, progress: Math.min(100, Math.max(0, progress)) };
  }
  return { status: ProgressBarStatus.None };
}

async function flush() {
  const { status, progress } = aggregate();
  const key = `${status}:${progress ?? ""}`;
  if (key === appliedKey) return;
  appliedKey = key;
  try {
    await getCurrentWindow().setProgressBar(
      status === ProgressBarStatus.None
        ? { status }
        : { status, progress },
    );
  } catch {
    // 窗口销毁中/平台不支持：忽略
  }
}

/** 窗口失焦时请求系统注意；聚焦时不打扰。 */
export async function flashTaskbarAttention(kind: "info" | "critical" = "info") {
  try {
    const now = Date.now();
    if (now - lastAttentionAt < 3000) return;
    const win = getCurrentWindow();
    if (await win.isFocused()) return;
    lastAttentionAt = now;
    await win.requestUserAttention(
      kind === "critical" ? UserAttentionType.Critical : UserAttentionType.Informational,
    );
  } catch {
    // 平台不支持或窗口已关闭
  }
}
