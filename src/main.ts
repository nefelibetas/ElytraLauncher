import { createApp } from "vue";
import App from "./App.vue";
import { i18n, initI18n } from "./i18n";
import router from "./router";
import { applyTheme } from "./store/app/theme";
import tooltip from "./utils/tooltip";
import "./styles/tokens.css";
import "./styles/transitions.css";

// 挂载前应用主题与语言，避免首帧闪白/闪中文
applyTheme();
initI18n();

const app = createApp(App);
// 全局 v-tooltip 指令（替换 UA title tooltip）
app.directive("tooltip", tooltip);
app.use(i18n);
app.use(router);

/** 应用首帧绘制后让骨架淡出并移除；骨架独立于 #app，Vue 挂载不会直接清掉它。 */
function hideBootSkeleton() {
  const el = document.getElementById("boot-splash");
  if (!el || el.classList.contains("bs-hide")) return;
  const fade = () => {
    if (!el.isConnected || el.classList.contains("bs-hide")) return;
    el.classList.add("bs-hide");
    window.setTimeout(() => el.remove(), 220);
  };
  // 两帧确保真实页面已经上屏；窗口不可见时 rAF 可能暂停，用定时器兜底。
  requestAnimationFrame(() => requestAnimationFrame(fade));
  window.setTimeout(fade, 500);
}

// 首帧静态骨架留在 index.html；等初始路由组件就绪再挂载，避免 App 已挂载但 router-view 空白。
let mounted = false;
function mountApp() {
  if (mounted) return;
  mounted = true;
  app.mount("#app");
  hideBootSkeleton();
}

router.isReady().then(mountApp, mountApp);
// 路由异常兜底：不让骨架永久停留。
window.setTimeout(mountApp, 4000);
