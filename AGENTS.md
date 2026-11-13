# Repository Guidelines

## Project Overview

Elytra Launcher — **Tauri v2（Rust 后端）+ Vue 3（TypeScript 前端）** 的 Minecraft 启动器。核心能力：多源游戏下载（官方/国内镜像）、ModLoader 安装（Fabric/Quilt/Forge/NeoForge/OptiFine）、微软 OAuth PKCE 登录与离线模式、皮肤衣柜（3D 预览 / 皮肤披风 / 本地收藏；第三方站点衣柜不内置，点头像用浏览器打开）、JDK 扫描与 managed runtime 下载、Mod 双源搜索与依赖解析、任务管理、启动与进程管理、局域网联机（easytier 内嵌）、工具箱（多线程下载器 / 种子预览 / 世界预览）。

后端 **161 个 `#[tauri::command]`**（21 个模块），前端 **10 个页面**（`src/pages/`），无状态管理库（模块级 `reactive()`/`ref()` 单例）。

## Architecture & Data Flow

```
前端 Vue3 (src/) ──invoke(snake_case)──▶ commands/（薄胶水，21 模块 / 161 命令）
    ▲                                        │ 读 persist 配置、调领域函数
    │ task-progress / ms-login-complete /    ▼
    │ game-exited 等（emit 顺序队列线程）  downloader/（传输层）  game/（内容与装配）
    └────── Result<T, String> 或裸值        task/  launcher/  auth/  jdk/  online/  config/  persist/
```

- **分层**：`commands/` 只做胶水（读配置 → 调领域函数 → `map_err` 展平 String）；`downloader/` 是纯传输层（URL → 校验过的字节，不认资源类型），**不得 `use crate::{task, launcher, jdk, game}`**；版本装配 / 资源 / ModLoader / 社区 / 整合包 / 更新一律放 `game/`；`config/` 纯数据；`persist/` 统一 TOML + secrets；`util/` 跨域纯函数。
- **事件推送**：`task/emit.rs` 的 `DISPATCH`（独立线程）**顺序派发全部事件**；`app.emit` 因 webview 未消费而阻塞只占派发线程，任务线程/进度锁永不碰 IPC。高频快照类（`task-progress` / `tooldl-progress` / `auto-task-progress`）按合并键只留最新一帧（payload 是完整快照，终态不会被丢；键含事件名，避免进度帧顶掉 `task-waiting-user` 的弹窗帧），其余 FIFO 直投——webview 卡住时内存有界。
- **锁**：全后端 `parking_lot`；**统一 inner→progress 锁序**（cancel/list/fail 先 inner 后 progress），`fail()` 前必须先 `drop` progress 锁；`cancel()` 的 DB 持久化移出锁；SQLite 写统一 2s 超时；`update_and_emit` 锁内更新 + clone 快照、锁外 emit。
- **终态收口唯一入口**：`task/progress.rs` 的 `set_done` / `set_error` / `set_cancelled`——统一「发终态事件 → 移除任务注册」，且 `set_error`/`set_cancelled` 取走此前 `set_cleanup_path` 登记的目录做**后台**整删（禁止在任务线程同步 `remove_dir_all` 大目录）。
- **任务 id** 带进程前缀（启动毫秒 + pid），避免重启后与崩溃残留快照行撞主键。
- **配置联动**：`gamedirs::set_active_game_dir` 写 downloader section 的 `minecraft_dir`，launch/downloader 都读该字段。
- **CurseForge Key**：运行时优先 `secrets.bin` 的用户 key，其次 `constants::CURSEFORGE_API_KEY`（构建期 `.env`）；用户 key 只经命令写入，不返回前端。
- **目录监听**：`game/version/watch.rs` 进程级单例（`notify`，非递归多目标）跟随激活目录监听 `versions/` 与聚焦版本的一级资源子目录；300ms 静默窗口合并后推 `versions-changed`（带全量列表）/ `version-dir-changed`；前端仅在窗口可见时应用（不可见置脏，`visibilitychange` 补拉）。
- **三态配置**：`game_source_mode`（统管版本列表 / 版本 json / 资源 / 支持库 / ModLoader 构件与安装器 / Mojang JDK runtime；OfficialOnly / MirrorOnly / Auto 默认，按健康度路由，元数据官方优先 4s 降级镜像）、`community_source_mode`（Modrinth vs MCIM）、`network_mode`（Auto 读系统代理 / System / Direct）。
- **natives 两代**：旧式（≤1.12）`process_natives` 解压到 `natives/<mc>/`（`.elytra-ready` 存 native 集合指纹，不一致即清空重建）并设 `-Djava.library.path`；新式（1.13+）native JAR 作普通库进 classpath。

## Key Directories

| 路径 | 职责与约束 |
| --- | --- |
| `src-tauri/src/downloader/` | 传输层。`fetcher/{api,core,routes,redirect}`（多源核心 / 镜像候选与健康度 / 黑洞 302 拒绝）、`batch.rs`（并发批处理，派发与 worker 均检查取消，`max_errors` 原子止损）、`segmented.rs`（HTTP Range 分段，段内断点续传）、`probe.rs`（TTFB/吞吐，TTL 缓存）、`path_lock.rs`（跨任务路径互斥）、`hash/progress/proxy/reuse`、`endpoints.rs`（网络拓扑单一事实源）。**镜像与不稳的官方 maven 统一压到 `MIRROR_CONCURRENCY = 8`，镜像源强制单连接；任何请求必带 `UA_DOWNLOAD`——统一用 `redirect::http()` 的全局 client，否则 BMCLAPI 302 到的节点 403**。`MIRROR_ROUTES` 行序即优先级（NeoForge 的 `…/releases/…` 在 BMCLAPI 侧须去掉 `/releases`，保留即 404） |
| `src-tauri/src/game/` | 内容与装配。`version/`（版本 JSON serde / 元数据 / 目录域 / 源重写 / 监听）、`resource/`（assets / library / extract，`TARGET_OS` 唯一实现）、`loader/`（`dispatch.rs` 入口 / `shared.rs` profile 合并 / `processors.rs` 按 installer 自己声明的 `install_profile.processors` 执行补丁链，不写死工具 / fabric·forge·neoforge·optifine）、`community/`（modrinth / curseforge / `dependencies.rs`）、`modpack/`（格式嗅探 / 各源清单解析 / 计划 / MRPACK·CurseForge 导出）、`updates/` |
| `src-tauri/src/task/` | 任务编排：`manager.rs`（单例 + 挂起决策）、`emit.rs`、`spawn.rs`、`progress.rs`（EmitLoop / 心跳 / `StepRef` / `set_*`）、`runner.rs`、`repair.rs`、`files.rs`、`jdk.rs`、`modpack.rs`（整合包安装编排）、`export.rs`（整合包导出任务：扫描/打包两阶段进度、临时文件 + 原子替换、失败/取消清理临时文件）、`store.rs`（SQLite 快照/恢复，崩溃残留标 interrupted）。**总百分比两种口径**：`update_progress` 默认字节占比；整合包用 `HeartbeatStats::percent_by_steps`（步骤数占比，其 `est_bytes` 含永不进分子的估算） |
| `src-tauri/src/tooldl/` | 工具箱下载器域（独立于 `task/`）：注册表 + 并发槽位 + 暂停/继续/取消/清除；复用 `downloader::segmented`；事件出口走 `task::EventSink`（不依赖 `AppHandle`，可注入测试），进度事件 `tooldl-progress` |
| `src-tauri/src/persist/` | `config.rs`（TOML section + 全局锁）、`store.rs`（低层 IO）、`instance.rs`（`versions/{id}/.elytra-instance.toml`，存在即该实例完全采用自己的 `[auto_tasks]`）、`secrets.rs`（DPAPI / Keychain / Secret Service + ChaCha20-Poly1305；原子写 `.tmp` + rename；列表读改写一律 `update_json_list`）。`util::config_dir()`：Windows `%APPDATA%\Elytra`、macOS `~/Library/Application Support/Elytra`、Linux `$XDG_DATA_HOME/Elytra`；dev/test 进程在 cargo target 下仍走 exe 旁保持隔离 |
| `src-tauri/src/launcher/` | `args.rs`（命令构建 / 占位符 / rules / classpath）、**`compat.rs`（版本 ↔ 所选 Java 兼容判定唯一入口）**、`env.rs`、`process.rs`（崩溃判定 + 启动取消闸门）、`auto_tasks/`（执行器内置在前、自定义在后，单项失败继续并汇总 / backup / record / crashpack / redact）、`script.rs`、`skin.rs`、`platform.rs`、`memory.rs`（`GcType::Custom` 不追加预设 GC 参数）、`model.rs` |
| `src-tauri/src/online/` | 联机：`service.rs`（单例；`stop()` 先释放端口再 `session.stop()`）、`host.rs`、`guest.rs`（`gen` 贯穿到回写，防停止后继续广播）、`session.rs`（no_tun + smoltcp + KCP 数据面，`enable_kcp_proxy=true` 必需，缺则传大数中断）、`scaffold/`（房间码 base34+模 7 / 帧编解码（长度字段外部可控，分配前校验 `MAX_BODY_LEN`）/ 玩家表 / `InfoServer` 必须被单例持有才会释放端口）、`mc_discover.rs`、`mc_advertise.rs`（guest 端模拟 LAN 广播） |
| `src-tauri/src/commands/` | 21 模块 / 161 命令，薄胶水：account(27) community(7) config(1) downloader(5) gamedirs(12) auto_tasks(4) jdk(5) launch(7) launch_options(3) launch_script(1) loader(6) map(15) seedmap(8) offline_account(5) online(6) server_status(1) skins(6) task(14) tooldl(9) version(15) version_icon(4) |
| `src-tauri/src/map/` `seedmap/` `vendor/cubiomes/` | 世界预览（只读 region：`region_io` / `render` / `tile_cache` / `mod_color`；不依赖 tauri）；种子预览（cubiomes 参数族，只做主世界）；cubiomes 为 xpple fork `d934de1`（MIT，不含 loot），Windows MSVC 用 clang-cl |
| `src-tauri/src/config/` `auth/` `jdk/` `util/` | 纯数据模型；微软 OAuth + 官方 API + 第三方皮肤站 + 本地收藏；JDK 扫描 + managed runtime 三源（Mojang all.json / Adoptium / Azul；zip/tar 目录条目名带尾 `/`，必须先跳过再校验路径；`on_attempt` 换源清零计数、`on_bytes` 以传输总长作进度分母）；`util` 纯函数（`path.rs` 的 `safe_relative_path`/`sha1_sub_dir`、`process.rs` 的 `NoWindow`、`dirs.rs` 的 `config_dir`） |
| `src/pages/` | 10 页：Launch / Download / DownloadDetail（顶级路由）/ ResourceDetail（依赖链进入的返回上一层详情）/ Settings / VersionSelect / VersionSetup / WorldPreview / CustomThemeEditor / SkinStudio（仅微软账号） |
| `src/store/` | 模块级单例，按域分包：`config/`、`task/`（`download` 状态机 + `ensureListen` 幂等 + `terminalTaskIds` 防幽灵）、`auth/`、`skin/`（`skinEditor` 的 64×64 像素缓冲刻意非响应式，视图只订阅 `revision`）、`community/`、`app/`（`events` 镜像 Rust `events.rs` / `theme` 四主题 / `launch` 代际号取消守卫）。**跨组件缓存必须放普通 `<script lang="ts">` 块或 store 模块级**（`<script setup>` 顶层变量随组件重建重置） |
| `src/types/` | ts-rs 自动生成（勿手改），按域分包；新类型登记 `src-tauri/src/ts_export.rs`，导出路径由 `src-tauri/.cargo/config.toml` 的 `TS_RS_EXPORT_DIR` 决定。**改 Rust 数据结构必须重跑生成** |
| `src/component/` `src/utils/` | 组件按域分目录（common / download / launch / settings / tools / version / version-setup / map），页面组件直接 import store，无 props 链；utils：`error.ts`（`errMsg()` 取首行）/ `toast.ts` / `focusNav.ts` / `format.ts` / `useConfigSection.ts` |

## Development Commands

```bash
pnpm tauri dev      # 主开发命令：vite :1420 + Rust dev 窗口（Rust 改动需手动重启）
pnpm tauri:build    # 本地发布构建（vue-tsc + lint:css + cargo release + bundle）
pnpm tauri build    # 仅打包（beforeBuildCommand 只跑 vite build；检查由 CI 的 check 承担）
pnpm dev            # 仅 vite dev server（:1420 strictPort）
pnpm build          # vue-tsc --noEmit + lint:css + vite build
pnpm lint:css       # CSS 兼容性检查（基线见 package.json browserslist）
pnpm i18n:check     # i18n 门禁：zh/en key 对称 + 占位符一致 + 后端 key + 前端静态 t() + 裸中文 + 未本地化错误首行（存量见 scripts/i18n-rust-plain-baseline.json；--update-baseline 重写）

cd src-tauri && cargo fmt                                # 改 Rust 后必跑
cd src-tauri && cargo check                              # 编译检查
cd src-tauri && cargo clippy --lib                       # lint（保持零代码警告）
cd src-tauri && cargo test --lib -- --test-threads=1     # 单元测试（串行防 persist/网络竞争，数量见 Testing）
cd src-tauri && cargo run --bin gen_ts --features gen-ts # 生成前端 TS 类型（不在测试里生成）
cd src-tauri && cargo test --test online_diag            # 联机集成测试（host/guest 回环，真实 easytier）
cd src-tauri && cargo test --test tool_download          # 工具箱下载器集成测试（本地 HTTP 服务）
cd src-tauri && cargo test --test e2e_download -- --ignored          # run_task 端到端（真实网络）
cd src-tauri && JAVA_HOME=<jdk> cargo test --lib -- --ignored --test-threads=1 matrix   # ModLoader 安装矩阵（报告落 target/e2e-loader/；ELY_E2E_CASES 选跑）
npx vue-tsc --noEmit    # 前端全量类型检查
```

**网络测试**（真实外网，离线环境可失败）：`fetch_asset_index_end_to_end`、`count_assets_all_end_to_end`、`global_client_uses_system_proxy_when_enabled`、`bmclapi_optifine_real_download`（`#[ignore]`）。

## Code Conventions & Common Patterns

### Rust 后端

- **格式化**：改 Rust 文件后必须 `cargo fmt`，`cargo fmt --check` 零差异（不手调换行）
- **命令命名**：snake_case、动词开头（`get_/save_/set_/fetch_/scan_/launch_/spawn_/start_/cancel_/remove_`）
- **返回**：可失败 → `Result<T, String>`；纯读取 → 裸 `T`/`Option<T>`；错误在模块边界 `.map_err(|e| e.to_string())`
- **错误消息**：**中文短文案首行 + `\n` + 技术细节**（如 `下载资源索引失败\n{e}`）；前端 toast/内联错误经 `errMsg()` 取首行，AlertDialog 显示全文
- **错误本地化**：用户可见首行走 `@i18n` 协议——`bail_key!` / `common_err_ctx_key!`（Result 路径）与 `keyed_msg!`（事件/状态等非 Result 路径，支持 `?k=v`）；首行 key、次行中文兜底、其后技术细节；`?k=` 必须与文案占位符完全一致（门禁校验）。**含内联捕获时必须走位置参数形态**（`common_err_ctx_key!("errors.x", "下载 {} 失败", x)`），裸 `"下载 {x} 失败"` 会退化成字面量输出。未 keyed 的 `common_err_ctx!`/`bail!` 已删除，别加回来
- **section 读写走宏**（`persist/accessors.rs` 的 `section_accessors!`）：生成 `xxx_config()` / `save_xxx_config()`（支持自定义默认形态）；新增 section 优先加进宏，不手写 `persist::read(SECT).unwrap_or_default()`
- **async 分布**：网络/IO 重命令 async；配置读写/本地小操作同步；重活（`remove_dir_all`、rename）走 `spawn_blocking`
- **注册**：新命令 = `commands/<file>.rs` 加 `#[tauri::command] pub fn` → `lib.rs` 顶部导入 → `generate_handler!` 列**裸名**（不写路径）；`commands/mod.rs` 只有 `pub mod`，re-export 不能代替
- **持久化**：`persist::read::<T>("section").unwrap_or_default()`；结构体实现 `Default`；save 前读改写
- **锁**：一律 `parking_lot`；锁序 inner→progress；`fail()` 前先 `drop` progress 锁；DB 写移出锁 + 2s 超时
- **模块组织**：`mod.rs` 只允许子模块声明 / `pub use` / 模块文档 / `#[cfg(test)]` / 条件编译，不许任何逻辑代码（哪怕一个小函数）；实现放同级命名文件
- **import**：禁止函数体内 `use`（测试模块顶部不受限）；方法体内最多两级路径，禁止三段及以上；同名 trait 用 `use xxx as _;`；分组 `std` → 外部 crate → `crate::`，组内字母序；`#[cfg]` 约束的 `use` 上提时保留属性
- **参数收敛**：超过 7 个抽上下文结构体（`TaskCtx`、`InstallCtx`、`PackMeta`、`LaunchRequest`、`AssetIndexRef`…）；**禁止 `#[allow(clippy::too_many_arguments)]`**
- **测试位置**：与实现同文件 `#[cfg(test)] mod tests`，不建 `tests.rs`；共用测试辅助放被测模块的 `#[cfg(test)] pub(super)` 区
- **下载**：`fetch_with`（3 次重试+退避）/ `fetch_fallback`（多源）/ `fetch_verified(_c)`（逐源 sha1，空 sha1 跳过）；`_c` 变体带取消（每块检查，Cancelled 不重试）；批量走 `downloader::batch::run`；全局 `http()` 带 2s connect + 30s read + 系统代理
- **版本目录**：`versions/{id}` 冲突时 `{id}_{n}`；目录内 jar/json 文件名可能 ≠ 目录名，一律扫描兜底
- **Java 硬下界**：`launcher/compat.rs::check` 是启动前兼容判定唯一入口——`javaVersion.majorVersion` 低于要求一律 Block；上界只在跨 Java 8→9 时生效（launchwrapper 主类 Block，8 时代 Forge/NeoForge/OptiFine Warn，`disable_launch_wrapper` 豁免）
- **崩溃判定**：非零退出码 + 时间窗内证据（crash-report / `logs/latest.log` / 启动器捕获的 `logs/elytra-launch.log` 本次偏移之后新增段）；启动取消由「取消标志 + 已拉起 pid」两个原子量协作，取消先置标志再取 pid，进程退出即注销 pid
- **零字节对象合法**：sha1 等于 `SHA1_EMPTY` 时 200 空响应按 0 字节文件写入并通过校验（Mojang `jre-legacy`）；未声明的空响应仍判失败
- **进度同口径**：`progress::update_progress` 同步单步任务的 `steps[0].progress`，避免「总 99% / 步骤 0%」
- **serde_json 的字符串索引读时也会写入**：探测一律 `get`/`get_mut`，只有赋值才用索引。`merged["arguments"]` 会给老版本 JSON 插入 `"arguments": null`，读取端反序列化直接失败——该字段为 `Option<Arguments>` + `jvm_args()/game_args()` 访问口（`#[serde(default)]` 兜不住显式 null）
- **loader 库条目两种形态都要进 classpath**：老式 `{name, url}` 甚至只有 `{name}`，无 `downloads.artifact`（路径从 `artifact.path` 取 → 缺失即静默丢出 classpath）。`shared::normalize_legacy_libraries` 按坐标补 path/url（无 url 走 `MOJANG_LIBRARIES`）；**只有 `classifiers` 的 natives 条目绝不合成普通构件**（会 404）
- **installer 式 loader 必须落盘整份 `install_profile.libraries`**：Forge 1.17+ 的 `fmlcore` / `javafmllanguage` / `lowcodelanguage` / `mclanguage` 只在 install_profile 声明，漏装则启动报 `Invalid paths argument, contained no existing paths` 秒退而安装任务「成功」。映射在 `forge::profile_libraries`（URL 为空的条目由 installer 内嵌流程落盘，须过滤否则批量下载中止），收尾用 `forge::verify_profile_artifacts` 核每一个构件
- **外部输入的落盘路径**一律过 `util::safe_relative_path`（拒绝绝对路径 / `..` / 空段），sha1 子目录走 `util::sha1_sub_dir`
- **子进程一律无窗口（铁律）**：所有 spawn 点（java、installer processors、游戏、`cmd /C`、`taskkill`…）必须经 `util::process::NoWindow`——`cmd.no_window()`；需附加标志用 `cmd.no_window_with(extra)`，**不要再调 `creation_flags`**（覆盖会丢 `CREATE_NO_WINDOW`）

### 前端 Vue/TS

- **IPC**：命令名与参数/返回值全 snake_case；**`invoke` 一律封装成方法**，只允许出现在 store/封装层（组件/页面禁止裸调），泛型只写在封装层
- **TS 类型镜像**：共享类型由 Rust `#[derive(ts_rs::TS)]` + `#[ts(export_to = "<域>/")]` 生成到 `src/types/<域>/`；改字段必须重跑生成；`#[serde(skip_serializing)]` 配 `#[ts(skip)]`；`u64`/`i64` 加 `#[ts(type = "number")]`（`Option` 用 `number | null`）；前端名不同时用 `export type { RustName as FrontendName }`
- **状态**：模块级 `reactive()`/`ref()` 单例导出，页面直接 import；无 Pinia/Vuex
- **样式**：CSS 变量统一在 App.vue `:root` + 四套 `data-theme`；组件一律 `var()` + `<style scoped>`；图标用 `@lucide/vue`
- **三端 WebView 一致性**：Win WebView2（Blink）/ macOS WKWebView / Linux WebKitGTK 版本滞后；新 CSS 特性一律 `@supports` 包裹或写兼容写法，**禁止裸用**；中文字体栈固定 `"Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", "Noto Sans SC", sans-serif`。基线 **Chrome 111 / Safari 16.4**（`browserslist` + `vite.config.ts` build.target）；`pnpm lint:css` 对 `color-mix()` / `backdrop-filter` / `scrollbar-gutter` 是盲区，这三类必须手写 `@supports` 或双声明降级
- **错误提示两级**：toast（`showToast`，默认 4000ms、同屏 5 条、hover 暂停）vs AlertDialog（复杂错误，带复制）；均经 `errMsg()` 取首行
- **键盘与焦点**：Tab/Shift+Tab 走 `utils/focusNav.ts` 自管遍历（文档序；弹窗内收窄成焦点陷阱，`data-tabskip` 跳过），不用浏览器原生遍历；焦点环由该模块**内联写入内描边**，不要再加 `:focus-visible` 外轮廓；弹窗需 `role="dialog" aria-modal="true"` + `focusDialog`/`restoreFocus`，提示通道用 `role="status"`
- **展开/收起动画**：`<Transition name="expand">` + `.expand-enter-to/.expand-leave-from { max-height: 1000px }` 显式目标值（写在 active 类上会因同特异性覆盖 from 而无动画）
- **社区资源搜索**：搜索表单 + 结果列表组件成对复制（绑各自 store）；聚合模式每源取 `AGG_PAGE_SIZE=15` 按标题去重；进入下载页并发预加载各 tab 热门（`ensureAutoSearch` 幂等）；刷新按钮 = 各 store 的 `reset()`
- **路由**：hash 路由，`RouteMeta.transition` + `routeLevel`/`tabOrder` 驱动动画；DownloadDetail 为顶级 tab（routeLevel 0）

### 注释规范（Rust / TS / Vue 通用）

**一句话**：只写「代码读不出来、但下一个改这段代码的人必须知道」的东西。

- **只写约束，不写故事**：不变量、边界与数值口径、平台/协议差异、外部系统硬约束、必须同步的位置、安全要求。
- **禁止**：复述代码；项目背景与开发历程；回归/来历叙事（「曾…」「此前…导致…」「踩过的坑」「实测过一次」「已修复」）；叮嘱语气与口语（「务必/切记」「炸档」「别改回」）——改客观陈述句。
- **长度**：单条注释 ≤3 行（并列事实最多 4 行）；模块头 `//!` ≤6 行。写不下说明该拆结构。
- **magic number 带口径**：写单位与作用（如「超时 4s：官方源 P95 2.5–4.7s，2s 会误判降级」）
- **结构标记**：分节用 `// ── X ──`；`///` 写「是什么 + 非显然约束」，不复读签名。
- **语言**：中文；标识符、协议/API 名、CLI 命令保留原文。

```rust
// ✓ 仅在线源落 `modpacks/{key}.{ext}`；本地源直接用用户选定的文件。
```

## Runtime/Tooling Preferences

- **包管理器必须 pnpm**：`beforeDevCommand` 硬编码 `pnpm dev`；`pnpm-lock.yaml` 入库；CI 与本地一律 `pnpm install --frozen-lockfile`
- **工具链单一版本源 `mise.toml`**（node / pnpm / rust / protoc）：本地 `mise install`；CI 用 `jdx/mise-action` 读同一文件，不写死版本号。各 job 设 `MISE_RUSTUP_HOME` 指向 mise 数据目录（rust 实体在 rustup home，否则缓存命中但工具链缺失）
- **工具链很新**：vite 8、typescript 6、vue-tsc 3、vue-router 5——新增依赖前确认兼容
- **窗口**：1008×648、`decorations:false` + `transparent:true` + `resizable:false`，前端自绘 TitleBar；CSP 的 `img-src` 需放行 `cdn.modrinth.com` / `*.forgecdn.net` / `mod.mcimirror.top`，否则远程图标被静默拦截
- **capabilities 是 v2 权限门**：新增 JS 侧 window/fs/dialog API 必须先改 `src-tauri/capabilities/default.json`
- **Rust 变更需重启 `pnpm tauri dev`**（vite 忽略 src-tauri）；前端跨多个 store 的批量改动后建议重启 dev——HMR 可能停在半旧半新（组件逻辑更新但 store 是旧的），排查前端怪状前先重启排除
- **reqwest 0.13 无 `.form()`**：表单手工 urlencode
- **MS_CLIENT_ID 已内置公开常量**；回调固定 `http://localhost:27205`，换 Azure 应用时同步改常量与注册地址
- **lib 名 `elytra_lib`**（Windows bin/lib 同名会 Cargo 8519）
- **版本号三处一致**：`tauri.conf.json` / `Cargo.toml` / `package.json`（当前 1.1.0）
- **系统代理**：Windows 读注册表（`winreg`）→ env；Linux env → GNOME gsettings；reqwest 默认不读，开启系统代理时官方源直连会被劫持
- **打包**：`bundle.targets` 列全 deb/rpm/appimage/app/dmg/nsis/msi（bundler 按平台求交过滤）。NSIS 卸载是逐文件 `Delete` + 非递归 `RMDir`，**不会删游戏数据**；`identifier`（`xyz.bridgefish.elytra`）≠ 配置目录（`Elytra`），故 `POSTUNINSTALL` 按真实路径补删；MSI 装 `Program Files`，`default_game_dir_base` 有回退；卸载后保留 `HKCU\Software\<manufacturer>\<product>` 是有意行为
- **发布工作流**：`.github/workflows/release.yml` 只在推 `v*` tag 时跑（手动 dispatch 只出 artifacts，需 workflow 在 `main`）。门禁复用 `check.yml`（frontend：vue-tsc + lint:css + i18n + fmt；rust-windows：clippy + test；rust-cross：Linux/macOS clippy）；`prepare` → `build`（三平台并行）→ `publish`，任一失败 Release 停在草稿。产物：Win 便携 zip + NSIS + MSI、macOS universal dmg（ad-hoc 签名，Apple Silicon 上未签名无法启动）、Linux AppImage + deb + rpm + tar.gz。密钥走 Secrets（`ELYTRA_CURSEFORGE_API_KEY`，缺失 workflow 直接失败；本地缺 `.env` 时 CurseForge 静默降级）。Linux 固定 `ubuntu-22.04`（glibc 下限）。同版本重跑：`git tag -f v1.0.0 <提交> && git push -f origin v1.0.0`（tag 必须与版本号一致）
- **EasyTier 构建依赖**：`kcp` feature 需 libclang（`winget install LLVM.LLVM` + `LIBCLANG_PATH`）；`7z` 在 PATH；`protoc`（mise 已列，勿在 workflow 另装）
- **种子预览构建依赖**：`build.rs` 编译 `vendor/cubiomes`；Windows MSVC 用 **clang-cl**（上游用 VLA，cl.exe 不兼容），从 `LIBCLANG_PATH` / `C:\Program Files\LLVM\bin` / PATH 自动查找
- 敏感信息（token/client secret）不硬编码、不打印日志、不提交 git
- **全局事件拦截**：App.vue 禁 F12/DevTools/全选/刷新；**Tab 不吞**——转发给 `focusNav`；ESC 无弹窗时回启动页

## Testing & QA

- **无前端测试**；Rust 439 个单元测试（431 passed + 8 ignored），与实现同文件、`#[cfg(test)] mod tests`；集成测试 `tests/online_diag.rs`、`tests/tool_download.rs`、`tests/e2e_download.rs`（`#[ignore]`，需真实网络）
- **只测纯函数与关键不变量**：PKCE、URL 组装、启动命令构建、离线皮肤 UUID、内存分配公式、util、maven 解析与排序、Mod 双源搜索、修复步骤规划、persist 读写、secrets DPAPI、store 快照/恢复、fetcher 取消/换源/校验/重定向、segmented 切分/续传/回退、loader profile 合并与继承、房间码与广播回环、emit 派发器（合并/保序）、progress 口径、步骤索引↔名字对齐
- **断言风格**：`assert_eq!` 精确值为主要；行为不变量用 `assert!` + 中文 message
- **平台隔离**：Windows 专属（BAT 渲染、DPAPI、注册表代理）用 `#[cfg(target_os = "windows")]`
- **persist 测试隔离**：测试进程配置落 `target/debug/deps/Elytra/Settings.toml`；每个测试先 remove_file 再 save（串行依赖）
- **日常验证命令**：`cargo test --lib -- --test-threads=1`（**并行会偶发竞争**：persist 共用 Settings.toml + 网络抖动）
- **clippy 红线**：`cargo clippy --lib` 零代码警告（仅余显式 allow 的 `type_complexity` ×2、`upper_case_acronyms` ×3）
- **e2e**：`tests/e2e_download.rs` 用 `#[tokio::test(flavor = "multi_thread")]`（终态收口走 `block_in_place`，current_thread 会 panic）；任务层事件出口抽象为 `task::emit::EventSink`，测试注入记录型实现即可，不触碰 IPC
- **ModLoader 矩阵**：`game/loader/mod.rs` 的 `#[cfg(test)] mod matrix`，真实网络 + 真实 JDK 跑「形态」而非「版本」：loader 9 条（Fabric / Quilt / Forge 1.7.10·1.8.9 spec-0 / Forge 1.12.2 / 1.16.5 / 1.20.1 / NeoForge 20.4·1.21.1）+ OptiFine 5 条；每例把安装出的版本 JSON 按真实模型解析并构建启动命令（classpath 项数、`${}` 残留、loader 是否进 `-cp`/`--module-path`、natives 标志）
- **工具箱下载器测试**：本地 HTTP 服务（可控 206/200 + 按块节流）验证入队/分段一致性/暂停位图/续传/取消/不支持 Range 回退；文件内有 `TEST_LOCK` 串行闸
- **发布前 GTK 冒烟**：CI 只构建不渲染，发版前在 WSLg / Linux VM 跑 `pnpm tauri dev` 过 10 个页面（滚动条宽度、徽章底色、弹窗遮罩、皮肤画布）

## Known Debt（已知遗留，待处理）

- **Fabric/Quilt + OptiFine 不推荐**：依赖 OptiFabric 桥接（CurseForge，slug `optifabric`），上游维护缓慢；UI 标注「不推荐」并建议 Sodium + Iris 或 Forge + OptiFine，1.21+ 隐藏并报错
- **跨网联机依赖协调节点**：默认内置 `https://etnode.zkitefly.eu.org/node{1,2}`（社区节点，禁中转）；默认 `allow_relay=false`，打洞失败需用户临时允许中继；MC 26.2 离线账号只能走局域网发现（广播模拟已绕过会话校验）
- **`src-tauri/src/data/wiki_entries.txt`**：PCL `WikiEntries.txt` 未修改快照，行号 = MC 百科条目 id；更新须同步 `THIRD-PARTY-NOTICES.md` 的 commit/版本并重跑 `cargo test --lib wiki_entry`
- **地图调色数据**：`assets/natural_block_colors.json` 等由客户端素材派生，仅本地着色用；公开分发前确认许可并在 `THIRD-PARTY-NOTICES.md` 保留来源
- **种子预览精度/版本边界**：cubiomes 仅覆盖正式版参数族（Beta 1.7 → 26.3）；补丁版本映射到最近参数族并在 UI 标注；快照/基岩版禁用；结构是候选（1.18+ 仍可能误报）。升级 cubiomes 或参数族后必须重跑 `seedmap::tile` 锚点测试与官方服务端抽样对拍
- **vendored cubiomes**：xpple fork `d934de1`（MIT），只含核心/feature helper、不含 loot；升级同步 `PROVENANCE.md` 与 `THIRD-PARTY-NOTICES.md`
