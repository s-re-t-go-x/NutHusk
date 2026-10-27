# NutHusk 功能演进路线图与开发进度（AI 续接交接文档）

> **致后续会话的 AI 助手**：本文档记录了 NutHusk 的核心设计约束、已完成功能（P0、P1、P2）的实现细节，以及尚未开发的 **P3 阶段计划**。接手新任务时，请先阅读本文档以快速对齐上下文。

---

## 一、核心设计哲学与强约束（不可破坏）

1. **轻壳 + 强插件隔离**：
   - 插件界面运行在 `sandbox="allow-scripts"` 的无同源权限 `iframe` 中，只能通过 `/_sdk/ui.js` 的 `postMessage` 与宿主通信。
   - 插件后端为任意语言的独立子进程，通过 `stdin/stdout` 进行单行 JSON-RPC 2.0 通信（懒启动，按需拉起）。
   - 宿主 API 当前版本为 **`1.1.0`**（`crates/core/src/manifest.rs` 中的 `HOST_API_VERSION`），必须保持对 `^1.0.0` 插件的向后兼容。
2. **键鼠双轨交互（Dual-Track UX）**：
   - 每个核心功能必须同时兼顾**鼠标党**（可视按钮、右键/中键、拖拽排序、拖入文件直达）与**键盘党**（`Ctrl+K` 命令面板、`Ctrl+\` 分屏、`Ctrl+R` 热重载、`Ctrl+Enter` 执行、自动聚焦输入框）。
3. **不做桌面悬浮球与贴边自动收起**：
   - 已明确决策：不做遮挡视线、易与多显示器冲突的桌面悬浮球或贴边隐藏；采用**系统托盘常驻 + 顶栏窗口置顶（Pin）+ 快捷键唤起**代替。
4. **测试与环境注意事项**：
   - Rust 测试：`cargo test -p nuthusk-core -p nuthusk-sign`
   - 前端测试：`node --test tests/*.test.js`（注意：`package.json` 位于 `tests/` 目录下，根目录无 `package.json`；在 Windows PowerShell 下若用 npm 请执行 `npm.cmd --prefix tests test`）。
   - `tests/settings.smoke.test.js` 强校验设置页的 6 个分区标题顺序：`['外观', '首页', '文件位置', '运行环境', '安全', '关于']`，新增设置项请放入这 6 个现有分区中，不要轻易改动一级分区标题。

---

## 二、已完成阶段（P0、P1 & P2）

### ✅ P0：操作手感打磨 + 后台进程透视管控 + 托盘常驻（已完成并验证）

- **键鼠操作手感**：
  - 标签页支持鼠标中键点击关闭、拖拽调整顺序（首页标签固定第一位，`S.moveTab`）。
  - 切换插件标签页时自动聚焦插件内第一个主输入框（`sdk/ui.js` 的 `focus` 消息处理）。
  - 在插件内按 `Ctrl/Cmd + Enter` 自动触发主按钮（`.primary` / `[data-primary]` / `#go` / `#fmt`）。
- **后台进程状态透视与自动回收**：
  - `crates/core/src/manager.rs`：`PluginInfo` 暴露 `backendRunning` 与 `backendPid`，支持 `stop(&id)`、`stop_all()` 和 `reap_idle(Duration)`。
  - 工具页顶栏、插件管理页（含「运行中」筛选）、底部状态栏实时显示运行中后端及 `PID`，支持一键停止单个或全部后端。
  - 设置页「运行环境」分区支持配置：`stopOnClose`（关闭标签页时自动结束后端进程，默认开启）与 `idleTimeoutMinutes`（后端闲置自动休眠：从不 / 1 / 5 / 15 / 30 分钟，默认 5 分钟）。
- **窗口置顶与系统托盘**：
  - 顶栏右侧新增图钉按钮 `#pin-toggle`（`set_always_on_top`），支持随时将窗口固定在最前端。
  - 启用 Tauri `tray-icon`，支持 `closeToTray`（关闭按钮最小化到托盘），托盘菜单支持「显示/隐藏主窗口」「结束所有后台插件进程」「退出 NutHusk」。

### ✅ P1：智能识别 + 跨工具投递 + 左右双栏分屏 + 配置清单导入导出（已完成并验证）

- **宿主协议 1.1.0 与智能内容识别（`match` + `nuthusk.onInput`）**：
  - `crates/core/src/manifest.rs`：新增 `MatchSpec`（`regex`, `prefixes`, `extensions`, `minLength`, `maxLength`, `label`）。
  - `src/js/state.js` & `src/js/palette.js`：在 `Ctrl+K` 命令面板粘贴文本时实时嗅探特征，置顶显示 `✨ 识别为 ... · 带内容打开`；拖入匹配 `extensions` 的非插件包文件时自动读取内容并用对应插件打开。
  - `sdk/ui.js`：新增 `nuthusk.onInput(fn)`；若旧版 `1.0` 插件未注册 `onInput`，SDK 在 30ms 后自动将文本填入页面主输入框并派发 `input` 事件。
- **跨工具数据投递（`nuthusk.sendTo`）与左右双栏分屏（`Ctrl+\`）**：
  - `src/js/main.js` & `src/style.css`：工具页顶栏新增「左右分屏」按钮（快捷键 `Ctrl+\` 或 `Shift + 点击标签`），支持左右 50%/50% 双栏并排显示两个工具、一键「互换」左右视图或「退出分屏」。
  - 插件调用 `nuthusk.sendTo(text)` 时弹出投递面板；若当前处于左右分屏模式，面板第一项置顶 `⚡ 直接投递到右侧分屏`，回车即可将左侧结果直接喂给右侧工具。
- **配置加载清单文件（`.nutprofile`）导入与导出**：
  - `crates/core/src/profile.rs` & `src/js/profile.js`：定义 `nuthusk-profile` (v1) JSON 清单格式，包含设置、自定义插件源、已安装运行时列表、已安装插件列表及工作区状态（收藏、打开标签、分屏状态、场景工作区、快捷流水线）。
  - 在「设置 → 文件位置」提供「导入配置清单…」与「导出配置清单…」按钮（已在 `src-tauri/capabilities/default.json` 开放 `dialog:allow-save` 权限），并支持直接将 `.nutprofile` 文件拖入主窗口触发差异对比、自动下载缺失运行时与插件、安全确认安装及工作区恢复。

### ✅ P2：开发者工具链 + 无界面动作与快捷流水线 + 场景工作区（已完成并验证）

- **插件开发者工具链（CLI + 宿主热重载）**：
  - `crates/core/src/devkit.rs` & `crates/sign/src/main.rs`：新增 `new <id> [--lang js|python|rust|ui-only]`、`check <插件目录>`（含静态校验与后端进程 JSON-RPC 2.0 探针测试）、`pack <插件目录>`（打包并校验 `.nut`）及 `run <插件目录> <动作>`（命令行无头运行插件动作）。
  - `crates/core/src/manager.rs` & `src/js/main.js`：新增 `reload_single_plugin` 命令，工具顶栏提供「重载」按钮（快捷键 `Ctrl/Cmd + R`），支持一键停止旧后端、重新读取 `plugin.json` 并加时间戳刷新插件 `iframe`。
- **无界面动作（`ActionSpec` + `nuthusk.onAction`）与快捷流水线组合（Workflows）**：
  - `crates/core/src/manifest.rs`：新增 `ActionSpec`（`id`, `name`, `method`, `description`, `defaultParams`），在 `PluginInfo` 中暴露 `actions`。
  - `sdk/ui.js`：新增 `nuthusk.onAction(fn)`，使纯前端插件（如 `json-formatter`）与带后端插件（如 `hash-calc`）都能无缝参与流水线执行。
  - `src/js/workflow.js` & `src/js/home.js` & `src/js/palette.js`：支持将多个插件动作按顺序串联保存为「快捷流水线」，在首页卡片或 `Ctrl+K` 命令面板中一键执行并支持将最终输出继续投递给其他工具。
- **场景工作区（Named Workspaces）**：
  - `src/js/state.js` & `src/js/main.js`：在左侧导航栏新增「场景工作区」分组与 `+ 保存` 按钮，支持将当前打开的多标签与左右双栏分屏布局保存为命名工作区（如「前端调试」「接口联调」），随时从侧边栏或 `Ctrl+K` 面板一键切换恢复，并随 `.nutprofile` 清单一并导出导入。

---

## 三、待完成阶段（P3 计划）

### ⏳ P3：数据沉淀与深水区安全（远期规划）

1. **工具输入历史与常用片段收藏（Input History & Snippets）**
   - **目标**：每个插件工具自动记录最近处理过的非敏感输入历史，支持在工具顶栏或快捷键下拉回填，支持将高频测试数据（如常用测试 Token、示例 JSON、正则模板）收藏为固定片段（Snippets）。
   - **注意**：需提供一键清空历史与按插件关闭记录的隐私开关。

2. **运行时权限弹窗与进程级沙箱探索（Runtime Permission Prompt & Sandboxing）**
   - **目标**：将目前仅作声明的 `clipboard` / `fs.read` / `fs.write` 逐步升级为受控宿主 API（调用时弹出「允许一次 / 始终允许 / 拒绝」并记录活动日志），并探索对带后端插件的操作系统级受限令牌/沙箱隔离或 WASI 后端支持。
