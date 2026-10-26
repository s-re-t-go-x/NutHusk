# NutHusk 插件协议 v1.1

宿主 API 版本：**1.1.0**（向下兼容 `^1.0.0`）。本文是写插件所需要知道的全部内容。

## 设计原则

- **外壳和插件互相独立。** 外壳不关心插件用什么语言写，插件也不依赖外壳的内部实现。
- **只有两条通道。** 插件界面（HTML/JS，跑在沙箱里）和插件后端（任意语言的程序，通过 stdin/stdout 说 JSON-RPC）。
  界面只能通过外壳转发，找自己的后端说话。
- **最小化宿主 API，严格按版本演进。** 宿主 API 只做加法，`1.x` 内保证向后兼容。插件用 `apiVersion` 声明自己需要的版本（如 `^1.0.0` 或 `^1.1.0`），不满足时会被明确标为“加载失败”并写明原因。

## 插件长什么样

一个插件就是一个文件夹（或打成 `.nut`，本质是 zip）：

```
my-plugin/
  plugin.json        必需
  ui.html            可选：界面
  icon.svg           可选：图标（png / svg / jpg / webp / gif / ico）
  main.py …          可选：后端（任意语言）
  signature.json     可选：签名（由 nuthusk-sign 生成，见 SIGNING.md）
```

插件放进下面两个目录之一即可被读取，**两个目录里的插件地位相同**，不区分内置和用户安装：

| 目录 | 位置 |
|---|---|
| AppData | 应用数据目录下的 `plugins`（设置页里能看到完整路径） |
| 程序目录 | 程序所在目录旁的 `plugins` |

同一个插件（`id` 相同）出现在两个目录里时，用优先级靠前的；默认 AppData 优先，可在设置里调整。新安装的插件装到靠前且可写的目录。

## plugin.json

```json
{
  "id": "com.example.my-tool",
  "name": "我的工具",
  "version": "1.0.0",
  "apiVersion": "^1.0.0",
  "description": "一句话说明",
  "icon": "icon.svg",
  "category": "开发",
  "keywords": ["关键词", "搜索用"],
  "ui": "ui.html",
  "backend": { "command": "python3", "args": ["main.py"], "windows": { "command": "python", "args": ["main.py"] } },
  "permissions": ["network"],
  "match": {
    "regex": ["^\\s*[\\{\\[][\\s\\S]*[\\}\\]]\\s*$"],
    "prefixes": ["{", "["],
    "extensions": ["json", "geojson"],
    "minLength": 2,
    "maxLength": 500000,
    "label": "识别为 JSON 文本"
  }
}
```

| 字段 | 必需 | 说明 |
|---|---|---|
| `id` | 是 | 全局唯一，小写字母、数字、连字符，用 `.` 分成至少两段，如 `com.example.my-tool` |
| `name` | 是 | 显示名称 |
| `version` | 是 | 语义化版本，如 `1.2.0` |
| `apiVersion` | 是 | 需要的宿主 API 版本范围，如 `^1.0.0` 或 `^1.1.0`。当前宿主是 `1.1.0` |
| `description` | 否 | 一句话说明，显示在卡片和工具页顶栏 |
| `icon` | 否 | 插件目录内的图片文件，≤ 512 KB。不写或加载失败时，用名称首字加彩色底代替 |
| `category` | 否 | 分类，默认「其他」 |
| `keywords` | 否 | 搜索用的关键词 |
| `ui` | 否 | 界面页面，相对路径 |
| `backend` | 否 | 后端启动方式，见下 |
| `permissions` | 否 | `clipboard` / `fs.read` / `fs.write` / `network`，见下 |
| `match` | 否 | **（1.1 新增）** 智能内容识别与文件匹配规则，用于在 `Ctrl+K` 粘贴内容、跨工具投递或拖入文件时自动推荐本插件，见下 |

所有相对路径都不能含 `..` 或绝对路径。

### match（1.1 新增：智能内容与文件识别）

插件可在 `plugin.json` 中声明 `match` 对象，告诉宿主自己擅长处理什么样的文本或文件后缀：

| 子字段 | 类型 | 说明 |
|---|---|---|
| `regex` | `string[]` | 正则表达式列表（JavaScript 正则语法），任意一条匹配输入文本时视为精确命中 |
| `prefixes` | `string[]` | 文本前缀列表（不区分大小写，如 `["eyJ", "http://"]`），匹配时视为精确命中 |
| `extensions` | `string[]` | 支持处理的文件扩展名（不含 `.`，如 `["json", "txt"]`），拖入对应后缀的文件时可打开 |
| `minLength` | `number` | 可接收文本的最小字符数 |
| `maxLength` | `number` | 可接收文本的最大字符数 |
| `label` | `string` | 命中时在命令面板展示的提示语，如 `"识别为 JSON 文本"` |

### backend

```json
"backend": { "command": "node", "args": ["main.js"] }
```

- `command` 以 `./` 开头表示插件目录里的可执行文件（如 `./bin/tool`）；否则从 PATH 里找（如 `node`、`python3`）。
- 工作目录是插件目录。
- 可以按系统分别覆盖：`windows` / `macos` / `linux`，各自是 `{ "command", "args" }`。没有专用配置的系统回退到顶层的 `command`。
  这样同一个插件可以携带各平台的可执行文件，或者在 Windows 上用 `python`、其他系统用 `python3`。
- 当前系统既没有专用配置、也没有顶层 `command` 时，插件会显示"没有适用于当前系统的后端"。
- 想让用户不用装运行环境，最稳妥的办法是自带编译好的可执行文件（`./xxx`）。从桌面图标启动的应用，PATH 往往比终端里的短，终端里能运行的 `node` 在应用里不一定找得到；安装确认框会提示这一点。

## 后端协议：stdin/stdout 上的 JSON-RPC

后端是一个普通程序，**从 stdin 逐行读取请求，向 stdout 逐行写出响应**。每行一条 UTF-8 JSON。

请求（宿主 → 后端）：

```json
{"jsonrpc":"2.0","id":1,"method":"hash","params":{"text":"abc"}}
```

成功响应（后端 → 宿主）：

```json
{"jsonrpc":"2.0","id":1,"result":"ba7816bf…"}
```

失败响应：

```json
{"jsonrpc":"2.0","id":1,"error":{"message":"不支持的算法: md4"}}
```

规则：

- 响应里的 `id` 必须原样带回。`result` 可以是任意 JSON。出错时只需要 `error.message`。
- **每写完一行必须刷新（flush）**，否则宿主收不到。
- stdout 只放协议消息；调试输出写到 **stderr**，会出现在「插件管理 → 日志」里（最多 500 行）。
- 后端在**第一次被调用时才启动**（懒加载）。进程退出或崩溃后，下一次调用会自动重新启动。
- 一次调用 15 秒内没有响应会超时。宿主可能在收到上一个响应之前就发来下一个请求，请按 `id` 对应。
- 更新或卸载插件、禁用插件、关闭标签页（若开启自动回收）或空闲超时后，宿主会自动结束后端进程。

**最小实现**（以下三份都用文档里的原样代码实际运行验证过）：

Python：

```python
import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    try:
        reply = {"jsonrpc": "2.0", "id": msg["id"], "result": msg["params"]["text"].upper()}
    except Exception as e:
        reply = {"jsonrpc": "2.0", "id": msg["id"], "error": {"message": str(e)}}
    sys.stdout.write(json.dumps(reply, ensure_ascii=False) + "\n")
    sys.stdout.flush()
```

Node.js：

```js
require('readline').createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line);
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: String(msg.params.text).toUpperCase() }) + '\n');
});
```

Rust（`serde_json`）：

```rust
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn main() {
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let text = msg["params"]["text"].as_str().unwrap_or("").to_uppercase();
        writeln!(out, "{}", json!({ "jsonrpc": "2.0", "id": msg["id"], "result": text })).ok();
        out.flush().ok();
    }
}
```

Go、C#、Java、Shell 等同理：读一行、解析、写一行、刷新。

## 界面

界面是一个 HTML 页面，运行在**沙箱 iframe** 里：不能访问宿主的任何东西，只能通过 SDK 找自己的后端或进行受控的数据投递。引入统一样式和 SDK：

```html
<link rel="stylesheet" href="/_sdk/ui.css">
<script src="/_sdk/ui.js"></script>
```

- `/_sdk/ui.css`：跟随宿主的亮/暗主题，并给按钮、输入框等原生控件统一外观；提供 `.nh-page`、`.nh-toolbar`、`.nh-panes`、`.nh-pane`、`.nh-status` 等常用版式（参考 `nutPlugins/json-formatter/ui.html`）。
- `/_sdk/ui.js`：
  - `await nuthusk.call(method, params)` 调用自己的后端，返回后端的 `result`；后端返回 `error` 时抛出异常。
  - **（1.1 新增）** `nuthusk.onInput(function (text, meta) { ... })`：注册输入接收回调。当用户在 `Ctrl+K` 粘贴内容带入打开、拖入匹配后缀的文件，或从其他插件（及左侧分屏）投递内容过来时触发。若旧版 1.0 插件未注册 `onInput`，SDK 会自动将文本填入页面中的第一个主输入框（`<textarea>` 或 `<input>`）。
  - **（1.1 新增）** `nuthusk.sendTo(text, targetId?)`：将当前工具产出的文本投递给其他工具。不传 `targetId` 时，宿主会弹出投递面板（若当前开启了左右双栏分屏，面板第一项支持一键投递到右侧分屏工具）。
  - 自动同步宿主的主题（给 `<html>` 加 `data-theme`）。
  - 切换到插件标签时自动聚焦主输入框；在插件内按 `Ctrl/Cmd + Enter` 自动触发主操作按钮（`.primary` / `[data-primary]` / `#go` / `#fmt`）。
  - 焦点在插件里时，把全局快捷键（`Ctrl/Cmd` + `K` / `W` / `\` / `Tab` / `1–9`）转发给宿主。
- 页面里的文件用相对路径引用即可（`icon.svg`、`app.js`、图片等都从插件自己的目录读取）。
- 安全策略：插件页面**默认禁止一切联网**；声明了 `network` 权限才允许 `http/https` 请求。
- 拖放：拖到插件界面上的文件由插件自己处理（所以图片类工具可以接收拖入的图片）。
- `window.toolbox` 是 `window.nuthusk` 的旧名字，仍然可用。

## 权限

`permissions` 是插件对自己需要什么的**声明**，会显示在卡片、管理页和安装确认框里。目前真正由外壳强制执行的只有一项：

| 权限 | 现状 |
|---|---|
| `network` | **强制**：只有声明了它，插件界面才能发起网络请求 |
| `clipboard` `fs.read` `fs.write` | 仅声明。外壳还没有提供对应的 API，将来提供时会先检查这些权限 |

**后端是普通系统进程，拥有当前用户的全部权限，不受 `permissions` 限制。** 所以带后端的插件，安装确认框会明确写出它要运行的命令；是否信任它，看的是来源和签名。

## 来源可信度

每个插件都会标注来源：

| 标识 | 含义 |
|---|---|
| 绿色「可信来源」 | 由受信任的密钥签名，且内容和签名时完全一致 |
| 黄色「未经安全验证」 | 没有签名，或签名者不在受信任列表里 |
| 红色「签名无效」 | 有签名但与内容对不上（被篡改）。**禁止安装** |

签名和审查流程见 [SIGNING.md](SIGNING.md)。

## 兼容性

- `apiVersion` 用语义化版本范围。宿主 API 只会做加法，`1.x` 内不会破坏已有行为。
- 想让插件在新旧版本宿主上都能用，保持 `^1.0.0` 即可。
- 需要 1.1 的能力时，可写 `^1.0.0`（在 `ui.js` 中用 `if (nuthusk.onInput)` 渐进增强）或直接要求 `^1.1.0`。
