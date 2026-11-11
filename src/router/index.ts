import { createRouter, createWebHashHistory } from "vue-router";
// 顶级 Tab 也按路由懒加载：首屏只加载当前页；页面/ store 引 router 时不再形成
// router→页面→store→router 的静态环，未访问的重页面（资源中心/设置）不进入首包。
const Launch = () => import("../pages/Launch.vue");
const Download = () => import("../pages/Download.vue");
const Settings = () => import("../pages/Settings.vue");
const DownloadDetail = () => import("../pages/DownloadDetail.vue");
const ResourceDetail = () => import("../pages/ResourceDetail.vue");
const VersionSelect = () => import("../pages/VersionSelect.vue");
const VersionSetup = () => import("../pages/VersionSetup.vue");
const CustomThemeEditor = () => import("../pages/CustomThemeEditor.vue");
const SkinStudio = () => import("../pages/SkinStudio.vue");
const WorldPreview = () => import("../pages/WorldPreview.vue");

declare module "vue-router" {
  interface RouteMeta {
    /** 页面切换动画名（slide-left / slide-right / route） */
    transition?: string;
  }
}

/** 路由路径常量（store/组件跳转共用，避免散落重复字符串）。 */
export const ROUTE_DOWNLOAD = "/download";
export const ROUTE_RESOURCE_DETAIL = "/resource-detail";

let tabPrefetchStarted = false;

/** 首帧后预取顶级 tab 页面的整个动态 import 图，点击时不再等待 chunk 解析。 */
export function prefetchTabPages(): void {
  if (tabPrefetchStarted) return;
  tabPrefetchStarted = true;
  void Promise.allSettled([Download(), Settings(), DownloadDetail()]);
}

const routes = [
  { path: "/", name: "launch", component: Launch },
  { path: ROUTE_DOWNLOAD, name: "download", component: Download },
  { path: "/settings", name: "settings", component: Settings },
  { path: "/download-detail", name: "download-detail", component: DownloadDetail },
  { path: ROUTE_RESOURCE_DETAIL, name: "resource-detail", component: ResourceDetail },
  { path: "/version-select", name: "version-select", component: VersionSelect },
  { path: "/version-setup", name: "version-setup", component: VersionSetup },
  { path: "/custom-theme", name: "custom-theme", component: CustomThemeEditor },
  { path: "/skin-studio", name: "skin-studio", component: SkinStudio },
  { path: "/world-preview", name: "world-preview", component: WorldPreview },
];

const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

const routeLevel: Record<string, number> = {
  launch: 0,
  download: 0,
  settings: 0,
  "download-detail": 0,
  "resource-detail": 1,
  "version-select": 1,
  "custom-theme": 1,
  "skin-studio": 1,
  "version-setup": 2,
  "world-preview": 3,
  // 独立窗口页面：不与主窗口路由互跳，等级独立（无动画耦合）
};

router.beforeEach((to, from) => {
  const toLevel = routeLevel[to.name as string] ?? 0;
  const fromLevel = routeLevel[from.name as string] ?? 0;
  if (toLevel > fromLevel) {
    to.meta.transition = "slide-left"; // 进入更深层级：新页从右侧推入
  } else if (toLevel < fromLevel) {
    to.meta.transition = "slide-right"; // 返回上级：新页从左侧推入
  } else {
    // 同级（顶级 tab / 详情间切换）：桌面应用用淡入，不做整页横滑。
    to.meta.transition = "route";
  }
});

export default router;
