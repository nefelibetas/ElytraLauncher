// 首帧主题：在 CSS/应用挂载前设置 data-mode/data-theme，避免骨架与真实主题闪变。
(function () {
  try {
    var mode = localStorage.getItem("elytra.appearanceMode") || "system";
    var dark = mode === "dark";
    if (mode === "system" && window.matchMedia) {
      dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    }
    document.documentElement.dataset.mode = dark ? "dark" : "light";
    var theme = localStorage.getItem("elytra.theme");
    if (theme) document.documentElement.dataset.theme = theme;
  } catch (e) {
    // localStorage 不可用时交给应用启动流程
  }
})();
