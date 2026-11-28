<script setup lang="ts">
// 资源详情页（Mod/光影/资源包/数据包/整合包共用）：信息卡片 + 版本文件列表（本地过滤）；仅 Mod 显示 ModLoader 筛选。
import { ref, computed, watch, onMounted, onBeforeUnmount } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AlertCircle, Download, LoaderCircle, RefreshCw } from "@lucide/vue";
import { showToast } from "../utils/toast";
import { showError } from "../store/app/alert";
import { errMsg, formatError } from "../utils/error";
import { resourceDetail, openResourceDetail } from "../store/community/resourceDetail";
import { goBack } from "../store/app/nav";
import { selectedVersion } from "../store/app/selectedVersion";
import { VERSION_DIR_CHANGED, type VersionDirChanged } from "../store/app/events";
import { dlShowReleaseOnly } from "../store/config/downloaderConfig";
import {
  communityTranslationEnabled, fetchVersions, fetchCommunityDetails,
  type CommunityVersion, type CommunityDependencyItem, type CommunityDisplayItem,
} from "../store/community/communitySearch";
import {
  downloadFiles,
  registerTaskName,
  type DownloadFileInput,
  type DownloadFileKind,
} from "../store/task/download";
import {
  resolveModDependencies,
  fetchModConflicts,
  type ModDependencyPlan,
  type ModConflictReport,
} from "../store/community/modDependencies";
import type { InstanceModTarget } from "../types/game/InstanceModTarget";
import {
  dependencyAppliesToInstance,
  fileAppliesToInstance,
  hasDeclaredIncompat,
} from "../store/community/resourceCompat";
import {
  displayLoadersForKind,
  isModLoader,
  loaderAccepts,
  versionMatchesKind,
} from "../store/community/resourceKind";
import { installModpack } from "../store/task/modpack";
import { getInstanceModTarget } from "../store/task/versionSetup";
import { LOADER_TITLES } from "../utils/versionMeta";
import ModDownloadDialog from "../component/download/ModDownloadDialog.vue";
import VersionFilterSelect from "../component/download/VersionFilterSelect.vue";
import { fmtCount, fmtSize, fmtModDate } from "../utils/format";
import IconImage from "../component/common/IconImage.vue";

const item = computed(() => resourceDetail.item);
const { t } = useI18n();
const router = useRouter();

const sourceLabel = computed(() =>
  item.value?.source === "modrinth" ? "Modrinth" : "CurseForge",
);

function loaderName(loader: string): string {
  return loader === "vanilla"
    ? t("download.tabs.vanilla")
    : LOADER_TITLES[loader] ?? loader;
}

// ── 版本列表 ──────────────────────────────

const versions = ref<CommunityVersion[]>([]);
const loadingVersions = ref(false);
const versionError = ref("");
/** 原始版本存在，但全部因下载入口/加载器不匹配被过滤。 */
const versionsHiddenForTab = ref(false);

// ── 版本列表过滤（本地过滤，选项从已加载版本收集）──

const loaderFilter = ref("");
const gameVersionFilter = ref("");

const LOADER_ORDER = ["fabric", "forge", "neoforge", "quilt"];

const isMod = computed(() => resourceDetail.backTab === "mod");

const loaderOptions = computed(() => {
  const values = Array.from(new Set(
    versions.value.flatMap(version => displayLoadersForKind(resourceDetail.backTab, version.loaders)),
  ));
  return values.sort(
    (a, b) => (LOADER_ORDER.indexOf(a) + 1 || 99) - (LOADER_ORDER.indexOf(b) + 1 || 99),
  );
});

/** 主版本归类：提取开头 major.minor（"1.20.4"→"1.20"）；无该前缀（快照等）原样保留 */
function majorVersion(gv: string): string {
  const m = gv.match(/^\d+\.\d+/);
  return m ? m[0] : gv;
}

/** 快照/预发布/候选等非正式版本标记（26w14a、26.3-snapshot-8、-pre1、-rc1、candidate） */
const SNAPSHOT_RE = /\d{2}w\d{2}|snapshot|pre\d*|rc\d*|candidate/i;
const isSnapshot = (gv: string): boolean => SNAPSHOT_RE.test(gv);

/** 参与筛选/展示的 MC 版本：开启“仅正式版”时剔除快照/预发布，与项目级版本范围口径一致。 */
function shownGameVersions(gvs: string[]): string[] {
  return dlShowReleaseOnly.value ? gvs.filter(gv => !isSnapshot(gv)) : gvs;
}

const loaderScopedVersions = computed(() =>
  loaderFilter.value
    ? versions.value.filter(v => v.loaders.some(loader => loaderAccepts(loaderFilter.value, loader)))
    : versions.value,
);

const gameVersionOptions = computed(() =>
  Array.from(new Set(loaderScopedVersions.value.flatMap(v => shownGameVersions(v.game_versions).map(majorVersion)))).sort((a, b) =>
    new Intl.Collator(undefined, { numeric: true }).compare(b, a),
  ),
);

// 切换加载器后已选版本若不在新选项内则清空（继承值除外：列表搜索已按其过滤）
watch(gameVersionOptions, () => {
  if (
    gameVersionFilter.value &&
    gameVersionFilter.value !== inheritedVersion &&
    !gameVersionOptions.value.includes(gameVersionFilter.value)
  ) {
    gameVersionFilter.value = "";
  }
});

const TYPE_ORDER: Record<string, number> = { release: 0, beta: 1, alpha: 2, unknown: 3 };

/** 解析正式版 MC 版本数值段；快照/预发布不参与，避免 "24w14a" 冒充最高版本。 */
function releaseSegments(gv: string): number[] | null {
  if (isSnapshot(gv)) return null;
  const m = gv.match(/^(\d+(?:\.\d+)*)/);
  return m ? m[1].split(".").map(Number) : null;
}

/** 数值段降序比较；段数不足按 0 补足（"1.20" 与 "1.20.0" 相等）。 */
function compareSegmentsDesc(a: number[], b: number[]): number {
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const diff = (b[i] ?? 0) - (a[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/** 文件声明的最高正式版 MC 版本；只有快照时返回 null（排在正式版之后）。 */
function highestReleaseVersion(version: CommunityVersion): number[] | null {
  let best: number[] | null = null;
  for (const gv of shownGameVersions(version.game_versions)) {
    const seg = releaseSegments(gv);
    if (!seg) continue;
    if (!best || compareSegmentsDesc(seg, best) < 0) best = seg;
  }
  return best;
}

/** 版本文件按支持的最高 MC 版本降序；同版本内 release > beta > alpha，再按发布时间降序。 */
const filteredVersions = computed(() => {
  const rows = versions.value
    .filter(
      v =>
        (!loaderFilter.value || v.loaders.some(loader => loaderAccepts(loaderFilter.value, loader))) &&
        (!gameVersionFilter.value || shownGameVersions(v.game_versions).some(gv => majorVersion(gv) === gameVersionFilter.value)),
    )
    .map(version => ({ version, mc: highestReleaseVersion(version) }));
  rows.sort((a, b) => {
    if (a.mc || b.mc) {
      if (!a.mc) return 1;
      if (!b.mc) return -1;
      const byGame = compareSegmentsDesc(a.mc, b.mc);
      if (byGame !== 0) return byGame;
    }
    const byType = (TYPE_ORDER[a.version.version_type] ?? 3) - (TYPE_ORDER[b.version.version_type] ?? 3);
    if (byType !== 0) return byType;
    return b.version.date_published.localeCompare(a.version.date_published);
  });
  return rows.map(row => row.version);
});

function loaderLabel(l: string): string {
  return l.charAt(0).toUpperCase() + l.slice(1);
}

// ── 自定义下拉（WebView2 无法限制原生 select 弹出层高度）──

const openFilter = ref<"" | "loader" | "game">("");

/** 下拉浮层选项：筛选值 → 展示名。 */
const loaderSelectOptions = computed(() =>
  loaderOptions.value.map(l => ({ value: l, label: loaderLabel(l) })),
);
const gameSelectOptions = computed(() =>
  gameVersionOptions.value.map(gv => ({ value: gv, label: gv })),
);

function pickLoader(l: string) {
  loaderFilter.value = l;
  openFilter.value = "";
}

function pickGame(gv: string) {
  gameVersionFilter.value = gv;
  openFilter.value = "";
}

function onDocPointerDown() {
  openFilter.value = "";
}

function onDocKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape" || !openFilter.value) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  openFilter.value = "";
}

let unlistenVersionDir: UnlistenFn | null = null;
let modsChangedWhileHidden = false;

/** 目标实例的 mods/ 增删后重新判定冲突；不可见时置脏，恢复可见再刷新。 */
function onVersionDirChanged(ev: { payload: VersionDirChanged }) {
  if (ev.payload.version !== selectedVersion.value) return;
  if (!ev.payload.kinds.includes("mods")) return;
  if (document.visibilityState !== "visible") {
    modsChangedWhileHidden = true;
    return;
  }
  void loadConflicts(true);
}

function onVisibilityChange() {
  if (document.visibilityState !== "visible" || !modsChangedWhileHidden) return;
  modsChangedWhileHidden = false;
  void loadConflicts(true);
}

onMounted(() => {
  document.addEventListener("pointerdown", onDocPointerDown);
  window.addEventListener("keydown", onDocKeydown, true);
  document.addEventListener("visibilitychange", onVisibilityChange);
  void listen<VersionDirChanged>(VERSION_DIR_CHANGED, onVersionDirChanged)
    .then(unlisten => {
      if (depDisposed) unlisten();
      else unlistenVersionDir = unlisten;
    })
    .catch(() => {
      // 事件系统不可用时仅失去自动刷新，手动刷新仍可用。
    });
});
onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocPointerDown);
  window.removeEventListener("keydown", onDocKeydown, true);
  document.removeEventListener("visibilitychange", onVisibilityChange);
  unlistenVersionDir?.();
  unlistenVersionDir = null;
  // 离开详情页后，未完成的依赖解析不得再自动提交/导航。
  depDisposed = true;
  versionsSeq++;
  depsSeq++;
  depDialogSeq++;
  conflictSeq++;
  depDialogOpen.value = false;
  depSubmitting.value = false;
  pendingMain = null;
});

/** 当前选中的版本文件 id（切换项目/重新加载时重置）；须在 loadVersions 前声明：immediate watch 在 setup 期即调用。 */
const selectedVersionId = ref("");

/** 当前选中的版本文件（过滤/重新加载后可能为 null）。 */
const selectedFile = computed(() =>
  versions.value.find(v => v.version_id === selectedVersionId.value) ?? null,
);

/** 作者禁止第三方分发时 CurseForge 不返回 downloadUrl：只能去来源页面手动下载。 */
const selectedWithoutDownload = computed(
  () => !!selectedFile.value && !selectedFile.value.download_url,
);

/** 版本请求序号：自增；响应返回时与当前值不符即过期（项目已切换/返回/重试）→ 丢弃。 */
let versionsSeq = 0;
/** 组件是否已卸载；immediate watcher 在 setup 期就会调用 loadConflicts，声明必须早于 watcher。 */
let depDisposed = false;

async function loadVersions(refresh = false) {
  const seq = ++versionsSeq;
  const it = item.value;
  if (!it) return;
  // 先清空旧数据：否则依赖卡片会用旧列表 + 旧选中版本算出残留前置依赖
  versions.value = [];
  versionsHiddenForTab.value = false;
  loadingVersions.value = true;
  versionError.value = "";
  try {
    const all = await fetchVersions(it.source, it.id, refresh);
    if (seq !== versionsSeq) return; // 过期响应丢弃
    const applicable = all.filter(version =>
      versionMatchesKind(resourceDetail.backTab, version.loaders));
    versionsHiddenForTab.value = all.length > 0 && applicable.length === 0;
    // 仅显示正式版：剔除所有支持版本均为快照/预发布的文件（混版保留）
    versions.value = dlShowReleaseOnly.value
      ? applicable.filter(v => v.game_versions.some(gv => !isSnapshot(gv)))
      : applicable;
    // 刷新/重进后旧选中文件可能已被作者删除；清掉避免右侧残留与下载时报“文件不存在”。
    if (selectedVersionId.value && !versions.value.some(v => v.version_id === selectedVersionId.value)) {
      selectedVersionId.value = "";
      resourceDetail.pendingSelectedVersion = "";
    }
  } catch (e) {
    if (seq !== versionsSeq) return;
    versionError.value = errMsg(e);
  } finally {
    if (seq === versionsSeq) loadingVersions.value = false;
  }
}

/** 项目切换：应用 store 同步来的筛选/选中版本（进入/返回还原）后加载版本；列表页选项为完整 ID、详情页为主版本归类，继承时做 major 归一化并记录 inheritedVersion（watch 清空逻辑跳过它）。 */
let inheritedVersion: string | null = null;

function onItemChange() {
  versionsSeq++; // 项目切换：使在飞请求过期（返回恢复路径不调用 loadVersions）
  loaderFilter.value = resourceDetail.pendingLoader;
  inheritedVersion = majorVersion(resourceDetail.pendingGameVersion);
  gameVersionFilter.value = inheritedVersion;
  selectedVersionId.value = resourceDetail.pendingSelectedVersion;
  void loadInstanceTarget();
  if (resourceDetail.pendingVersions !== null) {
    // 返回还原：恢复快照，不重新请求
    versions.value = resourceDetail.pendingVersions;
    versionError.value = resourceDetail.pendingVersionError;
    loadingVersions.value = false;
    resourceDetail.pendingVersions = null;
    resourceDetail.pendingVersionError = "";
    void loadConflicts();
    return;
  }
  void loadVersions();
  void loadConflicts();
}

// ── 前置依赖（选中版本的 required 依赖；不同版本可能不同）──

const depItems = ref<CommunityDependencyItem[]>([]);
const loadingDeps = ref(false);
const depError = ref("");

/** 选中版本的 required 依赖项目 ID（去重保序）。 */
const selectedDepIds = computed(() => {
  const v = versions.value.find(x => x.version_id === selectedVersionId.value);
  if (!v) return [];
  const seen = new Set<string>();
  const order: string[] = [];
  for (const dep of v.dependencies) {
    if (dep.relation === "required" && dep.project_id && !seen.has(dep.project_id)) {
      seen.add(dep.project_id);
      order.push(dep.project_id);
    }
  }
  return order;
});

// ── 目标实例（下载落点）：依赖与冲突都必须按该实例的 MC / loader 判定 ──

const instanceTarget = ref<InstanceModTarget | null>(null);
/** `instanceTarget` 对应的版本名；选中实例切换后旧 target 不得参与判定。 */
const instanceTargetFor = ref("");
let instanceTargetKey = "";
let instanceTargetSeq = 0;

async function loadInstanceTarget(force = false) {
  const version = selectedVersion.value;
  if (!version || !isMod.value) {
    instanceTargetSeq++;
    instanceTargetKey = "";
    instanceTargetFor.value = "";
    instanceTarget.value = null;
    return;
  }
  if (!force && version === instanceTargetKey && instanceTarget.value) return;
  const seq = ++instanceTargetSeq;
  instanceTargetKey = version;
  try {
    const target = await getInstanceModTarget(version);
    if (seq !== instanceTargetSeq || depDisposed) return;
    if (selectedVersion.value === version) {
      instanceTarget.value = target;
      instanceTargetFor.value = version;
      // 目标 loader/MC 到达后重新判定一次；已发出请求时由 conflictKey 去重。
      void loadConflicts();
    }
  } catch {
    if (seq !== instanceTargetSeq) return;
    if (selectedVersion.value === version) {
      instanceTargetKey = "";
      instanceTargetFor.value = "";
      instanceTarget.value = null;
    }
  }
}

/** 依赖项目是否适用于目标实例；文件自身不适用（loader 或 MC）时按声明原样展示。 */
function depAppliesToInstance(dep: CommunityDependencyItem): boolean {
  if (instanceTargetFor.value !== selectedVersion.value) return true;
  const target = instanceTarget.value;
  const file = selectedFile.value;
  if (!target || !file) return true;
  return dependencyAppliesToInstance(dep.loaders, file, target);
}

/** 选中文件能否装入目标实例（loader + MC）；任一未知时视为可用，避免请求未返回时闪隐。 */
const selectedFileApplies = computed(() => {
  const target = instanceTarget.value;
  const file = selectedFile.value;
  if (!target || instanceTargetFor.value !== selectedVersion.value || !file) return true;
  return fileAppliesToInstance(file, target);
});

const requiredIdSet = computed(() => new Set(selectedDepIds.value));
const requiredItems = computed(() =>
  depItems.value.filter(d => requiredIdSet.value.has(d.id) && depAppliesToInstance(d)),
);

// ── 已安装冲突（作者声明 incompatible 且目标实例 mods/ 中确实存在）──

const conflictReport = ref<ModConflictReport | null>(null);
const loadingConflicts = ref(false);
const conflictError = ref("");
const conflicts = computed(() => conflictReport.value?.conflicts ?? []);
const conflictNotices = computed(() => conflictReport.value?.notices ?? []);
const conflictErrors = computed(() => {
  const errors = conflictReport.value?.errors ?? [];
  return conflictError.value ? [conflictError.value, ...errors] : errors;
});
let conflictSeq = 0;
let conflictKey = "";

async function loadConflicts(force = false) {
  if (depDisposed) return;
  const key = `${item.value?.key ?? ""}:${selectedVersionId.value}:${selectedVersion.value}:${isMod.value}`;
  if (!force && key === conflictKey) return;
  const seq = ++conflictSeq;
  conflictReport.value = null;
  conflictError.value = "";
  const it = item.value;
  const versionId = selectedVersionId.value;
  const targetVersion = selectedVersion.value;
  if (!it || !versionId || !targetVersion || !isMod.value) {
    conflictKey = "";
    loadingConflicts.value = false;
    return;
  }
  if (!selectedFileApplies.value) {
    conflictKey = "";
    loadingConflicts.value = false;
    return;
  }
  const file = selectedFile.value;
  if (file && !hasDeclaredIncompat(file.dependencies)) {
    // 没有 incompatible 声明时后端只会白跑一次主项目版本查询；直接记 key 防止重复进入。
    conflictKey = key;
    loadingConflicts.value = false;
    return;
  }
  conflictKey = key;
  loadingConflicts.value = true;
  try {
    const report = await fetchModConflicts({
      source: it.source,
      projectId: it.id,
      versionId,
      versionDirName: targetVersion,
    });
    if (seq !== conflictSeq) return;
    conflictReport.value = report;
  } catch (e) {
    if (seq !== conflictSeq) return;
    conflictError.value = errMsg(e);
    conflictKey = "";
  } finally {
    if (seq === conflictSeq) loadingConflicts.value = false;
  }
}

/** 面板可见性：声明了前置、详情加载中/失败、或存在已安装冲突时都应保持可见。 */
const depsPanelVisible = computed(() => {
  if (!selectedVersionId.value) return false;
  if (conflicts.value.length > 0 || conflictErrors.value.length > 0 || depError.value) return true;
  if (loadingDeps.value) return selectedDepIds.value.length > 0;
  if (missingDepDetailCount.value > 0) return true;
  return requiredItems.value.length > 0;
});

const displayedDepCount = computed(() =>
  loadingDeps.value ? selectedDepIds.value.length : requiredItems.value.length,
);

/** 请求的依赖项中未能取到详情的数量（项目被删/不可见时 API 会静默省略）。 */
const missingDepDetailCount = computed(() => {
  if (loadingDeps.value || depError.value) return 0;
  return Math.max(0, requiredIdSet.value.size - depItems.value.length);
});

/** 依赖请求序号：自增；响应返回时与当前值不符即过期（选中版本已切换/返回）→ 丢弃。 */
let depsSeq = 0;

async function loadDeps(force = false) {
  const seq = ++depsSeq;
  const it = item.value;
  const reqIds = selectedDepIds.value;
  if (!it || reqIds.length === 0) {
    depItems.value = [];
    depError.value = "";
    loadingDeps.value = false;
    // 无前置时快照没有归属版本，留到下次选择会串到别的版本。
    resourceDetail.pendingDepItems = null;
    resourceDetail.pendingDepError = "";
    return;
  }
  // 返回还原：恢复依赖快照，不重新请求（手动刷新时跳过快照强制回源）
  if (!force && resourceDetail.pendingDepItems !== null) {
    depItems.value = resourceDetail.pendingDepItems;
    depError.value = resourceDetail.pendingDepError;
    loadingDeps.value = false;
    resourceDetail.pendingDepItems = null;
    resourceDetail.pendingDepError = "";
    return;
  }
  resourceDetail.pendingDepItems = null;
  resourceDetail.pendingDepError = "";
  loadingDeps.value = true;
  depError.value = "";
  try {
    const items = await fetchCommunityDetails(it.source, reqIds, force);
    if (seq !== depsSeq) return; // 过期响应丢弃
    depItems.value = items;
  } catch (e) {
    if (seq !== depsSeq) return;
    depError.value = errMsg(e);
  } finally {
    if (seq === depsSeq) loadingDeps.value = false;
  }
}

/** 面板刷新：目标实例元数据、前置详情与已安装冲突都重新取。 */
function refreshDependencyPanel() {
  if (loadingDeps.value || loadingConflicts.value) return;
  void loadInstanceTarget(true);
  void loadDeps(true);
  void loadConflicts(true);
}

watch(selectedDepIds, () => void loadDeps());
watch(selectedVersionId, () => void loadConflicts());
watch(selectedVersion, () => {
  void loadInstanceTarget();
  void loadConflicts();
});
watch(isMod, () => {
  void loadInstanceTarget();
  void loadConflicts();
});
watch(item, onItemChange, { immediate: true });

function pickVersion(v: CommunityVersion) {
  selectedVersionId.value = v.version_id;
  // 同步回 store：随 tab 切换卸载重建时用 pendingSelectedVersion 恢复选中态
  resourceDetail.pendingSelectedVersion = v.version_id;
}

function depToItem(d: CommunityDependencyItem): CommunityDisplayItem {
  const src = item.value?.source ?? "modrinth";
  return {
    id: d.id,
    title: d.title,
    title_zh: null,
    mcmod_id: null,
    description: d.description,
    translated: d.translated,
    loaders: d.loaders,
    game_version_range: null,
    author: "",
    downloads: 0,
    updated_at: null,
    icon_url: d.icon_url,
    page_url: "",
    category: null,
    source: src,
    key: `${src}:${d.id}`,
  };
}

// ── 提交文件下载 ──────────────────────────────

/** 来源 tab → 落盘目录 kind；modpack 走链式安装（isModpack 分流），无单文件下载。 */
const fileKind = computed<DownloadFileKind | null>(() => {
  switch (resourceDetail.backTab) {
    case "mod": return "mod";
    case "shader": return "shader";
    case "resourcepack": return "resourcepack";
    case "datapack": return "datapack";
    case "worlds": return "world";
    default: return null;
  }
});

const isModpack = computed(() => resourceDetail.backTab === "modpack");

/** 下载按钮提示：未选版本 > 无直链 > 正常下载/安装。 */
const downloadButtonTooltip = computed(() => {
  if (!selectedVersionId.value) return t("download.detail.selectVersionTooltip");
  if (selectedWithoutDownload.value) return t("download.detail.noDirectDownload");
  return isModpack.value
    ? t("download.detail.installTooltip")
    : t("download.detail.downloadTooltip");
});

async function submitModpackInstall() {
  if (!selectedVersionId.value) {
    showToast(t("download.detail.selectFileFirst"));
    return;
  }
  const v = versions.value.find(x => x.version_id === selectedVersionId.value);
  if (!v) {
    showToast(t("download.detail.selectedFileMissing"));
    return;
  }
  try {
    await installModpack({
      name: item.value?.title ?? v.filename,
      source: {
        type: "online",
        url: v.download_url,
        sha1: v.sha1 ?? "",
        size: v.file_size,
      },
      // 平台项目图标：随请求透传，安装期持久化到版本目录供列表展示
      icon_url: item.value?.icon_url ?? null,
    });
    showToast(t("download.detail.modpackSubmitted"));
    backAfterSubmit();
  } catch (e) {
    showToast(errMsg(e));
  }
}

/** 提交成功后退出一层：依赖链进入的回到上一层详情，从列表进入的回到列表（多层依赖无需逐级手动返回）。 */
function backAfterSubmit() {
  goBack();
}

// ── 前置 Mod 确认 ──

const depDialogOpen = ref(false);
const depDialogLoading = ref(false);
const depResolving = ref(false);
const depDialogUpdating = ref(false);
const depSubmitting = ref(false);
const depDialogError = ref("");
const depPlan = ref<ModDependencyPlan | null>(null);
const depModName = ref("");
const depModVersion = ref("");
const depModFilename = ref("");
const depModLoaders = ref<string[]>([]);
const depVersionOverrides = ref<Record<string, string>>({});
let depDialogSeq = 0;
let pendingMain: { version: CommunityVersion; target: string } | null = null;

function mainDownloadFile(
  version: CommunityVersion,
  replace: string[] = [],
): DownloadFileInput {
  return { kind: "mod", url: version.download_url, filename: version.filename, replace };
}

/** 与后端 InstalledFiles::normalize_mod_filename 保持一致：跨源文件名只差分隔符时仍能匹配。 */
function normalizeModFilename(name: string): string {
  return name.toLowerCase().replace(/[+_ ]/g, "-").replace(/-+/g, "-");
}

/** 主 Mod 同项目的旧文件：项目版本列表 ∩ 目标 mods 目录文件名（跨源分隔符归一化）。 */
function mainReplacements(version: CommunityVersion): string[] {
  const installed = new Map(
    (depPlan.value?.installed_files ?? []).map(name => [normalizeModFilename(name), name]),
  );
  const known = new Set(versions.value.map(item => item.filename));
  const replacements = new Set<string>();
  for (const name of known) {
    if (name === version.filename) continue;
    const actual = installed.get(normalizeModFilename(name));
    if (actual) replacements.add(actual);
  }
  return [...replacements];
}

/** 替换列表不能误删本批新文件（例如两个依赖恰好同名）。 */
function normalizeReplacement(files: DownloadFileInput[]): DownloadFileInput[] {
  const newNames = new Set(files.map(file => file.filename));
  return files.map(file => ({
    ...file,
    replace: Array.from(new Set(
      (file.replace ?? []).filter(name => name !== file.filename && !newNames.has(name)),
    )),
  }));
}

function dependencyDownloadFiles(selectedOptional: string[]): DownloadFileInput[] {
  if (!pendingMain) return [];
  const files = [mainDownloadFile(pendingMain.version, mainReplacements(pendingMain.version))];
  const seen = new Set(files.map(file => file.filename));
  for (const dep of depPlan.value?.items ?? []) {
    if (!dep.url || !dep.filename || dep.error || dep.installed) continue;
    const included =
      dep.root_optional === null
      || (dep.root_optional !== null && selectedOptional.includes(dep.root_optional));
    if (!included || seen.has(dep.filename)) continue;
    seen.add(dep.filename);
    files.push({
      kind: "mod",
      url: dep.url,
      filename: dep.filename,
      replace: dep.installed_files.filter(name => name !== dep.filename),
    });
  }
  return normalizeReplacement(files);
}

async function submitModFiles(files: DownloadFileInput[], name: string) {
  const target = pendingMain?.target;
  if (!target || files.length === 0 || depSubmitting.value || depDisposed) return;
  depSubmitting.value = true;
  try {
    const taskId = await downloadFiles(target, normalizeReplacement(files));
    if (taskId) registerTaskName(taskId, name);
    if (depDisposed) return;
    showToast(t("download.detail.submitted"));
    depDialogOpen.value = false;
    pendingMain = null;
    backAfterSubmit();
  } catch (e) {
    if (!depDisposed) showToast(errMsg(e));
  } finally {
    depSubmitting.value = false;
  }
}

/** 应用解析结果：丢弃已不在计划里的版本覆盖。 */
function applyDependencyPlan(plan: ModDependencyPlan) {
  depPlan.value = plan;
  const projectIds = new Set(plan.items.map(dep => dep.project_id));
  depVersionOverrides.value = Object.fromEntries(
    Object.entries(depVersionOverrides.value).filter(([projectId]) => projectIds.has(projectId)),
  );
}

/** 弹窗内重试/切换前置版本：按当前覆盖重新解析，不自动提交。 */
async function resolveDependencyPlan(initial: boolean) {
  const request = pendingMain;
  if (!request) return;
  const seq = ++depDialogSeq;
  if (initial) {
    depDialogLoading.value = true;
    depDialogError.value = "";
  } else {
    depDialogUpdating.value = true;
  }
  try {
    const plan = await resolveModDependencies({
      source: item.value?.source ?? "modrinth",
      projectId: item.value?.id ?? "",
      versionId: request.version.version_id,
      versionDirName: request.target,
      versionOverrides: depVersionOverrides.value,
    });
    if (depDisposed || seq !== depDialogSeq || !depDialogOpen.value) return;
    applyDependencyPlan(plan);
  } catch (e) {
    if (depDisposed || seq !== depDialogSeq || !depDialogOpen.value) return;
    if (initial) depDialogError.value = errMsg(e);
    else showToast(errMsg(e));
  } finally {
    if (seq === depDialogSeq) {
      depDialogLoading.value = false;
      depDialogUpdating.value = false;
    }
  }
}

/** 前置解析失败后手动重试；保留已选版本覆盖。 */
function retryDependencyPlan() {
  if (depDialogLoading.value || depDialogUpdating.value || !depDialogOpen.value) return;
  void resolveDependencyPlan(true);
}

/**
 * 前置解析过程与结果始终在确认弹窗内展示：先打开 loading 弹窗再解析，
 * 即使没有前置也停留在“没有前置 Mod”结果页，由用户确认提交。
 */
async function openModDownload(version: CommunityVersion, target: string) {
  if (depDisposed) return;
  pendingMain = { version, target };
  depModName.value = item.value?.title ?? version.filename;
  depModVersion.value = version.version_number;
  depModFilename.value = version.filename;
  depModLoaders.value = version.loaders;
  depPlan.value = null;
  depDialogError.value = "";
  depDialogLoading.value = true;
  depDialogOpen.value = true;
  depVersionOverrides.value = {};
  depDialogUpdating.value = false;
  depSubmitting.value = false;
  const seq = ++depDialogSeq;
  depResolving.value = true;
  try {
    const plan = await resolveModDependencies({
      source: item.value?.source ?? "modrinth",
      projectId: item.value?.id ?? "",
      versionId: version.version_id,
      versionDirName: target,
      versionOverrides: {},
    });
    if (depDisposed || seq !== depDialogSeq || !depDialogOpen.value) return;
    applyDependencyPlan(plan);
    depDialogOpen.value = true;
  } catch (e) {
    if (depDisposed || seq !== depDialogSeq || !depDialogOpen.value) return;
    depDialogError.value = errMsg(e);
    depDialogOpen.value = true;
  } finally {
    if (seq === depDialogSeq) {
      depDialogLoading.value = false;
      depResolving.value = false;
    }
  }
}

function onDependencyVersionChange(projectId: string, versionId: string) {
  depVersionOverrides.value = {
    ...depVersionOverrides.value,
    [projectId]: versionId,
  };
  void resolveDependencyPlan(false);
}

function onDependencyConfirm(selectedOptional: string[]) {
  if (depDialogUpdating.value || depSubmitting.value || !depPlan.value) return;
  void submitModFiles(dependencyDownloadFiles(selectedOptional), depModName.value);
}

function onDependencyMainOnly() {
  if (depDialogUpdating.value || depSubmitting.value || !pendingMain || !depPlan.value) return;
  void submitModFiles(
    [mainDownloadFile(pendingMain.version, mainReplacements(pendingMain.version))],
    depModName.value,
  );
}

const downloadSubmitting = ref(false);

/** ModLoader / MC 版本不匹配用弹窗（只给提示的 toast 会被漏看）；可直接跳实例设置处理。 */
function showDownloadCompatError(title: string, message: string, targetVersion: string) {
  showError(message, {
    title,
    actions: [
      {
        label: t("download.detail.openVersionSettings"),
        primary: true,
        run: () => { void router.push({ path: "/version-setup", query: { name: targetVersion } }); },
      },
      { label: t("common.action.close"), run: () => undefined },
    ],
  });
}

/** 提交选中版本下载任务（任务中心自动注册）；落盘 versions/{版本目录}/{kind_dir}/{filename}（后端兜底 _{n} 冲突）。 */
async function submitDownload() {
  if (downloadSubmitting.value) return;
  downloadSubmitting.value = true;
  try {
    await submitDownloadInner();
  } finally {
    downloadSubmitting.value = false;
  }
}

async function submitDownloadInner() {
  if (depDisposed) return;
  // 作者禁止第三方分发时文件无直链：modpack 安装与普通下载都不能走任务，提前给出明确提示
  if (selectedFile.value && !selectedFile.value.download_url) {
    showToast(t("download.detail.noDirectDownload"));
    return;
  }
  // modpack 分流：不要求已选启动版本（安装器自建版本目录）
  if (isModpack.value) {
    await submitModpackInstall();
    return;
  }
  // 任何未满足条件都明确提示（不静默）
  if (!selectedVersionId.value) {
    showToast(t("download.detail.selectFileFirst"));
    return;
  }
  const kind = fileKind.value;
  if (!kind) {
    showToast(t("download.detail.unsupportedType"));
    return;
  }
  const v = versions.value.find(x => x.version_id === selectedVersionId.value);
  if (!v) {
    showToast(t("download.detail.selectedFileMissing"));
    return;
  }
  // 下载目标版本：实时读取 selectedVersion（下载完成后 onTaskDone 自动选中更新）
  const targetVersion = selectedVersion.value;
  if (!targetVersion) {
    showToast(t("download.detail.noTargetVersion"));
    return;
  }
  if (kind === "mod") {
    let target;
    try {
      target = await getInstanceModTarget(targetVersion);
    } catch (e) {
      showToast(errMsg(e));
      return;
    }
    if (!isModLoader(target.loader)) {
      showDownloadCompatError(
        t("download.detail.modLoaderRequiredTitle"),
        `${item.value?.title ?? v.filename} · ${t("download.detail.modLoaderRequired", { loader: loaderName(target.loader) })}`,
        targetVersion,
      );
      return;
    }
    if (!v.loaders.some(loader => loaderAccepts(target.loader, loader))) {
      showDownloadCompatError(
        t("download.detail.modLoaderMismatchTitle"),
        `${item.value?.title ?? v.filename} · ${t("download.detail.modLoaderMismatch", {
          loader: loaderName(target.loader),
          loaders: v.loaders.map(loaderName).join(" / ") || "—",
        })}`,
        targetVersion,
      );
      return;
    }
    if (depDisposed) return;
    if (!v.game_versions.includes(target.mc_version)) {
      const versions = v.game_versions.slice(0, 5).join(" / ")
        + (v.game_versions.length > 5 ? " …" : "");
      showDownloadCompatError(
        t("download.detail.modGameVersionMismatchTitle"),
        `${item.value?.title ?? v.filename} · ${t("download.detail.modGameVersionMismatch", {
          mc: target.mc_version,
          versions: versions || "—",
        })}`,
        targetVersion,
      );
      return;
    }
    await openModDownload(v, targetVersion);
    return;
  }
  try {
    // 诊断：invoke 超时兜底（5s）——后端命令卡住时给出反馈而非静默"无反应"
    const taskId = await Promise.race([
      downloadFiles(targetVersion, [
        { kind, url: v.download_url, filename: v.filename },
      ]),
      new Promise<never>((_, rej) =>
        setTimeout(() => rej(new Error(t("download.detail.requestTimeout"))), 5000),
      ),
    ]);
    // 注册资源名：完成提示显示 "下载任务[资源名]完成"（fallback 文件名）
    if (taskId) {
      registerTaskName(taskId, item.value?.title ?? v.filename);
    }
    showToast(t("download.detail.submitted"));
    backAfterSubmit();
  } catch (e) {
    showToast(errMsg(e));
  }
}

// ── 操作 ──────────────────────────────

function gotoSource() {
  const it = item.value;
  if (!it?.page_url) return;
  openUrl(it.page_url).catch(() => showToast(t("download.detail.openLinkFailed")));
}

function gotoMcmod() {
  const it = item.value;
  if (!it) return;
  const searchTitle = communityTranslationEnabled.value ? (it.title_zh ?? it.title) : it.title;
  const url = it.mcmod_id
    ? `https://www.mcmod.cn/class/${it.mcmod_id}.html`
    : `https://search.mcmod.cn/s?key=${encodeURIComponent(searchTitle)}&mold=0`;
  openUrl(url).catch(() => showToast(t("download.detail.openLinkFailed")));
}

async function copyName() {
  const it = item.value;
  if (!it) return;
  const text = communityTranslationEnabled.value && it.title_zh ? `${it.title_zh} | ${it.title}` : it.title;
  try {
    await navigator.clipboard.writeText(text);
    showToast(t("download.detail.copiedName"));
  } catch {
    showToast(t("download.detail.copyFailed"));
  }
}
</script>

<template>
  <div class="md-page">
  <div v-if="item" class="md">
    <!-- 项目切换动画：内容整体淡入淡出（同路由 push 不触发路由级过渡） -->
    <Transition name="md-switch" mode="out-in">
      <div :key="item.key" class="md-body">
        <!-- 卡片一：项目信息（样式与外部 item 类似） -->
        <div class="md-card">
          <div class="md-row">
            <IconImage :src="item.icon_url" :size="40" />
            <span class="md-info">
              <span class="md-title-row">
                <span class="md-title">{{ communityTranslationEnabled && item.title_zh ? item.title_zh : item.title }}</span>
                <span v-if="communityTranslationEnabled && item.title_zh" class="md-title-en"> | {{ item.title }}</span>
                <span class="md-badge" :class="item.source">{{ sourceLabel }}</span>
                <span class="md-sub">{{ t("download.detail.downloads", { count: fmtCount(item.downloads) }) }}</span>
              </span>
            </span>
          </div>
          <div class="md-desc">{{ communityTranslationEnabled ? (item.translated ?? item.description) : item.description }}</div>

          <div class="md-actions">
            <button class="md-btn" @click="gotoSource">{{ t("download.detail.gotoSource", { source: sourceLabel }) }}</button>
            <!-- 光影包/地图存档无 MC百科条目，隐藏跳转 -->
            <button
              v-if="item.category !== 'shader' && item.category !== 'worlds'"
              class="md-btn"
              @click="gotoMcmod"
            >{{ t("download.detail.gotoMcmod") }}</button>
            <button class="md-btn" @click="copyName">{{ t("download.detail.copyName") }}</button>
          </div>
        </div>

        <div class="md-cards-row">
          <div class="md-card md-versions">
            <div class="mv-head">
              <span class="mv-title">{{ t("download.detail.versionFiles") }}<span v-if="!loadingVersions" class="mv-count"> | {{ t("download.detail.versionCount", { count: filteredVersions.length }, filteredVersions.length) }}</span></span>

              <VersionFilterSelect
                v-if="isMod"
                :open="openFilter === 'loader'"
                :label="loaderFilter ? loaderLabel(loaderFilter) : t('download.search.allLoaders')"
                :all-label="t('download.search.allLoaders')"
                :options="loaderSelectOptions"
                :selected="loaderFilter"
                @update:open="openFilter = $event ? 'loader' : ''"
                @select="pickLoader"
              />

              <VersionFilterSelect
                :open="openFilter === 'game'"
                :label="gameVersionFilter || t('download.search.allVersions')"
                :all-label="t('download.search.allVersions')"
                :options="gameSelectOptions"
                :selected="gameVersionFilter"
                @update:open="openFilter = $event ? 'game' : ''"
                @select="pickGame"
              />

              <span class="mv-spacer"></span>

              <button
                type="button"
                class="mv-icon-btn"
                :disabled="loadingVersions"
                v-tooltip="t('download.detail.refreshVersions')"
                :aria-label="t('download.detail.refreshVersions')"
                @click="loadVersions(true)"
              >
                <RefreshCw :size="16" :class="{ 'mv-spin': loadingVersions }" />
              </button>

              <button
                v-if="fileKind || isModpack"
                class="mv-dl"
                :class="{ 'mv-dl-disabled': !selectedVersionId || selectedWithoutDownload || depResolving }"
                :disabled="!selectedVersionId || selectedWithoutDownload || depResolving"
                v-tooltip="downloadButtonTooltip"
                :aria-label="downloadButtonTooltip"
                @click="submitDownload"
              >
                <LoaderCircle v-if="depResolving" :size="16" class="mv-spin" aria-hidden="true" />
                <Download v-else :size="16" aria-hidden="true" />
              </button>
            </div>

            <div v-if="selectedWithoutDownload" class="mv-dl-note">
              <span>{{ t("download.detail.noDirectDownload") }}</span>
              <button type="button" class="mv-dl-note-btn" @click="gotoSource">
                {{ t("download.detail.gotoSource", { source: sourceLabel }) }}
              </button>
            </div>

            <div v-if="loadingVersions" class="mv-state">
              <div class="spinner"></div>
              <p class="mv-state-text">{{ t("download.detail.loadingVersions") }}</p>
            </div>
            <button
              v-else-if="versionError"
              type="button"
              class="md-load-error"
              v-tooltip="t('download.detail.clickToRetry')"
              @click="loadVersions(true)"
            >
              <AlertCircle :size="20" />
              <span class="md-load-error-info">
                <span class="md-load-error-title">{{ t("download.detail.versionsLoadFailed") }}</span>
                <span class="md-load-error-desc">{{ versionError }}</span>
              </span>
            </button>
            <div v-else-if="versions.length === 0" class="mv-state">{{ t(versionsHiddenForTab ? "download.detail.noFilesForTab" : "download.detail.noVersions") }}</div>
            <div v-else class="mv-list">
              <div v-if="filteredVersions.length === 0" class="mv-state">{{ t("download.detail.noMatchingVersions") }}</div>
              <div
                v-for="v in filteredVersions"
                :key="v.version_id"
                class="mv-item"
                :class="{ active: v.version_id === selectedVersionId }"
                @click="pickVersion(v)"
              >
                <div class="mv-info">
                  <span class="mv-name-row">
                    <span class="mv-name" v-tooltip="v.filename">{{ v.filename }}</span>
                    <span v-if="v.version_type === 'alpha'" class="mv-type alpha">alpha</span>
                    <span v-else-if="v.version_type === 'beta'" class="mv-type beta">beta</span>
                  </span>
                  <span
                    class="mv-tags"
                    :class="{ 'mv-tags-split': displayLoadersForKind(resourceDetail.backTab, v.loaders).length > 0 }"
                  >
                    <span v-if="displayLoadersForKind(resourceDetail.backTab, v.loaders).length > 0" class="mv-tags-loaders">
                      <span v-for="ld in displayLoadersForKind(resourceDetail.backTab, v.loaders)" :key="ld" class="md-ld" :class="`md-ld-${ld}`">{{ ld }}</span>
                    </span>
                    <span class="mv-tags-gv">
                      <span v-if="v.game_version_range" class="mv-tag gv">{{ v.game_version_range }}</span>
                      <template v-else>
                        <span v-for="gv in v.game_versions.slice(0, 3)" :key="gv" class="mv-tag gv">{{ gv }}</span>
                      </template>
                    </span>
                  </span>
                </div>
                <span class="mv-meta">
                  <span class="mv-date">{{ fmtModDate(v.date_published) }}</span>
                  <span class="mv-size">{{ fmtSize(v.file_size) }}</span>
                </span>
              </div>
            </div>
          </div>

          <Transition name="mv-side">
            <div v-if="selectedVersionId && depsPanelVisible" class="md-card md-deps-card">
              <div class="mv-head">
                <span class="mv-title">{{ t("download.detail.dependencies") }}<span v-if="!loadingDeps" class="mv-count"> | {{ t("download.detail.dependencyCount", { count: displayedDepCount }, displayedDepCount) }}</span></span>
                <span class="mv-spacer"></span>
                <button
                  type="button"
                  class="mv-icon-btn"
                  :disabled="loadingDeps || loadingConflicts"
                  v-tooltip="t('download.detail.refreshDependencies')"
                  @click="refreshDependencyPanel"
                >
                  <RefreshCw :size="13" :class="{ 'mv-spin': loadingDeps || loadingConflicts }" />
                </button>
              </div>
              <div v-if="!loadingConflicts && conflicts.length > 0" class="md-dep-warn">
                <div class="md-dep-warn-title">{{ t("download.detail.incompatibleTitle", { count: conflicts.length }) }}</div>
                <div v-for="d in conflicts" :key="d.project_id" class="md-dep-warn-item">{{ d.name }}<span v-if="d.version_label" class="md-dep-warn-ver"> · {{ d.version_label }}</span></div>
                <div class="md-dep-warn-tip">{{ t("download.detail.incompatibleTip") }}</div>
              </div>
              <div v-if="conflictNotices.length > 0" class="md-dep-note">
                <span v-for="notice in conflictNotices" :key="notice">{{ formatError(notice).summary }}</span>
              </div>
              <button
                v-if="!loadingConflicts && conflictErrors.length > 0"
                type="button"
                class="md-dep-partial"
                v-tooltip="formatError(conflictErrors[0]).full"
                @click="refreshDependencyPanel"
              >
                <AlertCircle :size="14" />
                <span>{{ t("download.detail.conflictsPartial", { count: conflictErrors.length }) }}</span>
              </button>
              <button
                v-if="missingDepDetailCount > 0"
                type="button"
                class="md-dep-partial"
                v-tooltip="t('download.detail.clickToRetry')"
                @click="loadDeps(true)"
              >
                <AlertCircle :size="14" />
                <span>{{ t("download.detail.depsPartial", { count: missingDepDetailCount }) }}</span>
              </button>
              <div v-if="loadingDeps" class="md-dep-state">
                <div class="spinner"></div>
                <p class="md-dep-state-text">{{ t("download.detail.loadingDependencies") }}</p>
              </div>
              <button
                v-else-if="depError"
                type="button"
                class="md-dep-error"
                v-tooltip="t('download.detail.clickToRetry')"
                @click="loadDeps(true)"
              >
                <AlertCircle :size="18" />
                <span class="md-dep-error-info">
                  <span class="md-dep-error-title">{{ t("download.detail.depsLoadFailed") }}</span>
                  <span class="md-dep-error-desc">{{ depError }}</span>
                </span>
              </button>
              <div v-else-if="depItems.length === 0 && conflicts.length === 0" class="md-dep-empty">{{ t("download.detail.noDependencies") }}</div>
              <div v-else class="md-dep-list">
                <div v-for="d in requiredItems" :key="d.id" class="md-dep-item" @click="openResourceDetail(
                  depToItem(d),
                  { loader: loaderFilter, gameVersion: gameVersionFilter, selectedVersion: selectedVersionId },
                  { versions, versionError, depItems, depError },
                )">
                  <IconImage :src="d.icon_url" :size="28" />
                  <span class="md-dep-info">
                    <span class="md-dep-title">{{ d.title }}</span>
                    <span class="md-dep-desc">{{ communityTranslationEnabled ? (d.translated ?? d.description) : d.description }}</span>
                  </span>
                </div>
              </div>
            </div>
          </Transition>
        </div>
      </div>
    </Transition>
  </div>
  <div v-else class="md-empty">{{ t("download.detail.notFound") }}</div>

  <Teleport to="body">
    <ModDownloadDialog
      v-model="depDialogOpen"
      :loading="depDialogLoading"
      :updating="depDialogUpdating"
      :submitting="depSubmitting"
      :error="depDialogError"
      :mod-name="depModName"
      :mod-version="depModVersion"
      :mod-filename="depModFilename"
      :mod-loaders="depModLoaders"
      :plan="depPlan"
      @confirm="onDependencyConfirm"
      @main-only="onDependencyMainOnly"
      @version-change="onDependencyVersionChange"
      @retry="retryDependencyPlan"
    />
  </Teleport>
  </div>
</template>

<style scoped>
.md-page {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.md {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 20px 24px;
  overflow-y: auto;
  overflow-x: hidden;
  background: var(--surface-page);
  scrollbar-gutter: stable;
}

.md-switch-enter-active,
.md-switch-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}
.md-switch-enter-from {
  opacity: 0;
  transform: translateY(6px);
}
.md-switch-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

.md-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
}
.md-body > .md-card { flex-shrink: 0; }

/* 双卡片并列：左版本文件 flex:1，右前置依赖固定 240px（动态出现） */
.md-cards-row {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 12px;
}

.md-versions {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.md-versions .mv-head { flex-shrink: 0; }
.md-versions .mv-list,
.md-versions .mv-state { flex: 1; min-height: 0; }

/* 右侧前置依赖卡片：宽度展开动画（目标值写在 enter-to/leave-from） */
.mv-side-enter-active,
.mv-side-leave-active {
  transition: width 0.22s ease, opacity 0.18s ease;
  overflow: hidden;
}
.mv-side-enter-from,
.mv-side-leave-to {
  width: 0;
  opacity: 0;
}
.mv-side-enter-to,
.mv-side-leave-from {
  width: 240px;
  opacity: 1;
}
.md-deps-card {
  flex: 0 0 240px;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.md-deps-card .mv-head { flex-shrink: 0; }
.md-deps-card .md-dep-list,
.md-deps-card .md-dep-state,
.md-deps-card .md-dep-empty {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin-top: 12px;
}

.md-card {
  background: var(--surface-card);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 14px 16px;
}

.md-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.md-info {
  flex: 1;
  min-width: 0;
}

.md-title-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.md-title {
  font-weight: 600;
  font-size: var(--fs-md);
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.md-title-en {
  font-size: var(--fs-xs);
  font-weight: 400;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ModLoader 徽章（与外部 item 同款；专属色随主题变量切换） */
.md-ld {
  flex-shrink: 0;
  font-size: var(--fs-2xs);
  font-weight: 600;
  padding: 0 5px;
  border-radius: var(--radius-sm);
  background: var(--color-brand-bg);
  color: var(--color-brand);
}
.md-ld-fabric {
  color: var(--ld-fabric);
}
.md-ld-forge {
  color: var(--ld-forge);
}
.md-ld-neoforge {
  color: var(--ld-neoforge);
}
.md-ld-quilt {
  color: var(--ld-quilt);
}
.md-ld-optifine {
  color: var(--ld-optifine);
}

/* 徽章底色：color-mix 需 WebKitGTK ≥ 2.42（Ubuntu 22.04 的 2.40 不支持）；不支持则降级为纯文字色徽章 */
@supports (color-mix(in srgb, red 12%, transparent)) {
  .md-ld-fabric {
    background: color-mix(in srgb, var(--ld-fabric) 12%, transparent);
  }
  .md-ld-forge {
    background: color-mix(in srgb, var(--ld-forge) 12%, transparent);
  }
  .md-ld-neoforge {
    background: color-mix(in srgb, var(--ld-neoforge) 12%, transparent);
  }
  .md-ld-quilt {
    background: color-mix(in srgb, var(--ld-quilt) 12%, transparent);
  }
  .md-ld-optifine {
    background: color-mix(in srgb, var(--ld-optifine) 12%, transparent);
  }
}

.md-badge {
  flex-shrink: 0;
  font-size: var(--fs-2xs);
  padding: 1px 6px;
  border-radius: var(--radius-md);
  color: var(--text-inverse);
  font-weight: 600;
}
.md-badge.modrinth { background: var(--color-modrinth); }
.md-badge.curseforge { background: var(--color-curseforge); }

.md-sub {
  flex-shrink: 0;
  margin-left: auto;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.md-desc {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border-subtle, var(--border-default));
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  line-height: 1.6;
}

.md-actions {
  display: flex;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border-subtle, var(--border-default));
}

.md-btn {
  flex: 1;
  padding: 8px 0;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--surface-card);
  color: var(--text-secondary);
  font-size: var(--fs-base);
  font-weight: 500;
  cursor: pointer;
  transition: all var(--dur-fast);
}

.md-btn:hover {
  border-color: var(--color-brand);
  color: var(--color-brand);
  background: var(--color-brand-bg);
}

/* ── 版本列表 ── */

.md-versions {
  padding: 12px 16px;
}

.mv-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-subtle, var(--border-default));
}

.mv-title {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
}

.mv-count {
  font-size: var(--fs-xs);
  font-weight: 400;
  color: var(--text-tertiary);
}

.mv-spacer {
  flex: 1;
}

.mv-dl {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 1px solid var(--color-brand);
  border-radius: var(--radius-sm);
  background: var(--color-brand);
  color: var(--text-inverse);
  cursor: pointer;
  transition: all var(--dur-fast);
  outline: none;
}
.mv-dl > svg { flex: none; }
.mv-dl:hover:not(.mv-dl-disabled) {
  filter: brightness(1.12);
}
.mv-dl:active:not(.mv-dl-disabled) {
  filter: brightness(0.95);
}
.mv-dl-disabled {
  background: var(--surface-hover);
  border-color: var(--border-default);
  color: var(--text-tertiary);
  cursor: not-allowed;
}

.mv-icon-btn {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-card);
  color: var(--text-secondary);
  cursor: pointer;
  transition: color var(--dur-fast), border-color var(--dur-fast), background var(--dur-fast);
}

.mv-icon-btn:hover:not(:disabled) {
  color: var(--color-brand);
  border-color: var(--color-brand);
  background: var(--color-brand-bg);
}

.mv-icon-btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.mv-spin {
  animation: spin 0.7s linear infinite;
}

.mv-dl-note {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0 0 8px;
  padding: 7px 10px;
  border: 1px solid var(--color-warning);
  border-radius: var(--radius-sm);
  background: var(--color-warning-bg);
  color: var(--color-warning);
  font-size: var(--fs-sm);
}

.mv-dl-note span {
  flex: 1;
  min-width: 0;
}

.mv-dl-note-btn {
  flex: none;
  padding: 3px 10px;
  border: 1px solid currentColor;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-warning);
  font-size: var(--fs-sm);
  font-weight: 600;
  cursor: pointer;
  transition: background var(--dur-fast);
}

.mv-dl-note-btn:hover {
  background: var(--color-warning-bg);
}

/* ── 自定义下拉 ── */

/* expand 动画由共享骨架（styles/transitions.css）提供；元素通过内联 --expand-h 注入目标高度 */

.mv-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 30px 0;
  font-size: var(--fs-base);
  color: var(--text-tertiary);
}
.mv-state-text { font-size: var(--fs-base); color: var(--text-tertiary); }

.md-load-error {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  width: 100%;
  padding: 20px 16px;
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-page);
  text-align: left;
  font: inherit;
  cursor: pointer;
  transition: border-color var(--dur-fast), background var(--dur-fast);
}

.md-load-error:hover {
  border-color: var(--color-brand);
  background: var(--color-brand-bg);
}

.md-load-error > svg {
  flex: none;
  color: var(--color-danger);
}

.md-load-error-info {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.md-load-error-title {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
}

.md-load-error-desc {
  font-size: var(--fs-sm);
  color: var(--color-danger);
  word-break: break-word;
}

.spinner {
  width: 32px; height: 32px;
  border: 3px solid var(--border-default); border-top-color: var(--color-brand);
  border-radius: 50%; animation: spin 0.7s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }

.mv-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 8px 0 -4px;
}

.mv-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  box-shadow: inset 0 0 0 1px transparent;
  transition: background 0.1s, box-shadow 0.1s;
}
.mv-item:hover { background-color: var(--surface-hover); }
.mv-item.active {
  background: var(--color-brand-bg);
  box-shadow: inset 0 0 0 1px var(--color-brand);
}

.mv-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.mv-name-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.mv-name {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-sm);
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mv-type {
  flex: none;
  padding: 0 6px;
  border-radius: var(--radius-md);
  font-size: var(--fs-2xs);
  font-weight: 700;
  line-height: 1.6;
  text-transform: uppercase;
}
.mv-type.alpha { background: var(--color-danger-bg); color: var(--color-danger); }
.mv-type.beta { background: var(--color-warning-bg); color: var(--color-warning); }

.mv-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.mv-tags-loaders,
.mv-tags-gv {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  flex-wrap: wrap;
}
.mv-tags-split .mv-tags-gv {
  margin-left: auto;
  justify-content: flex-end;
}

.mv-tag {
  flex-shrink: 0;
  font-size: var(--fs-2xs);
  font-weight: 600;
  padding: 0 5px;
  border-radius: var(--radius-sm);
}
.mv-tag.gv { background: var(--surface-hover); color: var(--text-secondary); }

.mv-meta {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
}
.mv-date { font-size: var(--fs-xs); color: var(--text-tertiary); }
.mv-size { font-size: var(--fs-xs); color: var(--text-secondary); }

/* ── 前置依赖（右侧面板，选中版本后出现）── */
.md-dep-empty,
.md-dep-state {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 14px 10px;
  text-align: center;
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-tertiary);
  background: var(--surface-page);
  border-radius: var(--radius-sm);
}
.md-dep-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.md-dep-state.err {
  color: var(--color-danger);
  font-size: var(--fs-sm);
}

.md-dep-error {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-top: 12px;
  padding: 10px;
  border: 1px solid var(--color-danger);
  border-radius: var(--radius-sm);
  background: var(--color-danger-bg);
  text-align: left;
  font: inherit;
  cursor: pointer;
  overflow-y: auto;
  transition: filter var(--dur-fast);
}

.md-dep-error:hover {
  filter: brightness(1.04);
}

.md-dep-error > svg {
  flex: none;
  color: var(--color-danger);
  margin-top: 1px;
}

.md-dep-error-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.md-dep-error-title {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--color-danger);
}

.md-dep-error-desc {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  word-break: break-word;
}

.md-dep-partial {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
  padding: 6px 8px;
  border: 1px dashed var(--color-warning);
  border-radius: var(--radius-sm);
  background: var(--color-warning-bg);
  color: var(--color-warning);
  font: inherit;
  font-size: var(--fs-xs);
  text-align: left;
  cursor: pointer;
}

.md-dep-partial > svg {
  flex: none;
}

.md-dep-warn {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin-top: 12px;
  padding: 10px 12px;
  border: 1px solid var(--color-danger);
  border: 1px solid color-mix(in srgb, var(--color-danger) 35%, transparent);
  border-radius: var(--radius-sm);
  background: var(--color-danger-bg);
}
.md-dep-warn-title {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--color-danger);
}
.md-dep-warn-item {
  font-size: var(--fs-sm);
  color: var(--color-danger);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.md-dep-warn-ver {
  font-weight: 400;
  opacity: 0.8;
}
.md-dep-note {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 8px;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.md-dep-warn-tip {
  font-size: var(--fs-xs);
  color: var(--color-danger);
  opacity: 0.85;
}
.md-dep-state-text {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}
.md-dep-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.md-dep-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--surface-page);
  border: 1px solid var(--border-default);
  cursor: pointer;
  transition: border-color var(--dur-fast), background var(--dur-fast);
}
.md-dep-item:hover {
  border-color: var(--color-brand);
  background: var(--surface-hover);
}
.md-dep-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.md-dep-title {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.md-dep-desc {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.md-empty {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--fs-base);
  color: var(--text-tertiary);
  background: var(--surface-page);
}
</style>
