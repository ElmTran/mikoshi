# 架构与约定

M1 定下的工程结构和前后端边界。之后每个里程碑都按这里的规则加功能；规则要改，先改这份文档。

## 目录

| 路径 | 内容 |
|---|---|
| `crates/mikoshi-core/` | 核心库，不依赖 Tauri：`Secret` 类型、给前端看的数据类型（`model`）、搜索排序。M2 起 KDBX 读写也放这里 |
| `crates/kdbx-compat/` | M0 的兼容性检查工具，同时是回归测试 |
| `src-tauri/` | 桌面程序：窗口、命令、设置文件、日志。`demo/vault.json` 是 M1 样稿用的假数据库，M2 换成真实的库 |
| `src/` | 前端（React + TypeScript）：`api/` 是唯一和 Rust 通信的地方，`app/` 是页面和状态，`components/` 是界面部件，`i18n/` 是中英文文案，`styles/` 是样式 |
| `design/mockups/` | 选风格时的三套静态样稿，留作记录 |
| `tests/fixtures/` | KDBX 测试库 |

## 前后端边界

**明文只留在 Rust 端。** 这是 features.md §6 的要求，落到代码上是四条规则：

1. **前端拿到的数据类型里没有秘密。** `EntrySummary`、`EntryDetail` 等类型定义在 `mikoshi-core/src/model.rs`，里面没有密码、受保护字段的值和 TOTP 密钥。受保护的自定义字段，`value` 是 `null`。`src-tauri/src/demo.rs` 里有一条测试，把每个条目的视图序列化成 JSON，检查里面不含密码和受保护字段。
2. **复制、TOTP、自动输入都传条目 ID，由 Rust 端完成。** 前端调 `copy_field(entryId, key)`，值不经过前端。
3. **只有 `reveal_field` 会把一个秘密发给前端**，而且只在用户主动点"显示"（以及 M6 的"编辑"）时调用。前端把它存在组件状态里，切换条目、隐藏、锁定时立即丢弃。快速搜索窗口没有调用它的权限。
4. **秘密在 Rust 里用 `Secret` 类型。** 它的 `Debug` 和 `Display` 只输出 `[redacted]`，释放时擦除内存，并且故意没有实现 `Serialize`：想把它发给前端，只能显式调用 `.expose()`，这样所有出口都能搜到。

### 命令

前端能调用的全部命令。**加命令要改三处**：`src-tauri/src/commands.rs`（实现并注册）、`src-tauri/build.rs` 的 `COMMANDS`、`src-tauri/capabilities/` 里对应窗口的权限。没有在权限里列出的命令，对应的窗口调用不了。

| 命令 | 作用 | 主窗口 | 快速搜索 |
|---|---|---|---|
| `app_info` | 版本号、是否启用了云母背景 | ✓ | ✓ |
| `get_settings` | 读设置 | ✓ | ✓ |
| `set_settings` | 改设置 | ✓ | |
| `vault_status` | 是否已锁定、库名和路径 | ✓ | ✓ |
| `unlock` | 解锁 | ✓ | ✓ |
| `lock` | 锁定 | ✓ | |
| `list_groups` | 分组树 | ✓ | |
| `list_entries` | 某个分组的条目 | ✓ | |
| `search_entries` | 搜索（不含回收站） | ✓ | ✓ |
| `get_entry` | 条目详情（不含秘密） | ✓ | |
| `reveal_field` | 取一个字段的明文 | ✓ | |
| `copy_field` | 复制一个字段 | ✓ | ✓ |
| `get_totp` | 当前验证码和剩余秒数 | ✓ | |
| `show_quick_search` | 显示快速搜索框 | ✓ | |
| `hide_quick_search` | 隐藏快速搜索框 | | ✓ |

字段的 `key` 沿用 KeePass 的名字：`Title`、`UserName`、`Password`、`URL`、`Notes`，自定义字段用它自己的名字，`@totp` 表示当前验证码。

### 事件

Rust 端发给所有窗口，用来让多个窗口保持一致：

| 事件 | 什么时候发 | 内容 |
|---|---|---|
| `vault-changed` | 解锁或锁定之后 | 新的库状态 |
| `settings-changed` | 设置保存之后 | 新的设置 |
| `quick-shown` | 快速搜索框显示时 | 无；窗口收到后清空上次的输入 |

### 错误

命令失败时只返回一个错误码，例如 `{"code": "locked"}`，前端按当前语言显示对应的文案（`error.locked` 等）。错误里**不带任何用户数据**，所以可以放心写进日志或显示出来。错误码定义在 `src-tauri/src/error.rs`，前端的对应类型在 `src/api/types.ts`。

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
- 目前的内容：`theme`（`system` / `light` / `dark`）和 `language`（`system` / `zh-CN` / `en-US`）
- **只放非敏感信息。** 之后会加最近打开的文件路径、密钥文件路径、热键、各种时长
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

只改界面时也可以运行 `pnpm dev`，然后用浏览器打开 `http://127.0.0.1:1420`。这时没有 Rust 端，`src/api/mock.ts` 用同一份假数据顶替；它只在开发模式下存在，不会进正式构建。浏览器里没有云母背景，窗口按钮也不起作用。

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
