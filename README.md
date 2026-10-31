# NutHusk

> 轻壳之内，万千工具。

**NutHusk 是一个轻量、可扩展的本地桌面工具箱外壳，支持不同语言编写的小工具，将其以插件形式集中到同一个工作台。** 用户按需安装插件工具，工作台提供多标签、分屏、场景区、动作流水线等功能，让各工具独立使用或相互协作。

## 产品功能

- **多语言插件**：支持纯前端工具，以及 Python、Node.js、Rust、Go 等语言的独立后端；用户自己也可封装脚本或软件成 `.nut` 插件。
- **工具工作台**：多标签、左右分屏、收藏、最近使用和搜索；命名工作区可保存常用标签与分屏布局。
- **内容识别与投递**：按插件规则识别文本和文件，带内容打开工具，将结果继续发送给其他插件。
- **动作流水线**：串联插件声明的动作，让前一步输出成为下一步输入，保存后重复执行。
- **插件分发与管理**：支持官方源、私有 HTTPS 源、本地导入和直链安装，提供签名校验、启用/禁用、卸载及日志查看。
- **进程与运行环境**：后端按需启动，支持关闭或闲置回收；缺少受支持的解释器时，可下载便携运行时，不修改系统全局 PATH。
- **配置迁移**：用 `.nutprofile` 导出设置与工作区配置，在其他设备恢复布局并从对应源补齐插件和运行环境。
- **内容留存**：宿主不保存工具输入、输出及流水线中间结果，仅保存配置和工具使用记录。插件自身的数据处理方式见[安全与来源](#安全与来源)。

内容识别、投递与流水线需要插件提供相应支持。示例插件独立维护在 [NutHusk-Plugins](https://github.com/s-re-t-go-x/NutHusk-Plugins)。

## 技术栈与插件架构

| 部分 | 实现 |
| --- | --- |
| 桌面宿主 | Rust + Tauri 2，使用系统 WebView |
| 工作台界面 | 原生 HTML / CSS / JavaScript，无需前端构建工具 |
| 插件界面 | 沙箱 iframe，通过统一 SDK 与宿主通信 |
| 插件后端 | 任意语言的独立进程，通过 stdin/stdout 上的 JSON-RPC 2.0 通信 |
| 开发工具 | 独立的 `nuthusk-cli`，提供脚手架、校验、打包、签名与后端动作调用 |

主程序、插件与运行环境分别分发，按需安装。日常使用只需桌面主程序，CLI 可选；全新环境首次启动时没有预装插件。

## 常用快捷键

| 快捷键 | 操作 |
| --- | --- |
| `Ctrl/Cmd + K` | 打开命令面板，搜索工具或带内容打开 |
| `Ctrl/Cmd + \` | 切换左右分屏 |
| `Ctrl/Cmd + Enter` | 在插件内触发主操作 |
| `Ctrl/Cmd + R` | 重载当前插件 |
| `Ctrl/Cmd + W` | 关闭当前标签 |

## 开始使用

1. 取得并启动 `nuthusk` 主程序。升级或更换主程序时，会继续读取原有数据目录。
2. 在「插件下载」中下载安装，或在「插件管理」中选择本地 `.nut` 包；也可以把插件包拖到主窗口导入。
3. 查看插件信息与来源，确认安装后，在首页打开工具。
4. 缺少受支持的运行环境时按提示下载；也可使用系统已有环境。

## 数据位置

程序使用两个数据根目录，默认应用数据目录优先；完整路径和顺序可在「设置」中查看与调整。每个根目录按需使用以下子目录：

```text
<数据根>/
  nutPlugins/     已安装插件
  nutRuntimes/    便携运行时
  nutConfig/     设置、插件源、缓存和 runtimes.d/ 自定义配方
```

- **应用数据目录**：系统为 NutHusk 提供的用户数据位置。
- **程序目录**：发布版主程序所在目录；开发模式下为仓库根目录。

同 ID 的插件或同名运行时按目录优先级选择；新内容写入优先级靠前且可写的目录。上述目录属于运行时数据，**不需要预先创建或随程序分发**。移动主程序不会自动搬迁应用数据目录中的内容。

## 从源码运行与构建

### 开发准备

需要 Rust 工具链和平台对应的 Tauri 构建依赖。Windows 使用 MSVC 构建工具与 WebView2；Linux/macOS 需要各自的系统开发依赖。前端为原生 HTML/JS/CSS，无需 npm 构建。

开发运行需先安装 Tauri CLI：

```powershell
cargo install tauri-cli --version "^2"
cargo tauri dev
```

示例插件源码和发布包独立维护在 [NutHusk-Plugins](https://github.com/s-re-t-go-x/NutHusk-Plugins)。开发模式仍支持扫描仓库根目录的 `nutPlugins/`，该目录现在用于本地调试并被 Git 忽略；需要试用时，从插件仓库打包后导入即可。主项目构建和测试无需检出插件仓库。

### 构建分发文件

在仓库根目录执行：

```powershell
# 桌面主程序
cargo build --release -p nuthusk

# 可选：命令行工具
cargo build --release -p nuthusk-cli
```

Windows 产物与分发方式：

| 分发对象 | 文件 |
| --- | --- |
| 普通用户 | `target/release/nuthusk.exe` |
| 需要命令行功能的用户 | 额外提供 `target/release/nuthusk-cli.exe` |

前端界面、SDK、默认插件源配置、运行时配方和受信任公钥会嵌入程序。**无需复制 `src/`、`sdk/`、`nutPlugins/` 或配置 JSON 到分发目录。** 默认关闭安装器打包，不捆绑示例插件，也不自动将 CLI 放入主程序。

单文件分发仍使用系统提供的 WebView 和平台依赖；插件所需的 Node、Python 等由系统或运行时管理器另行提供。以上 `.exe` 分发方式面向 Windows；其他平台需在对应构建环境验证产物及系统依赖。

### 发布前配置

| 配置 | 用途 |
| --- | --- |
| [tauri.conf.json](src-tauri/tauri.conf.json) | 应用名称、版本、标识、窗口、图标和打包设置 |
| [market.json](crates/core/src/market.json) | 官方插件源名称、索引地址与允许下载的域名 |
| [runtimes.json](crates/core/src/runtimes.json) | 内置运行时下载配方 |
| [trusted_keys.json](src-tauri/trusted_keys.json) | 受信任的签名公钥 |

修改上述配置后需要重新构建。私钥单独保管，不能放入客户端或发布文件。

## 插件开发与发布

下面假定 `nuthusk-cli` 已加入 PATH；也可使用可执行文件的完整路径，或以 `cargo run -p nuthusk-cli --` 替代命令名。

```powershell
# 创建插件，支持 js / python / rust / ui-only
nuthusk-cli new com.example.my-tool --lang js --name "我的工具"

# 校验清单、目录及后端协议；只检查文件可加 --no-backend
nuthusk-cli check ./my-tool

# 无界面调用后端动作；也支持通过标准输入传入文本
nuthusk-cli run ./my-tool transform --input "hello"

# 打包为 my-tool-0.1.0.nut
nuthusk-cli pack ./my-tool
```

插件单独通过 `.nut` 文件或插件源发布，用户安装后进入其数据目录。示例源码、插件页面测试和发布产物在 [NutHusk-Plugins](https://github.com/s-re-t-go-x/NutHusk-Plugins) 仓库维护。主项目只保留宿主测试所需的最小夹具，测试时自动生成，不附带可发布的示例插件。

### 签名与配置生成

```powershell
# 首次生成密钥；公钥加入客户端 trusted_keys.json 后重新构建
nuthusk-cli keygen my-key

# 审查插件后签名，默认输出 .signed.nut
nuthusk-cli sign ./my-tool-0.1.0.nut --key ./my-key.key

# 从最终发布包生成索引：每个插件 ID 只保留一个包
nuthusk-cli index ./release/plugins --base-url https://example.com/plugins/ --out ./release/index.json

# 官方源必须签名索引；索引改动后需重新签名
nuthusk-cli sign-index ./release/index.json --key ./my-key.key
```

运行时配方可用 `nuthusk-cli runtime-recipe` 从本地安装包快速生成，放入用户数据根下的 `nutConfig/runtimes.d/`。目录名为 **`runtimes.d/`**，带 `s`。

完整格式、参数和发布流程：

- [插件协议与 SDK](docs/PLUGIN_PROTOCOL.md)
- [签名、验签与审查流程](docs/SIGNING.md)
- [插件索引与运行时配方生成](docs/MARKET.md)
- [路线图与进度](docs/ROADMAP.md)

## 安全与来源

插件界面运行在沙箱 iframe 中，通过宿主与自己的后端通信；页面默认禁止联网，声明 `network` 权限后才允许。插件包安装会检查安全路径、大小、哈希和签名，并采用暂存、替换与回滚流程。

**插件后端是普通系统进程，拥有当前用户权限，插件声明的 `permissions` 不会限制后端。** 签名证明内容与签名者相符，不等于系统隔离；请按来源和代码审查结果决定是否安装。签名无效的插件会被禁止安装。

## 测试

```powershell
# Rust 核心库与 CLI
cargo test -p nuthusk-core -p nuthusk-cli

# 界面与插件交互测试
cd tests
npm ci
npm test
```

前端测试使用 jsdom，只验证宿主界面与交互。Rust 集成测试使用临时生成的最小插件，需要 Node；Python 后端用例在找不到 Python 时跳过。插件自身的页面测试在 NutHusk-Plugins 仓库执行。

## 拖放导入没有反应

- Windows 下用普通权限运行程序；管理员权限窗口无法接收普通权限资源管理器的拖放。
- 将插件包拖到首页、管理页、侧边栏或顶栏。落在插件 iframe 上的文件交给该插件处理。
- 拖入文件时宿主应显示蓝色虚线提示框；若没有出现，先检查拖放位置与进程权限。

## 仓库结构

```text
crates/core/    核心逻辑，不依赖 Tauri
crates/cli/     独立 nuthusk-cli 命令行工具
src-tauri/     桌面宿主、应用配置与受信任公钥
src/           桌面界面，无需前端构建工具
sdk/           插件界面 SDK 与统一样式
tests/         界面与交互测试
docs/          协议、签名、分发配置与路线图
```
