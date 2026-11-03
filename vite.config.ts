import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [vue()],

  // 浏览器基线：WebView2 / WKWebView / WebKitGTK 2.40（≈ Safari 16.4）。
  // 显式固定，勿依赖 Vite 默认 target——它随主版本上移会静默抬高前端基线。
  build: {
    target: ["chrome111", "safari16.4"],
    // vendor 单独成块：入口 chunk 低于 Vite 默认 500 kB 告警阈值；Tauri 本地加载，拆块无网络收益。
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: "vendor", test: /node_modules/ },
            { name: "page-launch", test: /[\\/]src[\\/]pages[\\/]Launch\.vue$/ },
            { name: "page-download", test: /[\\/]src[\\/]pages[\\/]Download\.vue$/ },
            { name: "page-settings", test: /[\\/]src[\\/]pages[\\/]Settings\.vue$/ },
            { name: "page-task", test: /[\\/]src[\\/]pages[\\/]DownloadDetail\.vue$/ },
            { name: "pages", test: /[\\/]src[\\/]pages[\\/]/ },
            // tools 保持动态导入，不并入公共组件块；否则任意页面引用公共组件会把全部工具一起预加载。
            { name: "components", test: /[\\/]src[\\/]component[\\/](?!tools[\\/])/ },
            { name: "stores", test: /[\\/]src[\\/]store[\\/]/ },
            { name: "i18n", test: /[\\/]src[\\/]i18n[\\/]/ },
          ],
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
