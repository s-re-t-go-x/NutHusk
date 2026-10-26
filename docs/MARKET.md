# 插件下载板块与插件源规范

NutHusk 提供了「插件下载」板块，用户可在界面中直接浏览、搜索并一键安装常用插件；同时支持团队内部自建私有源以及通过直链安装私有插件。

---

## 一、 插件源架构与安全模型

NutHusk 的插件获取机制遵循以下安全原则：

1. **官方源白名单与签名验签**：
   - 官方源默认指向 GitHub 仓库：`https://raw.githubusercontent.com/nuthusk/nuthusk-plugins/main/index.json`。
   - 官方源强制要求存在对应的 `index.json.sig` 签名文件，且签名必须由 `trusted_keys.json` 中配置的官方公钥签名。
   - 官方源中列出的所有插件下载链接仅允许位于 `raw.githubusercontent.com` 或 `github.com` 官方组织下，且必须强制提供 SHA-256 哈希值。
2. **私有源与手动直链**：
   - 支持用户添加自定义 HTTPS 源（私有 `index.json`）。私有源若未签名会标注为「未验证」，方便团队内部轻量分发。
   - 支持用户直接粘贴 `.nut` 插件包的 HTTPS 直链进行手动安装。
3. **安装确认与权限审查**：
   - 无论从哪个源下载的插件，下载后**均会调起安全确认对话框**，向用户如实展示其声明的权限、后端命令、来源可信度与内容哈希，用户点击「安装」后方才解压安装。
4. **运行时环境与插件解耦**：
   - 插件下载与安装时不检查运行环境；仅在用户**实际执行启动**插件后端且系统缺少所需环境时，才会弹出环境缺失提示并支持一键下载。

---

## 二、 插件源索引格式 (`index.json`)

每个插件源需要托管一个 UTF-8 编码的 `index.json` 文件。格式如下（列表字段名写 `plugins` 或 `entries` 均可识别）：

```json
{
  "schemaVersion": 1,
  "name": "NutHusk 官方插件源",
  "updated": "2026-10-25T12:00:00Z",
  "plugins": [
    {
      "id": "com.example.hashcalc",
      "name": "哈希计算",
      "version": "1.0.0",
      "description": "本地快速计算文本与文件的 MD5、SHA-1、SHA-256",
      "author": "NutHusk Team",
      "category": "开发",
      "url": "https://raw.githubusercontent.com/nuthusk/nuthusk-plugins/main/plugins/hashcalc-1.0.0.nut",
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "size": 15420
    }
  ]
}
```

### 字段说明

| 字段 | 类型 | 说明 |
| :--- | :--- | :--- |
| `schemaVersion` | 数字 | 索引规范版本，当前固定为 `1`。 |
| `name` | 字符串 | 插件源名称，将在界面源切换下拉框中显示。 |
| `updated` | 字符串 | 索引最后更新时间（ISO 8601 格式）。 |
| `plugins`（或 `entries`） | 数组 | 插件条目清单。 |
| `plugins[].id` | 字符串 | 插件唯一标识（如 `com.example.tool`），必须与包内 `plugin.json` 一致。 |
| `plugins[].name` | 字符串 | 插件名称。 |
| `plugins[].version` | 字符串 | 语义化版本号（如 `1.0.0`）。若版本高于用户本地已安装版本，卡片上会自动显示「更新」按钮。 |
| `plugins[].description` | 字符串 | 简要介绍。 |
| `plugins[].author` | 字符串 | 选填，插件作者或组织名。 |
| `plugins[].category` | 字符串 | 分类名称（如 `开发`、`效率`、`文本`、`图片` 等），用于界面标签筛选。 |
| `plugins[].url` | 字符串 | 插件包（`.nut` 或 `.zip`）的 HTTPS 下载直链。 |
| `plugins[].sha256` | 字符串 | 插件包的 SHA-256 十六进制摘要（64 位小写）。下载时会流式校验，不匹配则拒绝安装。 |
| `plugins[].size` | 数字 | 选填，插件包文件字节大小，用于在下载前显示预估体积。 |

---

## 三、 官方源索引签名发布流程

为官方源发布索引时，必须生成配套的 `index.json.sig` 文件：

```bash
# 1. 签名索引文件（使用已配置在 NutHusk 中的受信任私钥）
nuthusk-sign sign-index index.json --key my-signing-key.key
# 生成 index.json.sig

# 2. 将 index.json 和 index.json.sig 一同推送到官方仓库 main 分支
git add index.json index.json.sig
git commit -m "release: update plugin index"
git push origin main
```

NutHusk 在拉取官方源时，会自动同级请求 `index.json.sig` 并利用公钥进行 Ed25519 签名验证。如果签名缺失或被篡改，官方源将拒绝加载并提示安全警告。

---

## 四、 插件打包与发布步骤

1. **打包插件为 `.nut`**：
   插件文件夹根目录下必须包含 `plugin.json`。将其打包为标准 zip 压缩包，并将扩展名命名为 `.nut`：
   ```bash
   # Windows PowerShell
   Compress-Archive -Path ./my-plugin/* -DestinationPath ./my-plugin-1.0.0.nut
   ```
2. **对插件包签名**（可选但推荐）：
   ```bash
   nuthusk-sign sign my-plugin-1.0.0.nut --key my-signing-key.key
   ```
   签名后的插件包在用户导入时会标绿显示为「可信来源」。
3. **计算哈希值**：
   ```bash
   certutil -hashfile my-plugin-1.0.0.signed.nut SHA256
   ```
4. **填入 `index.json`** 并更新版本清单，重新执行 `sign-index` 后提交推送。

---

## 五、 自定义运行环境配方 (Runtime Recipes)

NutHusk 内置支持在缺少环境时自动补全 Windows 下的精简版 Python 3.13 与各平台 Node.js。

若需要支持其他语言（例如 Lua、AutoHotkey、Bun、Deno 等），可在用户配置目录下的 `nutConfig/runtimes.d/` 放置 JSON 配方文件（如 `lua.json`）：

```json
{
  "name": "lua",
  "version": "5.4.6",
  "aliases": ["lua54"],
  "description": "Lua 5.4 便携解释器",
  "platforms": {
    "windows-x86_64": {
      "url": "https://example.com/lua-5.4.6-win64.zip",
      "sha256": "abcdef...",
      "archive": "zip",
      "bin": "lua.exe",
      "stripComponents": 0,
      "keep": ["lua.exe", "lua54.dll"]
    }
  }
}
```

- **安装目录**：配置后的环境会下载至 `nutRuntimes/<name>`。
- **动态隔离注入**：运行插件时，NutHusk 会自动将该目录加入子进程环境变量 `PATH` 最前端，既保证插件正常运行，又绝不篡改用户的全局系统环境。
