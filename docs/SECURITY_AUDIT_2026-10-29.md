# NutHusk 安全审核报告

审核日期：2026-10-29  
报告状态：整合初次审核、外部复核补充、已完成修复、开发者确认及修复后验证结果的当前工作区快照。后续源码、依赖或分发产物变化需重新验证。

## 1. 审核结论

本次共记录 **24 项问题或关注事项，18 项已修复，6 项保留为待决定或建议关注**。已修复项目包括签名执行边界、插件资源 CSP、消息来源校验、后端资源限制、配置导入、安装路径与并发安全、私钥创建安全，以及后续授权的 ZIP 升级、正则隔离、打包私钥拦截、跨插件投递限制、配置来源与版本校验、结构化运行时错误和旧入口清理。

- **ZIP 已升级至 9.1.3**：旧版本的三条公告在新锁定版本中不再命中；已有插件包签名验证通过。
- **插件正则已隔离到可终止 Worker**：保留 JavaScript 正则语法，超时跳过单条规则，避免阻塞主界面。
- **私钥不按公开泄露处理**：开发者确认其从未公开分发，仅由授权人员保管；不轮换、不移动现有密钥，不改变插件信任关系。
- **外部复核补充已处理**：私钥打包、后台投递、配置导入、运行时错误识别已修复；删除未使用的 `src/main.js`。索引防回滚仍需协调发布流程，列为 S16。
- **剩余依赖未强行替换**：glib 涉及 Linux 图形栈；block-buffer 的公告适用性及实际可达性待确认；另有两项维护状态提醒。替代方案见第 5 节。
- **验证通过**：73 项 Rust 测试、80 项前端测试及工作区构建；Edge 中真实 Blob Worker 隔离验证通过。

这些结论适用于当前工作区源码与锁文件。现有 `dist/` 成品未替换，不代表已验证所有平台、所有插件或所有发布产物均安全。

## 2. 项目概况、范围与方法

### 2.1 技术栈与依赖规模

| 项目 | 审核快照 |
| --- | --- |
| 语言与框架 | Rust 2021、Tauri 2；原生 HTML / CSS / JavaScript |
| 插件模型 | 沙箱 iframe 界面；基于 stdio JSON-RPC 的独立后端进程 |
| 包管理器 | Cargo workspace（core、CLI、桌面宿主）；npm（jsdom 测试） |
| 外部直接依赖 | Rust 17 个不同包；npm 1 个测试依赖 |
| 锁定依赖 | Cargo.lock 469 个包记录，含 3 个本地包、466 个外部包；npm 39 个包记录 |
| 完整版本清单 | 附录 A / B 及 [机器可读依赖快照](SECURITY_DEPENDENCIES_2026-10-29.json) |

未发现独立的 Python、Go、Java 等依赖清单；项目不包含数据库、独立 Web 服务或浏览器扩展。插件可使用的 Python / Node 运行时及第三方插件依赖，不等同于宿主自身的锁定依赖，不能据此报告一并安全。

### 2.2 审核范围

- 当前 `nuthusk` 工作目录内的宿主、CLI、SDK、配置、测试、文档和依赖清单；初审覆盖 39 个主要源码与配置文件，后续覆盖新增正则隔离模块及配套测试。
- 凭据初审扫描覆盖 76 个工作树文件、4 个 Git 历史提交中的 148 个 blob，以及 10 个分发 / 顶层构建成品。这些数字是扫描当时的范围，不是重整报告时新增文件后的总数。
- 不将第三方 `node_modules` 源码和编译器增量缓存当作项目自有代码逐文件审查；不包含全部第三方插件的安全审核。
- 相邻 NutHusk-Plugins 仓库未作修改、提交或发布；仅以两份原有发布包进行新 CLI 的签名兼容性验证。
- 保留既有未提交改动；本次安全工作没有提交或推送代码。

### 2.3 方法与证据口径

依赖审查按直接依赖名称检索公开安全资料，优先使用 RustSec、OSV、GitHub 官方安全公告和维护者信息；以 Rust semver 对锁定版本匹配 RustSec 公告并排除撤回项，升级后通过 OSV querybatch 查询 505 个 crates.io / npm 包版本，同时运行 npm audit。

代码审查覆盖输入来源、信任边界、资源限制、路径与解压、签名、消息通信、日志、配置导入及运行时启动。凭据检查同时覆盖工作树、所扫描 Git 历史和现有成品。采用脱敏输出，不在报告中记录密钥内容。

本机未安装 cargo-audit、gitleaks、trufflehog，未声称运行这些工具；RustSec 匹配器和脱敏扫描脚本位于忽略目录 `target/security-audit/`。版本命中、已证明的代码缺陷和维护状态提醒分别记录，无法确认的事项标注“待确认”。

## 3. 问题汇总

严重级别保留发现时的风险，用于说明修复优先级，不代表已修复问题仍可利用。D01 的 Critical 来自上游公告最高评级；未证明 NutHusk 曾存在对应的 Critical 远程利用链。表内位置为审核与修复入口，精确版本以锁文件和依赖快照为准。

| 编号 | 类别 | 严重级别 | 问题 | 位置 | 状态 |
| --- | --- | --- | --- | --- | --- |
| S01 | 代码 | High | 已安装插件签名无效仍可加载/调用 | `crates/core/src/manager.rs` | 已修复 |
| S02 | 代码 | High | SVG 插件文档缺少 CSP，可绕过声明的联网限制 | `src-tauri/src/main.rs` | 已修复 |
| S03 | 代码 | Medium | SDK 接受非父窗口的伪造消息 | `sdk/ui.js` | 已修复 |
| S04 | 代码 | Medium | 后端无界行读取与请求堆积导致内存耗尽 | `crates/core/src/manager.rs` | 已修复 |
| S05 | 代码 | Medium | CLI 后端写入不受超时保护，响应无界读取 | `crates/core/src/devkit.rs` | 已修复 |
| S06 | 代码 | Medium | ZIP 重复/平台别名路径造成检查与落地歧义 | `crates/core/src/package.rs` | 已修复 |
| S07 | 代码 | Medium | 并发安装共用暂存路径、缺少修改互斥 | `crates/core/src/manager.rs` | 已修复 |
| S08 | 代码 | Low | 插件原始输出自动转发控制台/错误原文 | `crates/core/src/manager.rs` | 已修复 |
| S09 | 密钥 | Low | 私钥创建存在覆盖竞争和 Unix 权限窗口 | `crates/cli/src/main.rs` | 已修复 |
| S10 | 代码 | Medium | 拖放配置绕过文件导入大小/版本/条目限制 | `src/js/profile.js` | 已修复 |
| S11 | 代码 | Low | 运行时配方允许特殊版本/文件路径 | `crates/core/src/runtime.rs` | 已修复 |
| S12 | 代码 | Medium | 正则移至可终止 Worker，超时不再阻塞 UI | `src/js/regex-matcher.js` / `src/js/regex-worker.js` | 已修复 |
| D01 | 依赖 | Critical | zip 2.6.1 → 9.1.3，3 条公告不再命中 | `Cargo.lock` / `crates/core/Cargo.toml` | 已修复 |
| D02 | 依赖 | Medium | glib 0.18.5：Linux 间接依赖中的未定义行为 | `Cargo.lock` | 待开发者决定 |
| D03 | 依赖 | Medium | block-buffer 0.10.4：OSV 命中，可达性/版本范围待确认 | `Cargo.lock` | 建议关注（待确认） |
| D04 | 依赖 | Low | proc-macro-error 1.0.4 停止维护 | `Cargo.lock` | 建议关注 |
| D05 | 依赖 | Low | whatwg-encoding 3.1.1 已弃用（测试依赖） | `tests/package-lock.json` | 建议关注 |
| K01 | 密钥 | Low | 开发者确认私钥从未公开分发；保留密钥，关注存放位置 | `dist/my-signing-key.key` | 建议关注 |
| K02 | 密钥 | High | pack 会把插件目录内的私钥原样打包 | `crates/core/src/devkit.rs` | 已修复 |
| S13 | 代码 | Medium | 后台插件可通过 sendTo / 快捷键抢占宿主界面 | `src/js/main.js` | 已修复 |
| S14 | 代码 | Medium | 配置导入混用源、回退版本且未充分展示 URL | `src/js/profile.js` | 已修复 |
| S15 | 代码 | Low | 插件错误文本可伪造 RuntimeMissing 提示 | `crates/core/src/manager.rs` / `src-tauri/src/main.rs` / `src/js/main.js` | 已修复 |
| S16 | 代码 | Low | 已签名索引缺少防回滚的持久化序号校验 | `crates/core/src/market.rs` | 待开发者决定 |
| S17 | 代码 | Low | 未使用的旧入口仍随前端资源嵌入 | `src/main.js`（已删除） | 已修复 |

## 4. 已修复的问题与具体改动

### S01：无效签名的执行边界（High）

利用条件：已安装的已签名插件被修改，或无效签名插件目录被手动放入扫描路径。原逻辑只改变 trust 标记，error、plugin_dir 和 invoke 仍可能放行；原有测试只检查标记，未检查执行拒绝。现在扫描将 Invalid 信任转为不可用错误，页面和后端入口共同阻断；启动新后端前重新校验内容及签名，并拒绝把本次扫描已知的 Trusted 静默降级成缺失签名的 Unverified。增加扫描后篡改、删除签名及页面访问拒绝测试。仍存在具有本机写权限的进程在校验与执行间竞争的常规 TOCTOU 边界，未引入 OS 沙箱或永久身份绑定。

### S02：非 HTML 活跃文档的 CSP（High）

利用条件：安装一个以 SVG 等活跃文档作为 ui 的插件。原先只给 text/html 响应加 CSP，SVG 可包含脚本并绕过默认的 connect-src 限制。现在插件文件响应统一带 CSP，并增加 X-Content-Type-Options: nosniff、base-uri 和 object-src 限制。原生协议单元测试确认 SVG 也携带禁止联网策略；未声称验证了所有 WebView 的导航行为。

### S03：SDK 消息来源（Medium）

利用条件：其他窗口能取得插件窗口引用并发送 postMessage。原 SDK 不检查 e.source，可伪造响应、输入或动作。现在仅接受父窗口且消息须为对象。沙箱 iframe 的 origin 是不透明来源，不能用固定 origin 代替窗口身份校验；向不透明来源发送消息使用 * 本身不等于来源认证。新增真实 iframe WindowProxy 的 jsdom 回归。

### S04 / S05：不可信后端输出与超时（Medium）

利用条件：已授权插件异常或恶意输出超长无换行文本，或拒绝读取 stdin。现在桌面后端每条 RPC 请求/响应上限为 16 MiB，stderr 行上限为 64 KiB，单后端最多 128 个等待请求；超长 stdout 会关闭后端并失败挂起请求。日志仍最多 500 行、每行 2000 字符。CLI 的 5 秒期限现在同时覆盖 stdin 写入和 stdout 响应，响应也限定为 16 MiB，并丢弃 stderr 防止管道阻塞。新增超长输出与阻塞写入测试。这是资源保护，不是恶意本地进程的 CPU、子进程或磁盘配额。

### S06 / S07：安装包路径与暂存竞争（Medium）

利用条件：构造含重复逻辑路径、Windows ADS/设备文件名/尾点空格别名的 ZIP，或并发安装多个包。现在拒绝重复路径（Windows 按 ASCII 大小写归一）、反斜杠 ZIP 路径、非普通路径段以及常见 Windows 特殊文件名；包读取和暂存均执行 300 MiB 上限。暂存复制使用 create_new、PID+序号及 Unix 0600，避免预先放置的文件被覆盖；同一 PluginManager 的安装和卸载串行化。继续保留既有符号链接拒绝、单文件/总解压量限制、权限掩码和回滚。新测试覆盖逻辑重复路径、危险名称和并发安装。

### S08：原始内容日志泄露（Low）

插件 stderr 原来自动打印到宿主 stderr，非 JSON stdout 和 CLI 解析错误也可能携带用户处理原文。现在 stderr 仅进入受限的内存日志；无效 stdout 只记录格式错误，CLI 解析错误不回显原始响应。用户主动使用 CLI run 得到的正常结果仍写入 stdout。未承诺自动清洗插件主动输出到日志中的所有秘密。

### S09：私钥文件创建（Low）

CLI keygen 原来的 exists 后 write 存在竞争窗口，Unix 先以默认权限写再 chmod。现在公私钥均使用 create_new；Unix 私钥从首次创建即使用 0600，写后 sync_all。Windows 使用目录继承 ACL，本轮未编造“已强制仅本人可读”的结论。加入 .env、.env.*、*.key、*.p12、*.pfx 的忽略规则，保留 .env.example 例外。

### S10 / S11：配置导入与配方路径（Medium / Low）

拖放 .nutprofile 原先直接走 JS 解析，绕过 Rust 文件读取路径的 2 MiB、条目数量及版本规则。现在读文件前检查大小，JSON 解析前检查 UTF-8 大小，并要求 version 严格等于数字 1，限制源/插件/运行时分别最多 50/500/50 项。运行时配方版本也必须为安全路径段，拒绝 .、.. 和危险平台别名；解压路径复用同样的便携路径约束。增加对应回归。

### S12：插件正则 ReDoS（已修复）

开发者已授权在保留功能的前提下隔离正则执行。新增 regex-worker.js / regex-matcher.js：保留完整 JavaScript 正则语法，将插件模式在独立 Worker 中执行；每条表达式限时 150 ms，超过后终止当前 Worker，跳过该表达式并继续后续规则，已经命中的插件结果会保留。前缀、长度和 JSON 识别仍即时执行；输入变化或关闭面板取消旧 Worker，过期结果不能覆盖新查询。

Worker 启动单独设置 2 秒期限；使用只包含仓库固定代码的 Blob Worker，避免自定义 WebView scheme 的 Worker URL 兼容问题，并在 CSP 中明确允许 worker-src self / blob。用户文本只通过 postMessage 传入，不拼接进 Worker 源码。结束时回收 Worker 与 Blob URL。

命令面板增量更新正则命中，兼容结果返回前按回车的纯正则插件打开。新增真实 worker_threads 测试覆盖恶意回溯、后续规则继续、无效正则、完整语法、长度过滤、取消、快速回车和过期结果。另在 Edge 无头浏览器中按应用同等 CSP 验证真实 Blob Worker：慢表达式被终止、后续规则仍命中、主线程定时器继续运行。没有声称已完成 macOS/Linux 原生 WebView 动态测试。

### D01：zip 2.6.1 → 9.1.3（已修复）

升级前 RustSec 命中 [RUSTSEC-2026-0367](https://rustsec.org/advisories/RUSTSEC-2026-0367.html)（High，SUID/SGID）、[RUSTSEC-2026-0368](https://rustsec.org/advisories/RUSTSEC-2026-0368.html)（符号链接目标逃逸，无公告 CVSS）和 [RUSTSEC-2026-0369](https://rustsec.org/advisories/RUSTSEC-2026-0369.html)（Critical，符号链接权限写入）。公告列出的修复线均为 >=9.0.0。

实际使用核对：本项目逐条读取、检查并手动写出 ZIP 内容，不调用 ZipArchive::extract、ZipStreamReader::extract 或流式解压 API；同时拒绝符号链接并把 Unix 权限屏蔽到 0777。升级前未发现上述利用路径可达，但旧版本仍存在包级告警，因此在开发者授权后升级。

开发者已授权升级，现统一使用 zip 9.x 并锁定 9.1.3。已适配可失败的文件名解析、文件名迭代 API，并在库归一化路径之前检查原始条目名称，保持旧有严格路径限制。RustSec 和 OSV 对新版本均无 zip 命中；全部 Rust 测试通过，插件仓库中两份原有发布包也通过新 CLI 的可信签名验证，.nut 格式和签名协议保持兼容。

### K02：打包私钥拦截（High）

利用条件：开发者将有效私钥误放在插件目录内，并把生成的插件包交给其他人。复核使用无效的合成私钥标记确认旧打包器会原样收入 `.key` 文件；这不代表实际私钥已经泄露。`.gitignore` 的 `*.key` 已生效，但不会控制 CLI 打包。

现在对将要打包的文件进行内容扫描，发现 `nuthusk-ed25519-private:` 就中止，仅输出文件路径，不回显秘密内容。预检在创建或覆盖目标包之前完成，支持嵌套目录、任意扩展名、二进制内容和跨读取块的标记；实际写入 ZIP 前再次检查本次读取的字节。普通 `.key` 数据和公钥不按扩展名误删，原输出包在预检失败时保持不变。此措施只识别 NutHusk 私钥标记，不声称是所有凭据格式的通用扫描器。

签名文档改为先构建 CLI，再到仓库与插件目录之外的私有目录生成密钥。keygen 默认路径保持兼容，不移动、不轮换开发者现有密钥。

### S13：跨插件投递与后台快捷键（Medium）

利用条件：已打开的插件 iframe 在切到后台后继续运行，并发送 sendTo 或伪造快捷键消息。此前只验证 iframe 身份，没有检查当前是否可见。

现在 sendTo 仅接受主活动标签或实际可见分屏的 iframe；有模态对话框或投递面板时拒绝新的投递，全局最多每 500 ms 接受一次，切换消息来源也不能绕过限频。目标插件须可用；后台 SDK 快捷键也被拒绝，避免绕过 sendTo 限制直接弹出面板。主标签、分屏投递和用户主动打开的面板继续可用。

本次不强制旧插件新增“允许接收”字段，避免中断现有协议。可见性和限频不等同于可信用户手势；仍然可见的恶意插件并未被赋予 OS 沙箱约束。

### S14：配置导入来源与精确版本（Medium）

利用条件：用户导入恶意或被修改的配置清单，清单添加私有源或直链，并指定待安装插件。此前源名称可掩盖真实地址；同 ID 的任意版本回退可能下载非预期插件。

现在新增源的初始确认中显示完整索引 URL，并标注由本次清单添加；候选源按官方优先，指定版本无匹配时跳过，禁止回退到其他版本。选中私有源或清单直链后，下载前独立展示完整索引 URL（如适用）、包 URL 和来源说明，用户可跳过。下载后再次检查实际包的 ID 与指定版本，不符则丢弃暂存包、不进入安装。清单直链只使用它自身提供的哈希，不挪用同 ID 索引条目的哈希；之后仍走原有签名、权限与安装确认。

已有插件的版本不符合清单时仍保留当前安装并报告“部分同步”，不擅自降级。自动恢复新增了必要的来源确认步骤；这不代表免除对第三方插件代码的审查。

### S15：结构化运行时错误（Low）

利用条件：插件后端在 JSON-RPC 错误 message 中写入 RuntimeMissing 前缀与伪造 JSON。此前前端通过字符串搜索识别宿主缺运行时错误，可被诱导显示宿主对话框。

现在核心新增 InvokeError / invoke_checked：宿主运行时解析缺失环境时才构造 RUNTIME_MISSING 与 detail；后端错误无论包含何种前缀、code 或 JSON 文本，都只转成 CALL_FAILED 的 message。桌面 IPC 传递结构化错误，前端不再解析错误字符串；旧 invoke 接口继续提供普通字符串错误，SDK 也继续接收可读的错误消息。真实缺失环境的确认、下载与自动重试仍可用。

真实 Node 后端回归确认无法伪造宿主 code，另用本地测试配方验证真实缺失错误的序列化；前端覆盖伪造字符串、普通结构化错误及真实下载重试流程。

### S17：旧前端入口清理（Low）

当前 index.html 只引用 src/js/main.js。删除不再使用的 src/main.js，防止它继续随整个 src 目录嵌入成品。本项属于冗余资源清理，未证明旧文件本身存在可达攻击路径。

## 5. 剩余依赖告警、替代方案与决策建议

### D02—D05：保留的依赖告警与维护事项

- **glib 0.18.5**：[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) / GHSA-wrw7-89jp-8q8g，修复 >=0.20.0。经 GTK/WebKit/托盘链引入，属于 Linux 图形栈；当前 Windows 产物不编译该平台链。扫描锁定依赖的 13,781 个 Rust 源文件，未在 glib 定义之外找到 VariantStrIter / array_iter_str 引用。这是源码检索证据，不是完整 Linux 调用图或动态验证。不要用 Cargo 强制覆盖 ABI 不兼容版本；建议等上游兼容迁移或独立维护审核过的回补。
- **block-buffer 0.10.4**：OSV 命中 [GHSA-qwgh-2vcv-g2f7](https://github.com/advisories/GHSA-qwgh-2vcv-g2f7)，公告给出修复 0.12.1。公告以 block_buffer 名称收录，示例为 panic 后内联游标损坏；实际 0.10.4 的游标为独立 pos 字段，未使用 ReadBuffer。经 sha2/digest 引入，项目未使用自定义压缩回调或 catch_unwind 后复用缓冲区，release 设置 panic=abort。版本范围与实际代码的一致性待确认，不能把版本命中当成已证实内存破坏；升级需要协调 RustCrypto 上游，不擅自跨不兼容版本替换。仅升级本项目的 sha2 或改用 ring 不足以清除旧链，因为 ed25519-dalek / curve25519-dalek 和 tauri-codegen 也通过 digest / sha2 引入它。
- **proc-macro-error 1.0.4**：[RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370.html) 标记停止维护，无公告修复版本，经 Linux glib-macros 的构建链引入。建议随上游迁移，或单独评估回补/替代方案，本轮不删除平台功能。
- **whatwg-encoding 3.1.1**：npm 官方 deprecated，建议用 @exodus/bytes；它属于 jsdom 测试依赖，不随桌面程序分发。没有已知漏洞命中，不为消除弃用提示擅自大版本升级 jsdom。

### 间接依赖替代方案与建议

| 依赖 | 当前建议 | 可选替代或回补方式 | 代价与限制 |
| --- | --- | --- | --- |
| glib 0.18.5 | Windows 发布保留当前图形栈并跟踪上游；Linux 发布前单独评估 | 回补官方 PR #1343 到受控的 0.18.5 源码，固定补丁来源并做 Linux 测试；或随 Tauri/Wry/GTK 绑定协调升级 | 不能在根项目把 glib 直接替换成 0.20/0.22；旧绑定要求 0.18。保留旧版本号的安全回补仍可能触发版本扫描，需要带证据的单条例外 |
| block-buffer 0.10.4 | 暂不更换加密实现，保留“范围/可达性待确认”结论 | 待 RustCrypto / Tauri 相关上游协调采用新的 digest/block-buffer；确有触发路径再审查精确回补 | 根项目添加一个新版依赖不会替换其他消费者的旧链；替换 hash 库也无法消除签名库和构建工具引入的旧版本 |
| proc-macro-error 1.0.4 | 随 Linux 宏依赖链升级，不单独做冒险替换 | 上游或自维护的 glib-macros 可迁移 manyhow / proc-macro2-diagnostics | 它是别的宏 crate 的实现依赖，需要改消费方；proc-macro-error2 也已停止维护，不推荐作为替代 |

上游已有 [GTK3 绑定 0.19.0](https://github.com/gtk-rs/gtk3-rs/releases/tag/0.19.0)，依赖 gtk-rs-core 0.22，说明替代路线不必仅限 GTK4；但当前 Tauri 图形栈仍锁定旧绑定，需要上下游一起适配。官方 glib 修复见 [PR #1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343)，框架约束见 [Tauri #12564](https://github.com/tauri-apps/tauri/issues/12564)。proc-macro-error2 的维护状态见 [RUSTSEC-2026-0173](https://rustsec.org/advisories/RUSTSEC-2026-0173.html)。本轮未对这些间接依赖做未经授权的 fork、例外屏蔽或框架迁移。

### S16：签名索引防回滚（Low，待决定）

签名只能证明内容来自相应密钥，不能证明索引是最新版本。当前客户端没有保存并比较索引序号；攻击者若能替换发布内容或相关缓存，可能重放旧索引和它的有效签名。普通网络旁观者不能因此直接绕过 HTTPS。

建议在被签名的索引内容中加入递增 serial，并按源保存已接受的最高序号；同序号不同内容也应拒绝。需同时决定旧索引兼容期、首次安装的基线、缓存删除后的行为以及发行端序号管理，不能只增加 generatedAt 字段就宣称防回滚完成。此项需要同步调整 NutHusk-Plugins 发布流程，本轮未改相邻仓库、未增加强制序号限制，保留为独立决策事项。

## 6. 密钥与凭据检查

### K01：私钥的保管范围（开发者已确认）

开发者确认：实际私钥从未公开分发，仅由需要使用的授权人员保留。结合该私钥未被 Git 跟踪、所扫描 Git 历史未发现该私钥的结果，本项不再作为已泄露凭据处理；保留原密钥与公钥，不删除、不轮换，也不改变现有插件与索引的信任关系。

`dist/my-signing-key.key` 仍属于本地敏感材料，已被目录及 *.key 规则忽略；建议后续将签名材料与分发成品分开存放。这是发布整理建议，不表示已发生泄露。本轮没有移动密钥或改动依赖它的本地脚本。

### 其他凭据与忽略规则

在已说明的扫描范围内，未确认其他硬编码 API Key、Token、数据库凭据或 Webhook 秘密。未发现需要从源码迁移到环境变量的有效应用凭据，因此没有为不存在的配置新增空泛 `.env.example`。

已补充 `.env`、`.env.*`、`*.key`、`*.p12`、`*.pfx` 忽略规则，并保留 `!.env.example` 例外。忽略规则只影响未跟踪文件，不能撤回已提交材料，也不能替代目录访问权限和发布清单检查。本次私钥未被 Git 跟踪，所扫描历史未发现该私钥；历史检查范围以第 2 节为准。

## 7. 其他审查结果与剩余风险

| 范围 | 结论与边界 |
| --- | --- |
| 注入与 DOM | 未发现项目使用 SQL/NoSQL、eval、动态 Function 或用户输入拼接 shell；后端启动使用程序名与独立参数。UI 文本使用 textContent；图标 innerHTML 仅来自内置静态常量。 |
| 路径与解压 | 插件包拒绝逃逸与符号链接，实际解压字节限流；协议资源做 canonicalize 后检查根目录。本轮补齐平台别名和并发安装。仍需防御可写目录中的同用户竞争。 |
| 网络与 TLS | 使用 HTTPS，拒绝 URL 内用户名/密码，手动逐跳校验重定向；未发现关闭 TLS 验证。私有源允许内网 HTTPS 是现有功能，不擅自禁用。 |
| 加密 | 核心摘要为 SHA-256，签名为 Ed25519，密钥种子来自 OsRng；未发现静态 IV、自造加密或核心认证使用 MD5。 |
| IPC 与 WebView | 主界面限定 main 窗口能力，无远程能力 URL；插件 iframe 为 allow-scripts 且无 allow-same-origin，宿主已有按 iframe 身份绑定后端调用/动作结果。本轮补齐 SDK 父窗口来源校验和 SVG CSP。仍需 Windows/macOS/Linux 原生 WebView 导航、协议 CORS 与进程注入的动态测试。 |
| CORS / SSRF | 自定义协议使用 * 以服务不透明沙箱来源，不能把它机械判定为已可从公网读取本机文件；它也意味着联网插件的资源隔离应进一步按原生浏览器实测。用户确认的私有源/直链可触发 HTTPS 请求，内网及 DNS 重绑定策略需结合产品需求再决定。 |
| 后端权限 | 后端是当前用户的普通进程，可访问该用户的文件、网络和继承的环境变量。签名与 Python/Node 运行时隔离不是 OS 安全沙箱；需要严格隔离时另行设计权限/容器策略。 |
| 配置与数据 | 未发现应用后端凭据明文配置；工具日志可能含插件主动写入的数据，不能承诺插件级零留存。配置文件读取仍信任本机用户拥有的数据目录。 |
| CI/CD | 当前仓库没有 .github/workflows、GitLab CI 或其他可执行流水线配置，未发现 pull_request_target 或第三方 Action 固定版本问题；未来增加 CI 应使用最小权限、固定提交和安全发布检查。 |
| 发布产物 | 已有 dist/ 成品未自动替换，发布前需重新构建修复版本并确保分发内容不含私钥；本轮只验证 Windows 主机的开发构建，未签署可直接发布的安全结论。 |

没有发现疑似仿冒包名称或非标准仓库来源；这不是对所有 466 个外部 Rust 包的完整供应链来源证明。建议后续引入持续 RustSec/OSV/npm 审计和发布目录允许清单，防止本地签名材料被误打包进公开分发包。

## 8. 验证结果与证据

下表整理本次安全修复阶段实际完成的验证；本轮补充修复后已重新运行 Rust / 前端全量测试、编译检查及工作区构建；既有依赖扫描和 Edge Worker 证据沿用本次审核前序阶段结果，没有声称重新扫描了漏洞数据库。

| 验证项 | 结果 | 证明范围 |
| --- | --- | --- |
| `cargo test --workspace` | 73 项通过 | 核心、CLI、桌面协议及集成回归 |
| `npm test`（在 `tests/` 下） | 80 项通过 | 界面、SDK、导入与正则隔离回归 |
| `cargo build --workspace` | 通过 | 当前 Windows 主机开发构建 |
| `cargo check --workspace` | 通过 | 当前工作区类型与编译检查 |
| `git diff --check` | 通过 | 补丁空白格式检查 |
| `npm audit --json` | 0 条已知漏洞 | 39 个 npm 测试依赖，不涵盖未知漏洞 |
| 升级后 RustSec / OSV 匹配 | zip 无命中；保留 D02 / D03 / D04 | 版本级结果，不等同于完整利用分析 |
| 既有 `.nut` 发布包 | hash-calc 与 UUID 两份包的可信签名验证通过 | ZIP 升级后的包读取与签名兼容性 |
| Edge 真实 Blob Worker | 恶意正则超时后仍命中后续规则，主线程定时器继续运行 | Chromium / CSP 冒烟验证，不代替原生 WebView 跨平台测试 |

可提交、供复核的材料：本报告与 [SECURITY_DEPENDENCIES_2026-10-29.json](SECURITY_DEPENDENCIES_2026-10-29.json)。

本机忽略目录 `target/security-audit/` 保留以下辅助证据，不随 Git 提交，也不作为公开分发内容：

| 证据 | 内容 |
| --- | --- |
| `osv-after.json`、`zip-after-osv.json` | 升级后的 OSV 查询结果 |
| `advisory-db/`、`rustsec-findings.json` | 本地公告数据库及匹配证据；复核时应核对文件生成阶段与版本 |
| `glib-reachability.json` | 锁定依赖中相关 API 的源码检索证据 |
| `redos-proof.json`、`browser-regex-result.json` | 初始正则阻塞复现及浏览器隔离验证 |
| `source-scope.json`、`secrets-scan.json`、`key-status.json` | 扫描范围与脱敏凭据检查结果 |

## 9. 后续处理建议

| 优先级 | 事项 | 建议处理方式 |
| --- | --- | --- |
| 发布前 | 使用修复后的产物 | 重新构建实际分发程序；核对分发文件清单，排除私钥和本地审计材料 |
| Linux 发布前 | D02：glib | 选择协调升级图形依赖链或审核官方回补，并执行 Linux 构建与回归；不强行覆盖不兼容版本 |
| 持续跟踪 | D03：block-buffer | 确认公告版本范围及调用条件；有新证据后决定精确回补或协调升级，不为消除扫描提示替换整个加密栈 |
| 持续跟踪 | D04 / D05：维护状态 | 随消费方升级迁移，避免直接替换成同样停止维护或 API 不兼容的包 |
| 后续决策 | S16：索引防回滚 | 协调发行端 serial 与客户端最高序号持久化，明确旧索引兼容策略后实现 |
| 后续验证 | 原生 WebView 安全边界 | 补充目标平台的导航、协议访问、iframe 联网限制与 Worker 行为测试 |
| 持续维护 | 依赖和分发检查 | 定期审计 Cargo / npm 锁文件，建立发布文件允许清单 |

当前没有证据要求作废现有签名密钥；若后续发现实际未经授权的访问或公开泄露，再到对应信任与发布流程中执行撤销、更换和通知，不能只删本地文件。

## 附录 A：全部外部直接依赖

下表为实际解析到的直接版本；“无命中”只表示本轮数据库匹配未报告该版本，不表示不存在未知漏洞。工作区本地依赖 nuthusk-core 0.1.0 另计，src/js/package.json 只声明模块类型，无 npm 运行依赖。

| 包 | 实际直接版本 | 使用位置 | 本轮结论 |
| --- | --- | --- | --- |
| serde | 1.0.229 | core / desktop | 无命中 |
| serde_json | 1.0.154 | core / CLI / desktop | 无命中 |
| semver | 1.0.28 | core | 无命中 |
| zip | 9.1.3 | core / CLI tests | D01 已修复，RustSec / OSV 无命中 |
| sha2 | 0.10.9 | core | 本包无命中；间接 block-buffer 见 D03 |
| ed25519-dalek | 2.2.0 | core | 无命中；历史密钥配对漏洞修复线 >=2.0 |
| base64 | 0.22.1 | core | 无命中；锁文件另含 0.21.7 / 0.23.1 |
| tokio | 1.54.1 | core / tests | 无命中；已包含 1.54.0 Interval 问题修复 |
| ureq | 2.12.1 | core | 无命中；旧主版本本身不等于停止维护 |
| url | 2.5.8 | core / CLI | 无命中 |
| flate2 | 1.1.10 | core | 无命中 |
| tar | 0.4.46 | core | 无命中；已在 PAX 修复线 |
| rand_core | 0.6.4 | CLI | 无命中 |
| tauri | 2.12.5 | desktop | 无命中；已超过 IPC fetch 公告修复线 2.11.6 |
| tauri-build | 2.7.4 | desktop build | 无命中 |
| tauri-plugin-dialog | 2.8.3 | desktop | RustSec/OSV 无命中；未将来源无法验证的二手条目认定为漏洞 |
| percent-encoding | 2.3.2 | desktop | 无命中 |
| jsdom | 26.1.0 | npm tests | npm audit / OSV 无命中；间接弃用项见 D05 |

依赖公告交叉验证示例：[Tokio 1.54.0 修复公告](https://github.com/tokio-rs/tokio/security/advisories/GHSA-5wjr-9356-4wc8)、[Tauri IPC fetch 修复公告](https://github.com/tauri-apps/tauri/security/advisories/GHSA-w28w-mhc8-qvjv)、[ed25519-dalek 历史公告](https://rustsec.org/advisories/RUSTSEC-2022-0093.html)。

## 附录 B：关键间接依赖实际版本

| 包 | Cargo.lock 版本 |
| --- | --- |
| tauri-runtime | 2.12.2 |
| tauri-runtime-wry | 2.12.3 |
| wry | 0.57.1 |
| tao | 0.37.2 |
| tauri-plugin-fs | 2.7.1 |
| glib | 0.18.5 |
| proc-macro-error | 1.0.4 |
| rustls | 0.23.46 |
| rustls-webpki | 0.103.16 |
| ring | 0.17.14 |
| webpki-roots | 0.26.11, 1.0.11 |
| curve25519-dalek | 4.1.3 |
| digest | 0.10.7 |
| block-buffer | 0.10.4 |
| getrandom | 0.2.17, 0.3.4, 0.4.3 |
| time | 0.3.55 |
| serde_with | 3.24.0 |

npm 间接包的完整实际版本如下（其中 jsdom 是直接测试依赖）：

| 包 | 实际版本 |
| --- | --- |
| @asamuzakjp/css-color | 3.2.0 |
| @csstools/color-helpers | 5.1.0 |
| @csstools/css-calc | 2.1.4 |
| @csstools/css-color-parser | 3.1.0 |
| @csstools/css-parser-algorithms | 3.0.5 |
| @csstools/css-tokenizer | 3.0.4 |
| agent-base | 7.1.4 |
| cssstyle | 4.6.0 |
| data-urls | 5.0.0 |
| debug | 4.4.3 |
| decimal.js | 10.6.0 |
| entities | 6.0.1 |
| html-encoding-sniffer | 4.0.0 |
| http-proxy-agent | 7.0.2 |
| https-proxy-agent | 7.0.6 |
| iconv-lite | 0.6.3 |
| is-potential-custom-element-name | 1.0.1 |
| jsdom | 26.1.0 |
| lru-cache | 10.4.3 |
| ms | 2.1.3 |
| nwsapi | 2.2.28 |
| parse5 | 7.3.0 |
| punycode | 2.3.1 |
| rrweb-cssom | 0.8.0 |
| safer-buffer | 2.1.2 |
| saxes | 6.0.0 |
| symbol-tree | 3.2.4 |
| tldts | 6.1.86 |
| tldts-core | 6.1.86 |
| tough-cookie | 5.1.2 |
| tr46 | 5.1.1 |
| w3c-xmlserializer | 5.0.0 |
| webidl-conversions | 7.0.0 |
| whatwg-encoding | 3.1.1 |
| whatwg-mimetype | 4.0.0 |
| whatwg-url | 14.2.0 |
| ws | 8.22.0 |
| xml-name-validator | 5.0.0 |
| xmlchars | 2.2.0 |
