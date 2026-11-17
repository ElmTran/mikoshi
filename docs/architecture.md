# 架构与约定

M1 定下的工程结构和前后端边界。之后每个里程碑都按这里的规则加功能；规则要改，先改这份文档。

## 目录

| 路径 | 内容 |
|---|---|
| `crates/mikoshi-core/` | 核心库，不依赖 Tauri：`Secret` 类型、给前端看的数据类型（`model`）、搜索排序、打开和读取 KDBX（`vault`） |
| `crates/kdbx-compat/` | M0 的兼容性检查工具，同时是回归测试 |
| `src-tauri/` | 桌面程序：窗口、命令、当前选中和已解锁的库（`session.rs`）、设置文件、日志 |
| `src/` | 前端（React + TypeScript）：`api/` 是唯一和 Rust 通信的地方，`app/` 是页面和状态，`components/` 是界面部件，`icons/` 是 KeePass 标准图标，`i18n/` 是中英文文案，`styles/` 是样式 |
| `design/mockups/` | 选风格时的三套静态样稿，留作记录 |
| `design/icons/` | 69 个标准图标的预览页，由 `tools/icon-sheet.mjs` 生成 |
| `tests/fixtures/` | KDBX 测试库 |

## 前后端边界

**明文只留在 Rust 端。** 这是 features.md §6 的要求，落到代码上是四条规则：

1. **前端拿到的数据类型里没有秘密。** `EntrySummary`、`EntryDetail` 等类型定义在 `mikoshi-core/src/model.rs`，里面没有密码、受保护字段的值和 TOTP 密钥。受保护的自定义字段，`value` 是 `null`。存 TOTP 密钥的字段（`otp`、`TOTP Seed`、`TOTP Settings`、`TimeOtp-*`、`HmacOtp-*`）根本不出现在字段列表里，`reveal_field` 也拒绝它们。`mikoshi-core/src/vault.rs` 里有一条测试，用测试库把所有视图序列化成 JSON，检查里面不含任何受保护的值和 TOTP 密钥。
2. **复制、TOTP、自动输入都传条目 ID，由 Rust 端完成。** 前端调 `copy_field(entryId, key)`，值不经过前端。
3. **只有 `reveal_field` 会把一个秘密发给前端**，而且只在用户主动点"显示"（以及 M6 的"编辑"）时调用。前端把它存在组件状态里，切换条目、隐藏、锁定时立即丢弃。快速搜索窗口没有调用它的权限。
4. **秘密在 Rust 里用 `Secret` 类型。** 它的 `Debug` 和 `Display` 只输出 `[redacted]`，释放时擦除内存，并且故意没有实现 `Serialize`：想把它发给前端，只能显式调用 `.expose()`，这样所有出口都能搜到。

标题、用户名、网址、备注、标签、自定义图标属于元数据，照常发给前端，即使库的"内存保护"设置把它们标成受保护（规格 §6 的划分）。

### 打开和锁定

- 解锁在后台线程里做（`unlock` 是异步命令），KDF 要花一秒左右也不会卡住窗口
- keepass-rs 遇到某些坏文件会 panic（M0 的 K9），打开时用 `catch_unwind` 拦住，报"文件已损坏"
- KeePass 1 的 `.kdb` 文件明确拒绝；KDBX 3.1 分不清密码错误和文件损坏时报"主密码或密钥文件不对，也可能是文件已损坏"（M0 的 K5）
- 打开时不写任何文件，包括库本身。分组的折叠状态也只在内存里，不写回
- 锁定就是丢掉解锁后的 `Vault`：keepass-rs 用 `SecretBox` 存受保护的值，释放时擦除。前端随之卸载工作区，所有缓存一起丢掉
- 时间一律用 UTC 的 ISO 8601（`2026-11-12T06:20:00Z`），前端按本地时区显示；文件里没有的时间是 `null`
- 根分组列出回收站以外的全部条目，其他分组只列自己的条目；回收站排在根分组子分组的最后；搜索跳过回收站和关闭了搜索的分组（`EnableSearching`，未设置时继承父分组）

### 命令

前端能调用的全部命令。**加命令要改三处**：`src-tauri/src/commands.rs`（实现并注册）、`src-tauri/build.rs` 的 `COMMANDS`、`src-tauri/capabilities/` 里对应窗口的权限。没有在权限里列出的命令，对应的窗口调用不了。

| 命令 | 作用 | 主窗口 | 快速搜索 |
|---|---|---|---|
| `app_info` | 版本号、是否启用了云母背景 | ✓ | ✓ |
| `get_settings` | 读设置 | ✓ | ✓ |
| `set_settings` | 改设置（最近文件列表除外，见"设置文件"） | ✓ | |
| `vault_status` | 是否已锁定、选中的库、记住的密钥文件路径 | ✓ | ✓ |
| `select_database` | 选中一个库（最近文件列表里的）；已解锁的库会先锁定 | ✓ | |
| `choose_database` | 弹出系统的打开文件对话框选库 | ✓ | |
| `choose_key_file` | 弹出对话框选密钥文件，只返回路径 | ✓ | |
| `unlock` | 用主密码和/或密钥文件解锁选中的库 | ✓ | ✓ |
| `lock` | 锁定 | ✓ | |
| `forget_recent` | 从最近文件列表里移除（不动文件本身） | ✓ | |
| `list_groups` | 分组树 | ✓ | |
| `list_entries` | 某个分组的条目 | ✓ | |
| `search_entries` | 搜索（不含回收站） | ✓ | ✓ |
| `get_entry` | 条目详情（不含秘密） | ✓ | |
| `get_custom_icons` | 库里的自定义图标（图片数据） | ✓ | ✓ |
| `reveal_field` | 取一个字段的明文 | ✓ | |
| `copy_field` | 复制一个字段或当前验证码（`@totp`），见"剪贴板" | ✓ | ✓ |
| `get_totp` | 当前验证码和剩余秒数 | ✓ | |
| `open_url` | 用默认浏览器打开条目的网址（只限 http 和 https） | ✓ | |
| `show_quick_search` | 显示快速搜索框 | ✓ | |
| `hide_quick_search` | 隐藏快速搜索框 | | ✓ |

字段的 `key` 沿用 KeePass 的名字：`Title`、`UserName`、`Password`、`URL`、`Notes`，自定义字段用它自己的名字，`@totp` 表示当前验证码。

### 事件

Rust 端发给所有窗口，用来让多个窗口保持一致：

| 事件 | 什么时候发 | 内容 |
|---|---|---|
| `vault-changed` | 解锁、锁定、选中另一个库之后 | 新的库状态 |
| `settings-changed` | 设置保存之后（包括解锁后更新最近文件列表） | 新的设置 |
| `quick-shown` | 快速搜索框显示时 | 无；窗口收到后清空上次的输入 |

### 错误

命令失败时只返回一个错误码，例如 `{"code": "locked"}`，前端按当前语言显示对应的文案（`error.locked` 等）。错误里**不带任何用户数据**，所以可以放心写进日志或显示出来。错误码定义在 `src-tauri/src/error.rs`，前端的对应类型在 `src/api/types.ts`。

解锁相关的错误码：`emptyPassword`（没给主密码也没给密钥文件）、`noDatabase`、`fileNotFound`、`fileUnreadable`、`keyFileUnreadable`、`wrongKey`、`wrongKeyOrCorrupt`（KDBX 3.1）、`corrupt`、`unsupported`、`keePass1`、`notKeePass`。其他：`totpInvalid`（验证码设置无法识别）、`unsafeUrl`（不是 http/https 网址）、`clipboard`（剪贴板被别的程序占着）、`notYet`（功能在后面的里程碑接入）。

### 剪贴板

`src-tauri/src/clipboard.rs`，直接调 Win32 剪贴板。

- **秘密**（密码、受保护的自定义字段、验证码）：同时写入 `ExcludeClipboardContentFromMonitorProcessing`、`CanIncludeInClipboardHistory`（DWORD 0）、`CanUploadToCloudClipboard`（DWORD 0），不进 Win+V 历史和云剪贴板。写不进这几种格式就整个放弃，不会留下没有保护的密码
- 秘密复制后按 `clipboardClearSeconds` 清除；锁定和退出时也清除
- **只清我们放的内容**：复制后记下剪贴板序列号（Windows 每次改动剪贴板都会加一），清除时序列号没变才清，用户之后自己复制的东西不会被误清
- 用户名、网址等非秘密字段按普通文本复制，不定时清除（M1 定下的约定，见 `CopyResult`）
- 剪贴板被别的程序占着时重试约 150 毫秒，还不行就报 `clipboard`

### TOTP

`crates/mikoshi-core/src/totp.rs` 自己实现 RFC 6238，不用 keepass-rs 的（M0 的 K3、K4）。按 KeePassXC 的顺序读三种存法：`otp` 字段（`otpauth://` URI，或 KeeOtp 的 `key=…&step=…&size=…`）、KeePass 2 原生字段（`TimeOtp-Secret[-Hex|-Base32|-Base64]` 等）、老的 `TOTP Seed` + `TOTP Settings`。支持 SHA-1/256/512、4–10 位、任意周期，以及 Steam 验证码（`encoder=steam`、issuer 为 Steam，或 `TOTP Settings` 里的 `S`）。Base32 不区分大小写，空格、连字符和补齐的 `=` 都忽略。验证码只在 Rust 端算，前端只拿到码和剩余秒数。

### 搜索语法

`crates/mikoshi-core/src/search.rs`。空格分开的每个词都要匹配；`title:` `user:` `url:` `tag:` 把一个词限定在某个字段；引号让词里可以有空格（`title:"my bank"`）。每个词先按子串找，找不到再模糊匹配（字母按顺序出现、中间隔得不多，`gthb` 能找到 GitHub）。排序：标题开头 > 标题包含 > 用户名 > 网址 > 标签 > 备注 > 模糊匹配。前端 `src/text.ts` 的 `searchTerms` 用同样的规则拆词，用来高亮。

### 自动锁定

`src-tauri/src/autolock.rs`，所有锁定都走 `session::lock_app`：丢掉解锁的库、清掉剪贴板里我们复制的秘密、通知所有窗口。

- 空闲：每 2 秒用 `GetLastInputInfo` 查一次全局（不只是 Mikoshi）的键盘鼠标空闲时间，超过 `idleLockMinutes` 就锁定
- Win+L：一个隐藏的顶层窗口注册 `WTSRegisterSessionNotification`，收到 `WTS_SESSION_LOCK` 就锁定
- 睡眠和休眠：同一个窗口收到 `WM_POWERBROADCAST` 的 `PBT_APMSUSPEND` 就锁定（只有顶层窗口收得到这个广播，所以不用 message-only 窗口）
- 最小化（可选，默认关）：主窗口的 `Resized` 事件里检查是否最小化

### 打开网址

`open_url` 在 Rust 端取条目的网址：没有协议的补上 `https://`，解析后必须是 http 或 https 且有主机名，否则报 `unsafeUrl`。KeePass 允许的 `cmd://` 之类会运行程序，一律拒绝。用 tauri-plugin-opener 交给默认浏览器；日志里不写网址。

## 安全基线

| 项 | 做法 | 在哪 |
|---|---|---|
| 内容安全策略（CSP） | 只允许加载程序自带的资源；脚本和样式不允许内联；不允许任何远程地址 | `tauri.conf.json` 的 `app.security.csp` |
| 不加载远程资源 | 字体用系统自带的，图标是代码里的 SVG，没有任何外链 | — |
| 窗口权限 | 每个窗口只能调用它用得到的命令和窗口操作。实测：快速搜索窗口调用 `reveal_field`、`list_groups` 会被拒绝 | `src-tauri/capabilities/` |
| DevTools | release 构建不带 DevTools（没有开 Tauri 的 `devtools` 功能） | `src-tauri/Cargo.toml` |
| 远程调试端口 | release 构建启动时清掉 WebView2 的几个环境变量，别人没法靠设环境变量给 Mikoshi 开调试端口或换掉浏览器内核。debug 构建保留，开发时靠它驱动程序做检查 | `src-tauri/src/lib.rs` |
| 浏览器自带功能 | release 构建里屏蔽浏览器右键菜单（文本框除外）和浏览器快捷键（刷新、打印、查看源码、查找等） | `src/app/appShell.ts` |
| 日志 | 秘密用 `Secret` 类型，打不出明文；错误不带用户数据；前端的 ESLint 禁止 `console.log` | `secret.rs`、`error.rs`、`eslint.config.js` |
| 原型冻结 | `freezePrototype`，防止页面脚本被篡改内置对象后影响通信 | `tauri.conf.json` |
| panic 不终止进程 | release 保持 `panic = "unwind"`。keepass-rs 遇到某些坏文件会 panic（M0 的 K9），M2 会用 `catch_unwind` 拦住 | 根目录 `Cargo.toml` |
| 单实例 | 再次启动时把已有窗口切到前台，新进程退出 | `src-tauri/src/lib.rs` |
| 窗口不会"隐身" | 主窗口启动时先隐藏，界面画好再显示。如果界面加载失败，5 秒后照样显示，不会留下一个看不见的进程 | `src-tauri/src/windows.rs` |
| 启动失败有提示 | 界面连不上后台时显示"没能启动"和重试按钮，不会停在空白窗口 | `src/app/MainApp.tsx` |

### 启动顺序

**先注册后台状态，再创建窗口。** 页面在窗口还没创建完时就会开始调用命令，这时命令需要的东西必须已经就位。所以 `tauri.conf.json` 里两个窗口都是 `"create": false`，由 `setup` 在 `app.manage(...)` 之后调用 `windows::create_all` 创建。

顺序反了的后果只在正式构建里出现：页面瞬间加载完，头几个调用全部失败，窗口停在空白。开发模式下页面加载慢，碰巧不会出错。M1 就踩过这个坑，所以**改动启动流程后，一定要用正式构建验证一次**（见下面"检查正式构建"）。

开发模式的 CSP（`devCsp`）比正式的宽：Vite 的热更新需要内联脚本和一个本地 WebSocket。它只在 `pnpm tauri dev` 时生效。

## 设置文件

- 位置：`%APPDATA%\com.elmtran.mikoshi\settings.json`
- 目前的内容：
  - `theme`（`system` / `light` / `dark`）和 `language`（`system` / `zh-CN` / `en-US`）
  - `recent`：最近打开的库，最新的在前，最多 10 个。每项是 `path`，加上上次用的密钥文件路径 `keyFile`（只存路径）。启动时自动选中第一个
  - `rememberKeyFiles`：是否记住密钥文件路径，默认开。关掉时清空已经记住的路径
  - `entrySort`：条目列表的排序（`key` 是 `title` / `username` / `url` / `modified`，`desc` 是否倒序），默认按修改时间倒序
  - `clipboardClearSeconds`：复制的秘密多少秒后清除，默认 10，0 表示不清除
  - `idleLockMinutes`：全局无操作多少分钟后锁定，默认 5，0 表示不锁定
  - `lockOnMinimize`：主窗口最小化时锁定，默认关
- `recent` 由 Rust 端维护：解锁成功时更新，`forget_recent` 删除。`set_settings` 收到的 `recent` 会被忽略，免得某个窗口拿着旧列表把新列表覆盖掉
- **只放非敏感信息。** 之后会加热键
- 写入方式：先写临时文件再改名，中途崩溃不会留下半截文件
- 文件损坏或缺字段时用默认值，不认识的字段忽略，所以新旧版本可以互相读
- 日志在 `%LOCALAPPDATA%\com.elmtran.mikoshi\logs\`

## 界面

### 设计变量

所有颜色、字号、圆角、动画时长都定义在 `src/styles/tokens.css`，组件里不直接写颜色值。浅色和深色各一组，靠 `<html data-theme>` 切换。

风格是 Windows 11 原生（Fluent）加一条渐变强调色。**渐变只用在这几处**，其他地方保持系统的中性色，免得喧宾夺主：

- 主要按钮（自动输入、解锁）
- 选中项左侧的指示条
- 输入框获得焦点时的下划线
- 复制提示的图标和倒计时条、验证码的倒计时环
- 快速搜索框顶部的细线、解锁页的图标
- 窗口左上角很淡的一片底色

要调渐变，改 `tokens.css` 里的 `--grad-1`、`--grad-2`、`--grad-3` 和 `--wash`。

### 动效

时长和缓动曲线也是设计变量（`--t-fast` 到 `--t-slower`，`--ease` 等），沿用 Fluent 的节奏：进入快、收尾缓。系统开了"减少动画"时全部关闭。

### 深色模式

设置里有三个选项：跟随系统、浅色、深色。`src/theme.ts` 负责两件事：告诉系统窗口用哪种主题（云母背景和标题栏跟着变），以及设置 `data-theme`。跟随系统时，系统切换深浅色会立即生效。

### 图标

- 界面图标（复制、锁定等）在 `src/components/Icon.tsx`，KeePass 的 69 个标准图标在 `src/icons/keepass.ts`，两者同一种画法：24 × 24，1.6 像素圆头线条，颜色跟随文字
- 标准图标是 Mikoshi 自己画的（M2 确定），按 KeePass 的编号含义对应（`KEEPASS_ICON_NAMES`）。改了之后运行 `node tools/icon-sheet.mjs` 重新生成 `design/icons/index.html` 预览，或者在 `pnpm dev` 下打开 `/?window=icons`
- 条目图标画在彩色方块上（颜色由标题决定）；库里有自定义图标时显示自定义图标，图片加载失败就退回标准图标

### 键盘

- `Tab` / `Shift+Tab` 在分组树、条目列表、详情三栏之间切换；焦点在搜索框时，`Tab` 进入列表
- 分组树：上下方向键移动，左键折叠或回到上级，右键展开或进入第一个子分组，`Enter` 进入列表
- 条目列表：上下方向键、`Home`、`End`、`PageUp`、`PageDown`。焦点在搜索框里时上下方向键也能移动选中项
- 每一栏作为一个整体获得焦点，焦点框画在选中的那一项上

### 中英文

- 文案在 `src/i18n/zh-CN.ts` 和 `en-US.ts`，两边的键必须一致。缺键时 TypeScript 编译不过，另有测试检查占位符（`{n}` 等）一致
- 组件里用 `t('键')` 取文案，带数量的用 `tn('list.count', n)`，英文会自动选单复数
- 日期、文件大小用 `fmt.date()` 等，跟随当前语言
- 加一条文案：先在 `zh-CN.ts` 加，再在 `en-US.ts` 加同一个键
- 设置里的"跟随系统"：系统是中文就用中文，其他语言用英文

## 开发

```bash
pnpm install
```

```bash
pnpm tauri dev
```

```bash
pnpm check
```

依次是：装依赖；启动程序（改代码后自动刷新）；跑全部检查（类型、ESLint、clippy、格式、测试）。

只改界面时也可以运行 `pnpm dev`，然后用浏览器打开 `http://127.0.0.1:1420`。这时没有 Rust 端，`src/api/mock.ts` 用 `src/api/mock-vault.json` 里的演示数据顶替（任意密码都能解锁，打开文件对话框会直接选中演示库）；它只在开发模式下存在，不会进正式构建。浏览器里没有云母背景，窗口按钮也不起作用。

### 不动鼠标键盘检查真实窗口

`tools/cdp.mjs` 通过调试端口操作正在运行的程序：读界面状态、按键、点击、截图。它驱动的是真实的窗口和真实的 Rust 端，不会抢桌面的焦点。只对 debug 构建有效。

```bash
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'; pnpm tauri dev
```

```bash
node tools/cdp.mjs main shot target/shots/main.png eef1f6
```

第一条带调试端口启动程序，第二条给主窗口截图（最后的颜色用来代替截不到的云母背景）。其他用法见文件开头的说明。

### 检查正式构建

正式构建拒绝调试端口，`cdp.mjs` 连不上，改用两个只读的小脚本：`tools/window-state.ps1` 列出窗口是否可见、是否最小化、是否在前台；`tools/window-shot.ps1` 给窗口截图（窗口被别的窗口挡住也能截，但不能是最小化状态）。

```bash
pnpm tauri build --no-bundle
```

```bash
pwsh tools/window-shot.ps1 -Out target/shots/release.png
```

第一条生成 `target/release/mikoshi.exe`，运行它之后用第二条截图。要用调试端口检查"正式的前端文件"，可以用 `pnpm tauri build --debug --no-bundle`：前端是正式构建，程序本身是 debug 构建。
