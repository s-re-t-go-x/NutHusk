# 插件下载板块与插件源规范

NutHusk 提供了「插件下载」板块，用户可在界面中直接浏览、搜索并一键安装常用插件；同时支持团队内部自建私有源以及通过直链安装私有插件。

---

## 一、 插件源架构与安全模型

NutHusk 的插件获取机制遵循以下安全原则：

1. **官方源白名单与签名验签**：
   - 官方源的名称、索引地址和下载域名白名单统一维护在 `crates/core/src/market.json`（`name`、`url`、`allowedHosts`）。与 `runtimes.json` 一样，配置在编译时嵌入程序；修改后需要重新构建。
   - 官方源强制要求存在对应的 `index.json.sig` 签名文件，且签名必须由 `trusted_keys.json` 中配置的官方公钥签名。
   - 索引、签名、插件包及其重定向地址均受 `allowedHosts` 限制（允许所列域名及其子域名），插件包必须提供 SHA-256。域名白名单不限制 GitHub 组织，来源真实性由签名保证。
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
      "id": "com.example.hash-calc",
      "name": "哈希计算",
      "version": "1.0.0",
      "description": "本地快速计算文本与文件的 MD5、SHA-1、SHA-256",
      "author": "NutHusk Team",
      "category": "开发",
      "url": "https://raw.githubusercontent.com/nuthusk/nuthusk-plugins/main/plugins/hash-calc-1.0.0.nut",
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "size": 15420
    }
  ]
}
```

索引中的 `id` 必须与插件包内 `plugin.json` 的 `id` 完全一致，否则无法识别已安装状态。

### 快速生成 index.json

先构建 CLI：`cargo build --release -p nuthusk-cli`，将 `target/release/nuthusk-cli.exe` 加入 PATH，或直接使用完整路径运行。开发时也可将下列命令的 `nuthusk-cli` 替换成 `cargo run -p nuthusk-cli --`。

将最终要发布的插件包集中放入一个目录，然后执行：

```powershell
nuthusk-cli index ./release/plugins --base-url https://example.com/plugins/ --name "团队插件源" --out ./release/index.json
```

- 自动读取包内 `plugin.json` 的 ID、名称、版本、描述和分类，计算最终文件的 SHA-256 与大小，并拼接下载地址，文件名中的空格等字符会自动编码。
- `--base-url` 是这些插件包在服务器上的目录地址；只扫描本地目录第一层，不访问网络、不启动插件后端。
- 每个插件 ID 只能保留一个最终发布包。签名后，应移走未签名的原包以及同 ID 的旧版本，再生成索引。
- 可用 `--author "作者"` 为本批条目统一补充作者。索引条目按文件名排序。
- 默认拒绝覆盖已有文件；确认完整重建时加 `--force`。已有的手工字段不会合并保留，`updated` 可自行补充。

如需官方源签名，在所有编辑完成后执行：

```powershell
nuthusk-cli sign-index ./release/index.json --key ./my-signing-key.key
```

把 `index.json`、`index.json.sig` 和对应插件包一起发布。命令只生成配置文件，不自动上传或签名；官方源的下载地址还须满足 `market.json` 中的域名白名单。

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
nuthusk-cli sign-index index.json --key my-signing-key.key
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
   插件文件夹根目录下必须包含 `plugin.json`。直接用 `nuthusk-cli` 校验并打包为 `.nut`：
   ```bash
   nuthusk-cli pack ./my-plugin
   ```
2. **对插件包签名**（可选但推荐）：
   ```bash
   nuthusk-cli sign my-plugin-1.0.0.nut --key my-signing-key.key
   ```
   签名后的插件包在用户导入时会标绿显示为「可信来源」。
3. **自动生成索引**：将最终签名包放进发布目录，运行上文的 `nuthusk-cli index`，自动提取元数据并计算哈希及大小。
4. **签名并发布**：如需验签，执行 `sign-index`；将插件包、索引和索引签名一起发布。

---

## 五、 自定义运行环境配方 (Runtime Recipes)

NutHusk 内置支持在缺少环境时自动补全 Windows 下的精简版 Python 3.13 与各平台 Node.js。

若需要支持其他语言执行环境（例如 Lua、AutoHotkey、Bun、Deno 等），可在用户配置目录下的 `nutConfig/runtimes.d/` 放置 JSON 配方文件（如 `lua.json`）：

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

- **安装目录**：配置后的环境会下载至 `nutRuntimes/<name>/<version>/`。
- **动态隔离注入**：运行插件时，NutHusk 会自动将该目录加入子进程环境变量 `PATH` 最前端，既保证插件正常运行，又绝不篡改用户的全局系统环境。


### 快速生成 runtimes.d/ 配方

目录名是 **`runtimes.d/`**（带 `s`），位于 `nutConfig/` 下。先取得要发布的原始运行时压缩包或可执行文件，再生成配方：

```powershell
nuthusk-cli runtime-recipe lua --version 5.4.6 --file ./downloads/lua-5.4.6-win64.zip --url https://example.com/lua-5.4.6-win64.zip --bin lua.exe --platform windows-x86_64 --aliases lua54 --keep "lua.exe,lua54.dll" --out ./nutConfig/runtimes.d/lua.json
```

如果压缩包内多套了一层目录，例如 `lua-5.4.6/bin/lua.exe`，可以剥掉顶层目录：

```powershell
nuthusk-cli runtime-recipe lua --version 5.4.6 --file ./downloads/lua.zip --url https://example.com/lua.zip --bin bin/lua.exe --platform windows-x86_64 --strip-components 1 --keep "bin/*,lib/*" --out ./nutConfig/runtimes.d/lua.json
```

| 参数 | 用法 |
| --- | --- |
| `--file` | 已下载的本地文件，用于自动计算 SHA-256 和大小；命令不会下载或执行该文件。 |
| `--url` | 用户实际下载的 HTTPS 地址，返回内容必须与本地文件逐字节相同。 |
| `--bin` | 去掉 `stripComponents` 层目录后，可执行文件的相对路径，使用 `/`。 |
| `--platform` | 默认当前平台；可选 `windows` / `linux` / `macos` 与 `x86_64` / `aarch64` 的组合。 |
| `--archive` | 默认按本地文件名识别 `.zip`、`.tar.gz`、`.tgz`；其他文件视为单文件可执行程序。也可显式指定 `zip`、`tar.gz` 或 `file`。 |
| `--strip-components` | 解压时移除的顶层目录层数，默认 `0`。 |
| `--keep` | 逗号分隔的保留文件或目录前缀，如 `bin/*,LICENSE`；省略则保留全部。 |
| `--aliases` | 逗号分隔的命令别名，如 `lua54,lua5.4`。 |
| `--description` | 可选的运行环境说明。 |
| `--out` | 默认输出当前目录的 `<名字>.json`；可以指定实际配置目录，父目录会自动创建。 |
| `--force` | 覆盖已有配方，完整重建文件。 |

配方生成时会校验 HTTPS、字段格式和相对路径，但不会解压检查 `bin` 是否存在；请根据实际压缩包布局填写 `--bin`、`--strip-components` 和 `--keep`。单文件模式应使用 `--archive file`，`--bin` 只能是文件名；不支持把 `.msi`、`.7z` 等安装包直接当作运行时解压。

命令一次生成一个平台的配方。多平台发布时分别指定不同的 `--out`，再把各自的 `platforms` 条目合并到同一份 JSON 中；不要用 `--force` 期待自动合并。将最终文件放入设置页所列数据目录下的 `nutConfig/runtimes.d/` 即可，无需重新编译。运行环境实际安装在 `nutRuntimes/<name>/<version>/`。
