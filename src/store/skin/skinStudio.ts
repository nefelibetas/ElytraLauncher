// 衣柜 store（模块级 reactive 单例，无 Pinia）：提供皮肤/披风/视角状态（渲染由页面 watch 后重绘）；拖动旋转：pointer 累加 yaw，pitch 限 ±90°（可转到正顶/正底视图）。
import { reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { authMode, ensureAuthReady, externalAccount, msAccount } from "../auth/auth";
import { cachedExternalSkin, cachedPremiumSkin, renderPremiumAvatar } from "../auth/avatar";
import { DEFAULT_VIEW_PITCH, DEFAULT_VIEW_YAW } from "../skin/skinRender";
import type { Animation } from "../skin/skinRender";
import { readCache, writeCache } from "../../utils/localCache";
import { showError } from "../app/alert";
import { showToast } from "../../utils/toast";
import { t } from "../../i18n";

/** 本地衣柜条目（后端 LocalSkinView）。 */
export interface LocalSkin {
  id: string;
  name: string;
  variant: "CLASSIC" | "SLIM";
  added_at: number;
  source: string;
  /** 衣柜内绝对路径（可直接交给 upload_skin_file 应用） */
  path: string;
  /** 皮肤 PNG base64（预览/缩略图用；文件缺失时为空串） */
  skin: string;
}

export type StudioTab = "skins" | "capes";

/** 官方披风条目（后端 CapeInfo）。 */
export interface StudioCape {
  id: string;
  alias: string;
  url: string;
  active: boolean;
  /** 披风贴图 base64（下载失败为空串；WebView 无 http 插件，只能由后端带出） */
  texture: string;
}

/** 俯仰角范围：限制在 ±60°，避免拖到正顶/正底后模型只剩上/下面、看起来像“躺下”。
 *  正常 3/4 视角和俯视检查都在此范围内。 */
const PITCH_MIN = -60;
const PITCH_MAX = 60;
/** 拖动灵敏度（度/像素）。yaw 符号与 viewer 相机配套（左拖 → 模型左转）；pitch 正号（下拖 → 视角下移）。 */
const YAW_SENS = -0.6;
const PITCH_SENS = 0.5;
/** 上传官方账号后等待 Mojang 处理新纹理生效的时间。 */
const APPLY_SETTLE_MS = 3000;

/** `Promise.withResolvers` 为 ES2024 新增，tsconfig lib 为 ES2020 未收录，故声明其形状；运行时可能缺失（旧 WebKitGTK），调用前用 `typeof` 守卫。 */
const promiseCtor = Promise as unknown as {
  withResolvers?: <T>() => { promise: Promise<T>; resolve: (value: T) => void };
};

/** 延时等待：优先 `Promise.withResolvers`（WebKitGTK 无此 API 则回退 executor）。必须写成 `promiseCtor.withResolvers()`——该静态方法依赖 `this` 指向 Promise 构造器。 */
function delay(ms: number): Promise<void> {
  if (typeof promiseCtor.withResolvers === "function") {
    const { promise, resolve } = promiseCtor.withResolvers<void>();
    setTimeout(() => resolve(undefined), ms);
    return promise;
  }
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

/** 拖动会话：上一次指针位置（非响应式，仅内部用）。 */
let lastX = 0;
let lastY = 0;

/** 衣柜当前皮肤缓存：带账号键，换账号后不误用上一账号的皮肤。 */
interface OfficialSkinCache {
  key: string;
  skin: string;
  model: string;
  at: number;
}

/** 衣柜披风列表缓存：含贴图与启用态，离线时仍可预览与切换入口展示。 */
interface CapeListCache {
  key: string;
  list: StudioCape[];
  at: number;
}

const OFFICIAL_CACHE_KEY = "studio.official";
const CAPE_CACHE_KEY = "studio.capes";

/** 衣柜缓存键：正版 uuid / 第三方 api_url+uuid / 离线；账号切换后自然失效。 */
function accountCacheKey(): string {
  const ext = externalAccount.value;
  if (authMode.value === "external" && ext) return `ext:${ext.api_url}|${ext.uuid}`;
  const acc = msAccount.value;
  if (authMode.value === "premium" && acc) return `ms:${acc.uuid}`;
  return "offline";
}

const ZOOM_MIN = 0.4;
const ZOOM_MAX = 4;

export const skinStudio = reactive({
  // ── 加载状态机 ──
  /** `loading` = 首帧或刷新中（右侧整体展示刷新态，不与旧列表共存）；`ready` = 三方数据已就绪、右侧展示列表。 */
  phase: "loading" as "loading" | "ready",

  // ── 3D 预览 ──
  dragging: false,
  /** 预览动作：`none` = 静止站姿，`idle` = 待机呼吸，`walk` = 行走 */
  animation: "none" as Animation,
  /** 初始视角见 DEFAULT_VIEW_*：3/4 正面 + 微俯。 */
  yaw: DEFAULT_VIEW_YAW,
  pitch: DEFAULT_VIEW_PITCH,
  zoom: 1,

  // ── 账号当前皮肤 ──
  officialSkin: "",
  /** "classic" | "slim"（后端 fetch_*_skin_profile 原样返回） */
  officialModel: "classic",
  officialError: "",

  // ── 本地衣柜 ──
  skins: [] as LocalSkin[],
  skinsError: "",
  tab: "skins" as StudioTab,

  // ── 官方披风 ──
  capes: [] as StudioCape[],
  capesError: "",
  capeBusy: false,

  // ── 选择状态 ──
  /** 'official' = 账号当前皮肤；否则为本地条目 id */
  selectedKey: "official",
  /** 预览披风 id（= 当前启用披风）；"" = 无披风 */
  selectedCapeId: "",

  // ── 上传皮肤（选文件 → 确认模型） ──
  applyBusy: false,
  skinModelDialog: false,
  pendingSkinPath: "",

  // ── 派生 ──
  /** 可执行账号级操作（应用皮肤 / 管理披风）——须为已登录的正版账号。 */
  get premium(): boolean {
    return authMode.value === "premium" && !!msAccount.value;
  },

  get selectedLocal(): LocalSkin | null {
    if (this.selectedKey === "official") return null;
    return this.skins.find((s) => s.id === this.selectedKey) ?? null;
  },

  get currentSkin(): string {
    return this.selectedLocal?.skin || this.officialSkin;
  },

  get currentSlim(): boolean {
    const local = this.selectedLocal;
    if (local) return local.variant === "SLIM";
    return this.officialModel.toLowerCase() === "slim";
  },

  get currentCapeTexture(): string {
    if (!this.selectedCapeId) return "";
    return this.capes.find((c) => c.id === this.selectedCapeId)?.texture ?? "";
  },

  get canApply(): boolean {
    return this.premium && !!this.selectedLocal && !this.applyBusy;
  },

  // ── 加载 ──

  /** 首帧缓存：当前账号皮肤 + 披风列表同步就绪；账号键不匹配的旧缓存直接忽略。 */
  hydrateCache() {
    const key = accountCacheKey();
    const official = readCache<OfficialSkinCache>(OFFICIAL_CACHE_KEY);
    const studioSkin = official?.key === key && official.skin ? official : null;
    // 头像已在启动时拉过：直接借用其贴图，首次进衣柜也不用再等一轮网络
    const acc = msAccount.value;
    const ext = externalAccount.value;
    const avatarSkin = this.premium && acc
      ? cachedPremiumSkin(acc.uuid)
      : authMode.value === "external" && ext
        ? cachedExternalSkin(ext.api_url, ext.uuid)
        : null;
    const cachedSkin = studioSkin && (!avatarSkin || studioSkin.at >= avatarSkin.at) ? studioSkin : avatarSkin;
    if (cachedSkin) {
      this.officialSkin = cachedSkin.skin;
      this.officialModel = cachedSkin.model || "classic";
    }
    if (!this.premium) {
      this.capes = [];
      this.selectedCapeId = "";
      return;
    }
    const capes = readCache<CapeListCache>(CAPE_CACHE_KEY);
    if (capes?.key === key && Array.isArray(capes.list)) {
      this.capes = capes.list;
      this.selectedCapeId = capes.list.find((c) => c.active)?.id ?? "";
    }
  },

  async load() {
    this.yaw = DEFAULT_VIEW_YAW;
    this.pitch = DEFAULT_VIEW_PITCH;
    this.zoom = 1;
    this.phase = "loading";
    // 账号列表是异步 IPC：未就绪就判定来源会把正版账号误判为离线
    await ensureAuthReady();
    this.officialSkin = "";
    this.officialModel = "classic";
    this.officialError = "";
    this.capes = [];
    this.selectedCapeId = "";
    this.hydrateCache();
    // 有缓存 → 立即放行首帧，网络刷新在后台无缝替换；无缓存才等首轮网络
    if (this.officialSkin) {
      this.phase = "ready";
      void this.refreshData();
    } else {
      await this.refreshData();
      this.phase = "ready";
    }
  },

  /** 皮肤 / 本地衣柜 / 披风的后台刷新；已有缓存时首帧不等待它。 */
  async refreshData() {
    await Promise.allSettled([this.loadOfficial(), this.loadSkins()]);
    this.phase = "ready";
    void this.cacheCurrent(); // 自动缓存当前皮肤（后台进行，不阻塞首帧）
    void this.loadCapes();
  },

  /** 拉取账号当前皮肤（正版 / 第三方 / 离线各自来源）；失败保留缓存，无缓存才回退内置 Steve。 */
  async loadOfficial() {
    this.officialError = "";
    await ensureAuthReady();
    const key = accountCacheKey();
    const cachedEntry = readCache<OfficialSkinCache>(OFFICIAL_CACHE_KEY);
    const cached = cachedEntry?.key === key && cachedEntry.skin ? cachedEntry : null;
    const acc = msAccount.value;
    const ext = externalAccount.value;
    try {
      let skin = "";
      let model = "classic";
      if (this.premium && acc) {
        const p = await invoke<{ skin: string; model: string }>("fetch_account_skin_profile", {
          uuid: acc.uuid,
        });
        skin = p.skin;
        model = p.model || "classic";
      } else if (authMode.value === "external" && ext) {
        const p = await invoke<{ skin: string; model: string }>("fetch_external_skin_profile", {
          apiUrl: ext.api_url,
          uuid: ext.uuid,
        });
        skin = p.skin;
        model = p.model || "classic";
      } else {
        skin = await invoke<string>("fetch_default_skin", { skin: "steve" });
      }
      if (accountCacheKey() !== key) return; // 请求期间切换账号：丢弃旧结果
      this.officialSkin = skin;
      this.officialModel = model;
      const entry: OfficialSkinCache = { key, skin, model, at: Date.now() };
      writeCache(OFFICIAL_CACHE_KEY, entry);
    } catch (e) {
      if (accountCacheKey() !== key) return;
      if (cached) {
        this.officialSkin = cached.skin;
        this.officialModel = cached.model || "classic";
        console.warn("衣柜皮肤刷新失败，沿用本地缓存:", e);
        return;
      }
      this.officialError = t("skin.studio.errors.officialLoad", { error: String(e) });
      try {
        this.officialSkin = await invoke<string>("fetch_default_skin", { skin: "steve" });
        this.officialModel = "classic";
      } catch {
        this.officialSkin = "";
      }
    }
  },

  async loadSkins() {
    this.skinsError = "";
    try {
      this.skins = await invoke<LocalSkin[]>("list_local_skins");
      // 选中项已不存在（被删/换账号）→ 回落官方皮肤
      if (this.selectedKey !== "official" && !this.skins.some((s) => s.id === this.selectedKey)) {
        this.selectedKey = "official";
      }
    } catch (e) {
      this.skinsError = t("skin.studio.errors.skinsLoad", { error: String(e) });
    } finally {
    }
  },

  async loadCapes() {
    this.capesError = "";
    if (!this.premium) {
      this.capes = [];
      this.selectedCapeId = "";
      return;
    }
    const key = accountCacheKey();
    const cachedEntry = readCache<CapeListCache>(CAPE_CACHE_KEY);
    const cached = cachedEntry?.key === key && Array.isArray(cachedEntry.list) ? cachedEntry : null;
    try {
      const list = await invoke<StudioCape[]>("fetch_capes");
      if (accountCacheKey() !== key) return; // 请求期间切换账号：丢弃旧结果
      // 贴图按 url 稳定：单个下载失败时用上次缓存补齐
      const textureByUrl = new Map((cached?.list ?? []).map((c) => [c.url, c.texture]));
      for (const c of list) {
        if (!c.texture && c.url) c.texture = textureByUrl.get(c.url) ?? "";
      }
      this.capes = list;
      this.selectedCapeId = list.find((c) => c.active)?.id ?? "";
      const entry: CapeListCache = { key, list, at: Date.now() };
      writeCache(CAPE_CACHE_KEY, entry);
    } catch (e) {
      if (accountCacheKey() !== key) return;
      if (cached) {
        this.capes = cached.list;
        this.selectedCapeId = cached.list.find((c) => c.active)?.id ?? "";
        console.warn("披风列表刷新失败，沿用本地缓存:", e);
        return;
      }
      this.capesError = t("skin.studio.errors.capesLoad", { error: String(e) });
    }
  },

  async refresh() {
    this.phase = "loading";
    await Promise.allSettled([this.loadOfficial(), this.loadSkins()]);
    this.phase = "ready";
    void this.cacheCurrent();
    void this.loadCapes();
    showToast(t("skin.studio.refreshed"));
  },

  dispose() {
    this.dragging = false;
  },

  // ── 拖动旋转 ──

  beginDrag(e: PointerEvent) {
    if (e.button !== 0) return; // 仅左键/单指（右键留给上下文菜单）
    e.preventDefault();
    const el = e.currentTarget as HTMLElement | null;
    try {
      el?.setPointerCapture(e.pointerId);
    } catch {
      // 指针已失效：忽略，后续 move 由元素自身接收
    }
    this.dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
  },

  moveDrag(e: PointerEvent) {
    if (!this.dragging) return;
    this.yaw += (e.clientX - lastX) * YAW_SENS;
    this.pitch = Math.max(PITCH_MIN, Math.min(PITCH_MAX, this.pitch + (e.clientY - lastY) * PITCH_SENS));
    lastX = e.clientX;
    lastY = e.clientY;
  },

  endDrag(e: PointerEvent) {
    if (!this.dragging) return;
    this.dragging = false;
    const el = e.currentTarget as HTMLElement | null;
    try {
      if (el?.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    } catch {
      // 已释放：忽略
    }
  },

  resetView() {
    this.yaw = DEFAULT_VIEW_YAW;
    this.pitch = DEFAULT_VIEW_PITCH;
    this.zoom = 1;
  },

  zoomBy(factor: number) {
    this.zoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, this.zoom * factor));
  },

  // ── 衣柜操作 ──

  /** 自动缓存当前皮肤进衣柜。**只缓存正版账号**（第三方在站点网页管理、离线默认不入柜）；按来源 + 贴图去重。 */
  async cacheCurrent() {
    const skin = this.officialSkin;
    const acc = msAccount.value;
    if (!this.premium || !acc || !skin) return;
    if (this.skinsError) return; // 列表没读全 → 去重不可靠，跳过本轮（下次进入再补）
    if (this.skins.some((s) => s.source === "microsoft" && s.skin === skin)) return; // 已缓存
    try {
      const item = await invoke<LocalSkin>("save_local_skin", {
        name: t("skin.studio.currentSkinName"),
        b64: skin,
        variant: this.officialModel.toLowerCase() === "slim" ? "SLIM" : "CLASSIC",
        source: "microsoft",
      });
      this.skins = [item, ...this.skins.filter((s) => s.id !== item.id)];
    } catch (e) {
      console.error("自动缓存当前皮肤失败:", e); // 静默：衣柜里已有旧缓存，不打断预览
    }
  },

  selectSkin(key: string) {
    if (this.selectedKey === key) return;
    this.selectedKey = key;
  },

  async pickSkinFile() {
    const path = await open({
      multiple: false,
      filters: [{ name: t("skin.studio.pngFilter"), extensions: ["png"] }],
    });
    if (!path || typeof path !== "string") return;
    this.pendingSkinPath = path;
    this.skinModelDialog = true;
  },

  /** 确认模型 → 直接上传到账号（不落本地衣柜）；成功后作为历史缓存进衣柜供日后切回。 */
  async confirmSaveSkin(variant: "CLASSIC" | "SLIM") {
    this.skinModelDialog = false;
    const path = this.pendingSkinPath;
    this.pendingSkinPath = "";
    const acc = msAccount.value;
    if (!path || !this.premium || !acc || this.applyBusy) return;
    this.applyBusy = true;
    try {
      await invoke("upload_skin_file", { filePath: path, variant });
      showToast(t("skin.studio.uploadedPending"));
      await this.settleAfterSkinChange(acc.uuid);
    } catch (e) {
      showError(t("skin.studio.changeFailed", { error: String(e) }), {
        copyable: true,
        title: t("skin.studio.errorTitle"),
      });
    } finally {
      this.applyBusy = false;
    }
  },

  async removeSkin(item: LocalSkin) {
    try {
      this.skins = await invoke<LocalSkin[]>("remove_local_skin", { id: item.id });
      if (this.selectedKey === item.id) {
        this.selectedKey = "official";
      }
      showToast(t("skin.studio.deleted", { name: item.name }));
    } catch (e) {
      showError(t("skin.studio.deleteFailed", { error: String(e) }), {
        copyable: true,
        title: t("skin.studio.errorTitle"),
        actions: [
          {
            label: t("common.action.retry"),
            primary: true,
            run: () => this.removeSkin(item),
          },
        ],
      });
    }
  },

  async applySelected() {
    const local = this.selectedLocal;
    const acc = msAccount.value;
    if (!this.premium || !acc || !local || this.applyBusy) return;
    this.applyBusy = true;
    try {
      await invoke("upload_skin_file", { filePath: local.path, variant: local.variant });
      showToast(t("skin.studio.uploadedPending"));
      await this.settleAfterSkinChange(acc.uuid);
    } catch (e) {
      showError(t("skin.studio.applyFailed", { error: String(e) }), {
        copyable: true,
        title: t("skin.studio.errorTitle"),
        actions: [
          {
            label: t("common.action.retry"),
            primary: true,
            run: () => this.applySelected(),
          },
        ],
      });
    } finally {
      this.applyBusy = false;
    }
  },

  /** 换肤成功后的共同尾段：等 Mojang 生效 → 重拉账号皮肤 → 缓存进衣柜 → 回落实时卡 → 同步启动页头像。 */
  async settleAfterSkinChange(uuid: string) {
    await delay(APPLY_SETTLE_MS);
    await this.loadOfficial();
    await this.cacheCurrent();
    this.selectedKey = "official";
    void renderPremiumAvatar(uuid, true); // 启动页头像同步
    showToast(t("skin.studio.skinUpdated"));
  },

  async saveSkinFile() {
    const acc = msAccount.value;
    if (!this.premium || !acc) return;
    const path = await save({
      defaultPath: `${acc.name}.png`,
      filters: [{ name: t("skin.studio.pngFilter"), extensions: ["png"] }],
    });
    if (!path) return;
    try {
      await invoke("save_skin", { uuid: acc.uuid, path });
      showToast(t("skin.studio.skinSaved"));
    } catch (e) {
      showError(t("skin.studio.saveFailed", { error: String(e) }), {
        copyable: true,
        title: t("skin.studio.errorTitle"),
        actions: [
          {
            label: t("common.action.retry"),
            primary: true,
            run: () => this.saveSkinFile(),
          },
        ],
      });
    }
  },

  // ── 披风操作 ──

  /** 点选披风：立即预览并切换（id = "" 表示移除披风）；失败回滚预览。 */
  async useCape(id: string) {
    if (!this.premium || this.capeBusy || id === this.selectedCapeId) return;
    const prev = this.selectedCapeId;
    this.selectedCapeId = id;
    this.capeBusy = true;
    try {
      if (id) await invoke("set_active_cape", { capeId: id });
      else await invoke("disable_cape");
      if (id) {
        const alias = this.capes.find((c) => c.id === id)?.alias || id.slice(0, 8);
        showToast(t("skin.studio.capeSwitched", { name: alias }));
      } else {
        showToast(t("skin.studio.capeRemoved"));
      }
      await this.loadCapes();
    } catch (e) {
      this.selectedCapeId = prev;
      showError(t("skin.studio.capeFailed", { error: String(e) }), {
        copyable: true,
        title: t("skin.studio.errorTitle"),
        actions: [
          {
            label: t("common.action.retry"),
            primary: true,
            run: () => this.useCape(id),
          },
        ],
      });
    } finally {
      this.capeBusy = false;
    }
  },
});
