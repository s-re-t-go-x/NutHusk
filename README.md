# NutHusk

> 轻壳之内，万千工具。

一个很薄的桌面外壳（Tauri 2），所有功能都是独立的插件。插件可以用任何语言开发，只要符合[协议](docs/PLUGIN_PROTOCOL.md)；
外壳和插件互相独立，外壳目前不向插件提供任何 API。

## 目录

```
crates/core/       核心库：插件发现与目录优先级、后端进程（stdio JSON-RPC）、插件包安装、签名校验、便携运行时管理、插件市场、设置存储（不依赖 Tauri）
crates/sign/       nuthusk-sign：生成密钥、给审查过的插件包和索引签名、验证签名
src-tauri/         Tauri 宿主：窗口、plugin:// 协议、命令；trusted_keys.json 是受信任的签名公钥
src/               壳界面（纯 HTML/JS，无需打包工具）
sdk/               插件界面端的 ui.js / ui.css（宿主提供给插件页面）
nutPlugins/        随程序附带的示例插件：json-formatter（纯前端）、hash-calc（Node 后端）
examples/          可直接导入试用的 .nut 示例包（源码在 examples/src）
docs/              PLUGIN_PROTOCOL.md 插件协议；SIGNING.md 签名与审查流程；MARKET.md 插件源与运行时配方规范
tests/             壳界面测试（独立的 package.json，不放在根目录，避免影响插件里的 Node 脚本）
```

## 运行与构建

```bash
cargo install tauri-cli --version "^2"      # 一次性
# Linux 另需：libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libssl-dev build-essential

cargo tauri dev            # 开发运行
cargo tauri build          # 打包安装器（打包前先 cargo tauri icon src-tauri/icons/icon.png 生成完整图标集）
```

## 测试

```bash
cargo test -p nuthusk-core -p nuthusk-sign      # Rust：核心库 + 签名工具（hash-calc 示例需要 node，Python 示例需要 python3）
cd tests && npm install && npm test             # 界面：jsdom，含示例插件自己的页面逻辑
```

## 功能

- **界面**：左侧导航（首页 / 收藏 / 最近使用 / 分类 / 插件下载 / 插件管理 / 设置）、多标签（切换时保留各工具的状态）、`Ctrl K` 搜索、工具页顶栏、底部状态栏。
- **数据根目录**：两个根目录（AppData、程序目录）地位相同，每个根下统一包含 `nutPlugins`（插件）、`nutRuntimes`（便携运行时）、`nutConfig`（配置）。同 id 的插件/运行时取优先级靠前的目录里的（默认 AppData），新内容装到靠前且可写的目录；优先级在设置里调整。
- **插件下载（市场）**：内置官方插件源（强制签名与 GitHub 域名白名单），支持添加自定义 HTTPS 私有源或粘贴 `.nut` 直链下载，离线时自动回退至缓存。下载后与本地导入走完全相同的安全确认与安装流程。详见 [MARKET.md](docs/MARKET.md)。
- **便携运行时管理**：当插件需要 Python、Node.js 等运行环境而系统未安装时，首次调用后端会自动弹出提示并支持一键下载便携版到 `nutRuntimes/`（不污染系统 PATH）；支持在 `nutConfig/runtimes.d/` 追加自定义语言配方。
- **插件管理**：导入（拖入窗口，或选择文件）、卸载、启用/禁用、查看后端日志、重新扫描。导入前一定先弹确认框，写明来源可信度、权限、后端命令、安装位置和 SHA-256。
- **图标**：`plugin.json` 的 `icon` 指向插件目录里的图片，卡片、标签页、搜索、管理页和安装确认框里都会显示。
- **来源可信度**：绿色「可信来源」（开发者审查并签名）、黄色「未经安全验证」、红色「签名无效」（禁止安装）。见 [SIGNING.md](docs/SIGNING.md)。
- **设置**：亮/暗主题（插件界面同步）、首页是否显示「最近使用」和「收藏」、数据根目录优先级、便携运行环境管理。设置页由一份清单生成，加设置项只需往 `src/js/settings.js` 的清单里加一项。
- **多语言插件**：后端是任何能读写 stdin/stdout 的程序；`backend` 可以按系统（windows / macos / linux）分别指定命令。仓库里有 Node 和 Python 的例子，协议文档里有 Node / Python / Rust 的最小实现。

## 安全模型

- 插件界面在沙箱 iframe 里（无法访问宿主），只能通过外壳转发找自己的后端；页面默认禁止联网，声明 `network` 权限才放开。
- 插件包：拒绝路径穿越和符号链接；限制文件数与大小（按实际解压字节计）；先解压到临时目录再整体替换，失败回滚；确认的 SHA-256 与实际安装的文件核对；签名无效的拒绝安装。
- **后端是普通系统进程，拥有当前用户的全部权限，`permissions` 对它不起作用。** 请只信任你审查过的插件。
- 签名只证明"持有私钥的人审查并认可了这份内容"，安装后每次启动都会重新校验。

## 拖放导入没反应时

这一版不再使用 Tauri 的原生文件拖放，改用浏览器自带的拖放（文件内容交给后端暂存成临时文件，之后与"选择文件"走同一流程）。如果仍然没有反应：

1. 若程序是**以管理员身份**启动的，Windows 会阻止从普通权限的资源管理器拖入文件（这是系统限制）。用普通权限启动即可。
2. 拖到**插件界面**上的文件归插件处理，不会触发导入——在首页、管理页或侧边栏、顶栏上松手。
3. 拖拽时应该整个窗口出现蓝色虚线框和「松开鼠标，导入插件包」；如果连这个都没出现，请告诉我。
