// 头像与皮肤预览渲染：统一走 skinRender.ts 等距渲染器；离线用户名回退像素脸（2D canvas）。状态在模块级 avatarState，渲染函数直接导出。
import { reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { readCache, writeCache } from "../../utils/localCache";

/** 取 canvas 2D 上下文：内存压力下可能为 null，调用方须兜底。 */
function ctx2d(c: HTMLCanvasElement): CanvasRenderingContext2D | null {
  return c.getContext("2d");
}

/** FNV-1a 哈希（与 Rust 侧 config/launcher/skin.rs hash_name 一致）。 */
function hashName(name: string): number {
  let h = 2166136261;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** 离线皮肤模型决策：哈希奇数 → Alex（slim）。 */
function randomIsAlex(name: string): boolean {
  return hashName(name) % 2 === 1;
}

export const avatarState = reactive({
  premiumAvatar: "",
  offlineAvatar: "",
  externalAvatar: "",
  avatarError: false,
});

// 模块级去重缓存：账号/用户名未变不重复请求（重挂载不闪占位）
let lastPremiumUuid = "";
let lastOfflineName = "";
let lastExternalKey = "";
let avatarTimer: number | undefined;

/** 头部头像输出边长：容器 96px，按 2× 输出防高分屏发虚/去饱和。 */
const HEAD_AVATAR_PX = 192;

/** 头像缓存条目：dataURL + 源贴图（贴图未变则跳过重绘）；at 用于超限淘汰。 */
interface HeadCacheEntry {
  head: string;
  skin: string;
  model: string;
  at: number;
}

/** 每类头像缓存上限：条目为 192px PNG，64 条约 1 MB。 */
const HEAD_CACHE_MAX = 64;

/** uuid（正版）/ `apiUrl|uuid`（第三方）→ 头像，模块加载时读入供首帧直出。 */
const premiumHeadCache = readCache<Record<string, HeadCacheEntry>>("avatar.premium") ?? {};
const externalHeadCache = readCache<Record<string, HeadCacheEntry>>("avatar.external") ?? {};

function putHeadCache(
  storeKey: string,
  cache: Record<string, HeadCacheEntry>,
  key: string,
  entry: HeadCacheEntry,
) {
  cache[key] = entry;
  const stale = Object.keys(cache)
    .filter((k) => k !== key)
    .sort((a, b) => cache[a].at - cache[b].at)
    .slice(0, Math.max(0, Object.keys(cache).length - HEAD_CACHE_MAX));
  for (const k of stale) delete cache[k];
  writeCache(storeKey, cache);
}

export async function renderSkinHead3D(b64: string): Promise<string> {
  if (!b64) return "";
  // 3D 渲染器按需加载：启动期只需头像，首屏不解析整套身体渲染代码。
  const { renderHeadDataUrl } = await import("../skin/skinRender");
  return renderHeadDataUrl(b64, HEAD_AVATAR_PX);
}

/** 衣柜 3D 缩略图缓存上限（超出按插入顺序淘汰最旧）。 */
const SKIN_THUMB_CACHE_MAX = 64;
const skinThumbCache = new Map<string, string>();

/** 衣柜卡片缩略图：3/4 正面 + 微俯，每体素 8px（160×272；卡片行高 186px×DPR，低于此发糊）；按皮肤+模型缓存，失败回落平面图。 */
export async function renderSkinThumb3D(b64: string, slim = false): Promise<string> {
  if (!b64) return "";
  const key = `S|${slim ? 1 : 0}|${b64}`;
  const hit = skinThumbCache.get(key);
  if (hit !== undefined) return hit;
  const { DEFAULT_VIEW_PITCH, DEFAULT_VIEW_YAW, renderFullDataUrl } = await import("../skin/skinRender");
  const url = await renderFullDataUrl(b64, slim, 8, DEFAULT_VIEW_YAW, DEFAULT_VIEW_PITCH, 0.86);
  if (skinThumbCache.size >= SKIN_THUMB_CACHE_MAX) {
    const oldest = skinThumbCache.keys().next().value;
    if (oldest !== undefined) skinThumbCache.delete(oldest);
  }
  skinThumbCache.set(key, url);
  return url;
}

export async function renderPremiumAvatar(uuid: string, force = false) {
  if (!uuid) {
    avatarState.premiumAvatar = "";
    lastPremiumUuid = "";
    return;
  }
  const cached = premiumHeadCache[uuid];
  const changed = lastPremiumUuid !== uuid;
  if (cached?.head) {
    avatarState.premiumAvatar = cached.head;
    avatarState.avatarError = false;
  } else if (changed) {
    avatarState.premiumAvatar = ""; // 换账号时先清掉上一账号的头像
  }
  // 账号未变 → 不重复请求（含进行中，避免并发竞态；失败时重置可重试）
  if (!force && !changed) {
    return;
  }
  lastPremiumUuid = uuid;
  try {
    const profile = await invoke<{ skin: string; model: string }>("fetch_account_skin_profile", { uuid });
    if (cached?.head && cached.skin === profile.skin) {
      avatarState.avatarError = false;
      return;
    }
    const head = await renderSkinHead3D(profile.skin).catch(() => "");
    avatarState.premiumAvatar = head || cached?.head || "";
    avatarState.avatarError = false;
    if (head) {
      putHeadCache("avatar.premium", premiumHeadCache, uuid, {
        head,
        skin: profile.skin,
        model: profile.model,
        at: Date.now(),
      });
    }
  } catch (e) {
    // 装饰性资源：静默降级（UI 回退占位图标），不弹 dialog；网络恢复后可点刷新重试
    console.error("正版头像加载失败:", e);
    if (cached?.head) {
      avatarState.premiumAvatar = cached.head;
      avatarState.avatarError = false;
      lastPremiumUuid = ""; // 保留离线头像，下次挂载仍尝试刷新
      return;
    }
    avatarState.premiumAvatar = "";
    lastPremiumUuid = ""; // 失败允许下次重试
    avatarState.avatarError = true;
  }
}

function mulberry32(seed: number) {
  return function () {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** 离线头像：按用户名哈希确定性渲染 Steve/Alex 默认皮肤（与游戏内一致），失败回退像素脸。 */
export async function renderOfflineAvatar(name: string) {
  const trimmed = name.trim();
  if (!trimmed) {
    avatarState.offlineAvatar = "";
    lastOfflineName = "";
    return;
  }
  if (lastOfflineName === trimmed) {
    return; // 用户名未变 → 不重复请求（含进行中）
  }
  lastOfflineName = trimmed;
  const head = await renderOfflineHead(trimmed);
  if (head) {
    avatarState.offlineAvatar = head;
  } else {
    lastOfflineName = ""; // 失败允许下次重试（如版本 jar 就绪后）
    avatarState.offlineAvatar = generatePixelFace(trimmed);
  }
}

// ── 离线头像缓存（用户名 → Steve/Alex 3D 头部 dataURL；模块级 reactive，重建不丢，列表与主头像共用）──
export const offlineHeads = reactive<Record<string, string>>({});

/** 离线用户名 → Steve/Alex 默认皮肤 3D 头部（哈希决定模型）；仅缓存成功结果，失败返回 "" 以便重试。 */
export async function renderOfflineHead(name: string): Promise<string> {
  const trimmed = name.trim();
  if (!trimmed) return "";
  const cached = offlineHeads[trimmed];
  if (cached !== undefined) return cached;
  try {
    const skinKind = randomIsAlex(trimmed) ? "alex" : "steve";
    const b64 = await invoke<string>("fetch_default_skin", { skin: skinKind });
    const head = await renderSkinHead3D(b64);
    offlineHeads[trimmed] = head;
    return head;
  } catch {
    return "";
  }
}

/** 第三方头像键：同一站点的不同角色共用一个缓存命名空间。 */
function externalHeadKey(apiUrl: string, uuid: string): string {
  return `${apiUrl}|${uuid}`;
}

/** 已缓存的正版皮肤（含模型与时间）：头像先拉过时，衣柜首帧无需再等一次网络。 */
export function cachedPremiumSkin(
  uuid: string,
): { skin: string; model: string; at: number } | null {
  const entry = premiumHeadCache[uuid];
  return entry?.skin ? { skin: entry.skin, model: entry.model || "classic", at: entry.at } : null;
}

/** 已缓存的第三方皮肤（含模型与时间）；角色 uuid 维度，与头像缓存共用。 */
export function cachedExternalSkin(
  apiUrl: string,
  uuid: string,
): { skin: string; model: string; at: number } | null {
  const entry = externalHeadCache[externalHeadKey(apiUrl, uuid)];
  return entry?.skin ? { skin: entry.skin, model: entry.model || "classic", at: entry.at } : null;
}

/** 进行中的第三方头像请求：同一角色并发打开菜单/角色面板时只发一次。 */
const externalHeadPending = new Map<string, Promise<string>>();

/** 拉取第三方角色头部；贴图未变时直接复用缓存，不重绘。 */
async function fetchExternalHead(apiUrl: string, uuid: string): Promise<string> {
  const key = externalHeadKey(apiUrl, uuid);
  const pending = externalHeadPending.get(key);
  if (pending) return pending;
  const task = (async () => {
    const profile = await invoke<{ skin: string; model: string }>("fetch_external_skin_profile", { apiUrl, uuid });
    const cached = externalHeadCache[key];
    if (cached?.head && cached.skin === profile.skin) return cached.head;
    const head = await renderSkinHead3D(profile.skin);
    if (head) {
      putHeadCache("avatar.external", externalHeadCache, key, {
        head,
        skin: profile.skin,
        model: profile.model,
        at: Date.now(),
      });
    }
    return head;
  })().finally(() => externalHeadPending.delete(key));
  externalHeadPending.set(key, task);
  return task;
}

/** 第三方角色头部（角色列表用）：缓存优先，后台补拉最新贴图。 */
export async function renderExternalHead(apiUrl: string, uuid: string): Promise<string> {
  const cached = externalHeadCache[externalHeadKey(apiUrl, uuid)];
  if (cached?.head) {
    void fetchExternalHead(apiUrl, uuid).catch(() => "");
    return cached.head;
  }
  return fetchExternalHead(apiUrl, uuid).catch(() => "");
}

export async function renderExternalAvatar(apiUrl: string, uuid: string, force = false) {
  const key = externalHeadKey(apiUrl, uuid);
  if (!uuid || !apiUrl) {
    avatarState.externalAvatar = "";
    lastExternalKey = "";
    return;
  }
  const cached = externalHeadCache[key];
  const changed = lastExternalKey !== key;
  if (cached?.head) {
    avatarState.externalAvatar = cached.head;
    avatarState.avatarError = false;
  } else if (changed) {
    avatarState.externalAvatar = "";
  }
  if (!force && !changed) return;
  lastExternalKey = key;
  try {
    const head = await fetchExternalHead(apiUrl, uuid);
    avatarState.externalAvatar = head || cached?.head || "";
    avatarState.avatarError = false;
  } catch (e) {
    console.error("第三方头像加载失败:", e);
    if (cached?.head) {
      avatarState.externalAvatar = cached.head;
      avatarState.avatarError = false;
      lastExternalKey = "";
      return;
    }
    avatarState.externalAvatar = "";
    lastExternalKey = "";
  }
}

/** 8×8 Minecraft 风格像素脸（确定性、左右镜像）：主头像兜底与历史列表行共用（同用户名同脸）。 */
export function generatePixelFace(name: string): string {
  const trimmed = name.trim();
  const seed = hashName(trimmed);
  const rng = mulberry32(seed);
  const skinTones = ["#c68642", "#b5763a", "#a0662f", "#d1a579", "#e0ac69", "#8d5524", "#e8b57e"];
  const hairColors = ["#2a2222", "#3b2f2f", "#4a3728", "#1f1f1f", "#5c4033", "#6b4a2a"];

  const base = skinTones[seed % skinTones.length];
  const hair = hairColors[(seed >> 3) % hairColors.length];
  const eye = "#2f2f2f";

  const canvas = document.createElement("canvas");
  canvas.width = 8;
  canvas.height = 8;
  const ctx = ctx2d(canvas);
  if (!ctx) return ""; // 拿不到上下文返回空串，调用方按无头像降级

  ctx.fillStyle = base;
  ctx.fillRect(0, 0, 8, 8);
  ctx.fillStyle = hair;
  for (let x = 0; x < 4; x++) {
    for (let y = 0; y < 2; y++) {
      if (rng() > 0.35) {
        ctx.fillRect(x, y, 1, 1);
        ctx.fillRect(7 - x, y, 1, 1);
      }
    }
  }
  ctx.fillStyle = eye;
  ctx.fillRect(2, 3, 1, 1);
  ctx.fillRect(5, 3, 1, 1);
  ctx.fillRect(3, 6, 1, 1);
  ctx.fillRect(4, 6, 1, 1);

  const out = document.createElement("canvas");
  out.width = 96;
  out.height = 96;
  const octx = ctx2d(out);
  if (!octx) return "";
  octx.drawImage(canvas, 0, 0, 96, 96);
  return out.toDataURL("image/png");
}

/** 用户名输入防抖 1.337s 后刷新离线头像。 */
export function scheduleOfflineRender(name: string) {
  avatarState.avatarError = false;
  clearTimeout(avatarTimer); // 对任意状态句柄均无副作用，无需判断
  avatarTimer = window.setTimeout(() => void renderOfflineAvatar(name), 1337);
}

// ── 账号列表 3D 头像（sessionserver 皮肤头部渲染，按 uuid 模块级缓存）──

/** 正版账号 3D 头部缓存（uuid → dataURL；失败也占位空串防重复请求）。 */
export const accountHeads = reactive<Record<string, string>>({});

for (const [uuid, entry] of Object.entries(premiumHeadCache)) {
  if (entry?.head) accountHeads[uuid] = entry.head;
}

/** 本次会话已刷新 / 正在刷新的 uuid：命中缓存先同步返回，再后台换新。 */
const accountHeadRefreshed = new Set<string>();
const accountHeadPending = new Set<string>();

async function refreshAccountHead(uuid: string): Promise<string> {
  if (accountHeadPending.has(uuid)) return accountHeads[uuid] ?? "";
  accountHeadPending.add(uuid);
  try {
    const profile = await invoke<{ skin: string; model: string }>("fetch_account_skin_profile", { uuid });
    const entry = premiumHeadCache[uuid];
    if (entry?.head && entry.skin === profile.skin) {
      accountHeads[uuid] = entry.head;
    } else {
      const head = await renderSkinHead3D(profile.skin);
      accountHeads[uuid] = head;
      if (head) {
        putHeadCache("avatar.premium", premiumHeadCache, uuid, {
          head,
          skin: profile.skin,
          model: profile.model,
          at: Date.now(),
        });
      }
    }
    accountHeadRefreshed.add(uuid);
    return accountHeads[uuid] ?? "";
  } catch {
    if (accountHeads[uuid] === undefined) accountHeads[uuid] = "";
    accountHeadRefreshed.add(uuid);
    return accountHeads[uuid] ?? "";
  } finally {
    accountHeadPending.delete(uuid);
  }
}

export async function renderAccountHead(uuid: string): Promise<string> {
  const cached = accountHeads[uuid];
  if (cached !== undefined) {
    if (cached && !accountHeadRefreshed.has(uuid)) void refreshAccountHead(uuid);
    return cached;
  }
  return refreshAccountHead(uuid);
}
