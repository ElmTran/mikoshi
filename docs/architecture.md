# 架构与约定

M1 定下的工程结构和前后端边界。之后每个里程碑都按这里的规则加功能；规则要改，先改这份文档。

## 目录

| 路径 | 内容 |
|---|---|
| `crates/kdbx/` | 自己写的 KDBX 读写层（M6）：读 3.1/4.0/4.1，写 4.1，保存时只改动过的节点。不依赖本项目的其他部分 |
| `crates/mikoshi-core/` | 核心库，不依赖 Tauri：`Secret` 类型、给前端看的数据类型（`model`）、搜索排序、打开和读取 KDBX（`vault`，建立在 `kdbx` 上） |
| `crates/kdbx-compat/` | 兼容性检查工具：以 `keepassxc-cli` 为准检查 `kdbx` 的读写，同时是回归测试 |
| `src-tauri/` | 桌面程序：窗口、命令、当前选中和已解锁的库（`session.rs`）、托盘（`tray.rs`）、全局热键（`hotkeys.rs`）、设置文件、日志 |
| `src/` | 前端（React + TypeScript）：`api/` 是唯一和 Rust 通信的地方，`app/` 是页面和状态，`components/` 是界面部件，`icons/` 是 KeePass 标准图标，`i18n/` 是中英文文案，`styles/` 是样式 |
| `design/mockups/` | 选风格时的三套静态样稿，留作记录 |
| `design/icons/` | 69 个标准图标的预览页，由 `tools/icon-sheet.mjs` 生成 |
| `tests/fixtures/` | KDBX 测试库 |

## 前后端边界

**明文只留在 Rust 端。** 这是 features.md §6 的要求，落到代码上是四条规则：

1. **前端拿到的数据类型里没有秘密。** `EntrySummary`、`EntryDetail` 等类型定义在 `mikoshi-core/src/model.rs`，里面没有密码、受保护字段的值和 TOTP 密钥。受保护的自定义字段，`value` 是 `null`。存 TOTP 密钥的字段（`otp`、`TOTP Seed`、`TOTP Settings`、`TimeOtp-*`、`HmacOtp-*`）根本不出现在字段列表里，`reveal_field` 也拒绝它们。`mikoshi-core/src/vault.rs` 里有一条测试，用测试库把所有视图序列化成 JSON，检查里面不含任何受保护的值和 TOTP 密钥。
2. **复制、TOTP、自动输入都传条目 ID，由 Rust 端完成。** 前端调 `copy_field(entryId, key)`，值不经过前端。
3. **只有 `reveal_field` 和 `get_entry_for_edit` 会把秘密发给前端**，前者只在用户主动点"显示"时调用，后者只在打开编辑表单时调用。前端把它们存在组件状态里，切换条目、隐藏、关闭编辑、锁定时立即丢弃。快速搜索窗口没有调用它们的权限。
4. **秘密在 Rust 里用 `Secret` 类型。** 它的 `Debug` 和 `Display` 只输出 `[redacted]`，释放时擦除内存，并且故意没有实现 `Serialize`：想把它发给前端，只能显式调用 `.expose()`，这样所有出口都能搜到。

标题、用户名、网址、备注、标签、自定义图标属于元数据，照常发给前端，即使库的"内存保护"设置把它们标成受保护（规格 §6 的划分）。

### 打开和锁定

- 解锁在后台线程里做（`unlock` 是异步命令），KDF 要花一秒左右也不会卡住窗口
- 坏文件只会得到错误，不会 panic（`kdbx` 经过模糊测试）；打开时仍用 `catch_unwind` 兜底，万一 panic 也只报"文件已损坏"
- KeePass 1 的 `.kdb` 文件明确拒绝。KDBX 3.1 解密后先核对开头的校验字节，密码错误和文件损坏能分开（M0 的 K5 已解决）；只有 3.1 头部本身损坏时会报成密码错误，这是格式决定的（3.1 的头部在解密前没有校验）
- 打开时不写任何文件，包括库本身。分组的折叠状态也只在内存里，不写回
- 锁定就是丢掉解锁后的 `Vault`：`kdbx` 把受保护的值、附件和解密过程中的缓冲区放在释放时擦除的内存里（未受保护的字段是普通字符串）。前端随之卸载工作区，所有缓存一起丢掉
- 时间一律用 UTC 的 ISO 8601（`2026-11-12T06:20:00Z`），前端按本地时区显示；文件里没有的时间是 `null`
- 根分组列出回收站以外的全部条目，其他分组只列自己的条目；回收站排在根分组子分组的最后；搜索跳过回收站和关闭了搜索的分组（`EnableSearching`，未设置时继承父分组）

### 命令

前端能调用的全部命令。**加命令要改三处**：`src-tauri/src/commands.rs`（实现并注册）、`src-tauri/build.rs` 的 `COMMANDS`、`src-tauri/capabilities/` 里对应窗口的权限。没有在权限里列出的命令，对应的窗口调用不了。

| 命令 | 作用 | 主窗口 | 快速搜索 |
|---|---|---|---|
| `app_info` | 版本号、是否启用了云母背景、是否随开机启动（`startHidden`） | ✓ | ✓ |
| `get_settings` | 读设置 | ✓ | ✓ |
| `set_settings` | 改设置（最近文件列表除外，见"设置文件"）。热键改了先注册，注册不上就不保存，报 `hotkeyTaken` / `hotkeyInvalid` | ✓ | |
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
| `get_entry_for_edit` | 带全部值（含秘密）的条目，给编辑表单用；编辑结束就丢掉 | ✓ | |
| `create_entry` | 在分组里新建条目，返回 id（只在内存里） | ✓ | |
| `save_entry` | 应用一次编辑；没改动时返回 `false` | ✓ | |
| `list_history` | 条目的历史版本（旧到新） | ✓ | |
| `get_history_entry` | 一个历史版本，按条目详情的形式（不含秘密） | ✓ | |
| `reveal_history_field` | 历史版本里的一个字段明文 | ✓ | |
| `restore_history` | 把一个历史版本变成当前版本，现在的版本存进历史 | ✓ | |
| `save_database` | 写回文件；`options.force` 覆盖外部修改，`options.upgrade` 同意变成 4.1 | ✓ | |
| `reload_database` | 重新读文件，丢掉未保存的修改 | ✓ | |
| `quit_app` | 从主窗口退出（托盘退出走的也是这个）；`discard` 表示不要未保存的修改 | ✓ | |
| `deletes_permanently` | 删除这些条目/分组是不是永久的（在回收站里，或者库关了回收站）；是的话前端先确认 | ✓ | |
| `delete_entries` / `delete_group` | 删除：进回收站（没有就新建，名字由前端按语言给），或者在回收站里永久删除 | ✓ | |
| `restore_item` | 把条目或分组从回收站移回原处（原分组没了就回根分组） | ✓ | |
| `empty_recycle_bin` | 清空回收站 | ✓ | |
| `create_group` / `rename_group` / `move_group` | 分组的新建、改名、移动（不能移进自己或自己的子分组，也不能移进回收站） | ✓ | |
| `move_entries` | 把条目移到另一个分组 | ✓ | |
| `export_attachment` | 弹出保存对话框，Rust 端直接把附件写到选的文件，内容不经过前端 | ✓ | |
| `add_attachment` / `remove_attachment` | 加附件（打开对话框选文件，最大 256 MB）、删附件；都会先存历史版本 | ✓ | |
| `choose_new_database` | 新库的保存对话框；已经存在的文件一律拒绝（`fileExists`） | ✓ | |
| `create_key_file` | 在选的位置新生成一个 XML 2.0 密钥文件，只返回路径 | ✓ | |
| `create_database` | 新建库：校准 KDF、写文件、直接打开 | ✓ | |
| `generate_password` | 按选项生成密码或口令短语，返回结果、熵和强度；记住选项（不记结果） | ✓ | |
| `password_strength` | zxcvbn 强度估算（编辑器和新建库的强度条） | ✓ | |
| `copy_secret` | 把界面上已经显示的值（生成的密码）按秘密复制：不进剪贴板历史，定时清除 | ✓ | |
| `copy_field` | 复制一个字段或当前验证码（`@totp`），见"剪贴板" | ✓ | ✓ |
| `get_totp` | 当前验证码和剩余秒数 | ✓ | |
| `open_url` | 用默认浏览器打开条目的网址（只限 http 和 https） | ✓ | |
| `show_quick_search` | 显示快速搜索框 | ✓ | |
| `hide_quick_search` | 隐藏快速搜索框；`restore` 为真时把焦点还给之前的窗口 | | ✓ |
| `hotkey_status` | 两个全局热键的状态：`off` / `active` / `taken` / `invalid` | ✓ | |
| `pause_hotkeys` | 设置里录制新热键时暂停全局热键；保存热键也会恢复 | ✓ | |
| `get_autostart` | 是否开机启动（读 Windows 的启动项，不存在设置文件里） | ✓ | |
| `set_autostart` | 打开或关闭开机启动 | ✓ | |

字段的 `key` 沿用 KeePass 的名字：`Title`、`UserName`、`Password`、`URL`、`Notes`，自定义字段用它自己的名字，`@totp` 表示当前验证码。

### 事件

Rust 端发给所有窗口，用来让多个窗口保持一致：

| 事件 | 什么时候发 | 内容 |
|---|---|---|
| `vault-changed` | 解锁、锁定、选中另一个库之后 | 新的库状态 |
| `settings-changed` | 设置保存之后（包括解锁后更新最近文件列表） | 新的设置 |
| `quick-shown` | 快速搜索框显示时 | 无；窗口收到后清空上次的输入 |
| `hotkeys-changed` | 热键重新注册之后 | 新的热键状态 |
| `vault-edited` | 编辑、新建、恢复历史之后 | 新的库状态（含 `dirty`）；快速搜索框据此重新搜索 |
| `save-conflict-copy` | 锁定或退出时文件被别处改过，修改另存了一份 | 另存的文件路径 |
| `quit-requested` | 托盘点了退出，但设置是"有未保存的修改时问我" | 无；主窗口弹出对话框 |

托盘菜单在 Rust 端监听 `vault-changed` 和 `settings-changed`，跟着更新"锁定/解锁…"和语言。

### 错误

命令失败时只返回一个错误码，例如 `{"code": "locked"}`，前端按当前语言显示对应的文案（`error.locked` 等）。错误里**不带任何用户数据**，所以可以放心写进日志或显示出来。错误码定义在 `src-tauri/src/error.rs`，前端的对应类型在 `src/api/types.ts`。

解锁相关的错误码：`emptyPassword`（没给主密码也没给密钥文件）、`noDatabase`、`fileNotFound`、`fileUnreadable`、`keyFileUnreadable`、`wrongKey`、`corrupt`、`unsupported`、`keePass1`、`notKeePass`。其他：`fieldName`（自定义字段名不能为空、重复或占用标准/验证码字段名）、`expiry`（过期时间无法识别）、`unsavedChanges`（有未保存的修改，等用户决定）、`saveConflict`（文件被别处改过）、`saveFailed`（写不了文件或加密后读不回来）、`formatUpgrade`（保存会变成 KDBX 4.1，等用户同意）、`notAllowed`（这个操作不能用在这里，比如删除根分组）、`emptyName`（名字为空）、`fileExists`（新库或密钥文件的位置已经有文件）、`writeFailed`（文件写不进去）、`tooLarge`（附件超过 256 MB）、`noCharacters`（生成器一种字符都没选）、`totpInvalid`（验证码设置无法识别）、`unsafeUrl`（不是 http/https 网址）、`clipboard`（剪贴板被别的程序占着）、`hotkeyTaken`（热键被别的程序占用）、`hotkeyInvalid`（不是可用的热键）、`autostart`（改不了开机启动）、`notYet`（功能在后面的里程碑接入）。

### 编辑与保存（M7）

- 编辑在右侧详情栏进行：详情栏换成表单，条目列表照常。表单打开时一次性取走条目的全部值（`get_entry_for_edit`），关闭或锁定时整个组件卸载
- 核心库（`crates/mikoshi-core/src/vault/edit.rs`）里，编辑只改对应的节点：标准字段按 `MemoryProtection` 决定新字段是否受保护（密码总是），自定义字段可以改名、增删、单独改保护，标签用 `;` 分隔，过期时间写进 `Times`。TOTP 写成 `otp` 字段并删掉这个条目里其他验证码写法，避免两套设置算出不同的码
- 每次应用编辑前先把原版本存进历史（`push_history`），完成后按库里的 `HistoryMaxItems`、`HistoryMaxSize` 裁剪。没改动的编辑不产生历史，也不标未保存
- 未保存的修改只用 `Vault.dirty` 记着，标题栏显示小圆点和"保存"按钮，`Ctrl+S` 保存
- 保存（`crates/mikoshi-core/src/vault/save.rs`）：
  1. 先比文件内容的 SHA-256 和打开时（或上次保存时）记下的是否相同；不同就报 `saveConflict`，除非 `force`
  2. `kdbx` 加密后自己读回来核对，树和附件必须一致（用已经算出的 KDF 结果，不重跑 KDF）
  3. 写到旁边的 `名字.mikoshi-tmp`，落盘
  4. `ReplaceFileW` 换上去，旧文件变成 `名字.bak`（关闭备份时改成临时名字再删掉）。失败时旧文件不动，临时文件删掉
- 文件是 3.1 或 4.0 时，第一次保存前要 `upgrade: true`，前端弹框说明会变成 4.1
- 锁定、退出、切换库时有未保存的修改：设置是 `save` 就直接保存；是 `ask` 时手动锁定/退出先弹框（保存 / 不保存 / 取消），自动锁定没人回答，照样保存。任何一条自动保存路径碰到 `saveConflict` 都不覆盖原文件，而是把这一版写成 `名字.mikoshi-conflict-<时间>.kdbx`（同一个密钥），并发 `save-conflict-copy` 让主窗口提示
- 重新加载（`reload_database`）丢掉内存里的修改，用同一个密钥重读文件

### 回收站、分组和附件（M8）

- `crates/kdbx/src/ops.rs`：没有回收站时第一次删除自动建（图标 43，关搜索和自动输入，写进 `Meta/RecycleBinUUID`）；分组改名、移动（记 `PreviousParentGroup` 和 `LocationChanged`）；永久删除条目和分组，每个都记进 `Root/DeletedObjects`；删附件。附件池里没人引用的项会被移除，后面的引用编号跟着改
- `crates/mikoshi-core/src/vault/organize.rs`：回收站外删除是移进回收站，回收站里删除或库关了回收站是永久删除；移回原处看 `PreviousParentGroup`；加删附件和应用一次编辑一样先存历史
- 界面：`Delete` 删除（焦点在分组树时删分组），右键菜单（条目：编辑、移动到、删除；分组：在这里新建条目、新建子分组、重命名、移动到、删除；回收站：清空），条目和分组能拖到分组上，拖到回收站上就是删除。主窗口关掉了 Tauri 自己的文件拖放（`dragDropEnabled: false`），否则 WebView2 里网页的拖放事件收不到

### 新建库和密码生成器（M8）

- 新建库（`crates/kdbx/src/create.rs`、`crates/mikoshi-core/src/vault/create.rs`）：KDBX 4.1，AES-256，gzip，Argon2id 64 MiB、并行度等于 CPU 核数，迭代次数按本机校准到约 1 秒（量 2 次和 10 次两点，拟合固定开销和每次的开销，只量一次会把固定开销算进每次里、偏少）。只写到还不存在的文件
- 密钥文件：32 字节随机数，KeePass 2.47+ / KeePassXC 的 XML 2.0 格式，带 SHA-256 前 4 字节的校验
- 生成器（`crates/mikoshi-core/src/generator.rs`）：系统随机源，拒绝采样避免取模偏差，每种选中的字符至少一个再打乱；口令短语用 EFF 大词表（7776 个词，CC BY 3.0，`eff_large_wordlist.txt` 原样放在源码里）。强度用 Rust 的 `zxcvbn` crate，词典不进前端
- 编辑器密码栏右边的钥匙按钮打开生成器，`Ctrl+G` 单独打开、只复制

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

### 托盘、热键与快速搜索框

- **托盘**（`tray.rs`）：左键单击显示主窗口；菜单是显示主窗口、锁定/解锁…（锁定时打开主窗口去解锁）、快速搜索、退出。菜单文字按设置的语言，"跟随系统"时看 Windows 界面语言是不是中文。关闭主窗口只是隐藏，退出只能从托盘菜单
- **全局热键**（`hotkeys.rs`，tauri-plugin-global-shortcut）：默认 `Ctrl+Alt+K` 快速搜索、`Ctrl+Alt+L` 锁定。设置里存成 `Ctrl+Alt+K` 这样的文字：修饰键按 Ctrl、Alt、Shift、Win 的顺序，最后一个键用 `KeyboardEvent.code` 的名字（字母和数字去掉 `Key`、`Digit` 前缀）。必须带 Ctrl、Alt、Win 之一。快速搜索热键再按一次关闭搜索框
- **注册失败**：启动时注册不上的热键记为 `taken`，主窗口显示提示，设置里对应的热键下面也有提示；随开机启动时如果有热键注册不上，主窗口照样显示出来，否则只剩托盘一个入口
- **焦点**：快速搜索框显示前记下当时的前台窗口（Mikoshi 自己的窗口不算），`Enter` 复制或 `Esc` 关闭时在隐藏前把焦点还给它；点别处关闭时不还。M5 的自动输入也打到这个窗口
- **开机启动**（tauri-plugin-autostart）：写当前用户的 `Run` 注册表项，带 `--hidden` 参数，启动后只在托盘。状态以注册表为准（任务管理器里也能关），不存设置文件。注意开发构建打开它，写进去的是 `target\debug\mikoshi.exe`
- 自动启动插件自带的前端命令没有授权给任何窗口，只能通过 `get_autostart` / `set_autostart`

### 自动输入

分三层：匹配和序列解析在核心库（`crates/mikoshi-core/src/autotype.rs`、`vault.rs`），只认字段名、不碰字段值；按键在 `src-tauri/src/typing.rs`；入口在 `src-tauri/src/autotype.rs` 和 `commands.rs`。

- **匹配**（`Vault::autotype_matches`）：按分组树的顺序看每个条目，命中第一条就算。依次是：窗口关联（`*` 通配符，不区分大小写；`//…//` 是正则，限制编译大小）；前台是浏览器时，页面主机名等于条目网址的主机名或是它的子域名（条目网址开头的 `www.` 忽略；不带点的主机名和 IP 必须完全相同；只认 http、https 和不写协议的网址）；没有窗口关联的条目，窗口标题包含条目标题。跳过回收站、关闭了自动输入的条目和分组。分组的"启用自动输入"和默认序列向下继承
- **序列**（`autotype::parse`）：解析成步骤列表，字段引用只记名字。§3.10 的占位符和转义、`{TAB 2}` 这样的重复、`+` `^` `%` 修饰键和 `(…)` 分组。有认不出的内容就整段报错，一个键都不输入
- **浏览器网址**（`browser.rs`）：前台进程是 Edge、Chrome、Brave、Vivaldi、Firefox 时，用 UI 自动化找地址栏（Chromium 的类名 `OmniboxViewViews`，Firefox 的 AutomationId `urlbar-input`）读它的值。在单独的线程里初始化 COM，最多等 1.5 秒，超时就当不是浏览器。网址只用来比对，不写日志。实测 Chrome 读一次约 30 毫秒（`cargo test -p mikoshi browser -- --ignored --nocapture`，要先在 Chrome 里打开 example.com）
- **准备**（`typing::plan`）：先把序列里要用的字段值全部取出来（放在 `Zeroizing` 里），检查目标窗口还在、权限不比 Mikoshi 高（比较两个进程令牌的提升状态；Windows 会悄悄丢掉发给管理员窗口的按键，所以提前报 `autotypeElevated`）。能出的错都在这一步出，快速搜索框还开着的时候就能显示
- **输入**（`typing::run`，单独的线程）：
  1. Alt 或 Win 还按着时先补一个没分配的按键，松开时就不会打开菜单栏或开始菜单（M5 的 A3，KeePass 和 AutoHotkey 也这么做）；再等触发热键的修饰键全部松开，5 秒还没松开就放弃（`keysHeld`）
  2. 把目标窗口调到前台
  3. 目标窗口的键盘布局是中文、日文、韩文时，换成英文（美国）键盘，输完换回。微软拼音这类 TSF 输入法不理会旧的"关闭输入法"消息（M5 的 A1），所以要换布局；没装英文键盘就不换。另外照旧给还在用老输入法接口的程序发 `WM_IME_CONTROL` 关闭输入法，输完按原样打开
  4. 普通字符用 `KEYEVENTF_UNICODE` 逐个码元发送（和键盘布局无关，中文、emoji 都行）；Tab、Enter 等用虚拟键码，方向键那一块带"扩展键"标记
  5. 每个键之前确认前台窗口没变，变了就停（`focusLost`，用户自己切走的，不提示）；键之间按设置的间隔等待（默认 20 毫秒）
- **入口和目标窗口**：
  - 全局热键（默认 `Ctrl+Alt+V`）：目标是当时的前台窗口（Mikoshi 自己的不算）。只匹配到一个就直接输入；多个或库已锁定，打开快速搜索框的选择模式（`pick`，锁定时先解锁）列出匹配的条目；一个都没有，打开快速搜索框
  - 快速搜索框：目标是搜索框显示前记下的窗口。`Enter` 用条目的序列，`Ctrl+Enter` 只输入密码。先隐藏搜索框让 Windows 按常规激活下面的窗口，没激活再补一次（M5 的 A2）
  - 主窗口 `Ctrl+Shift+V` 和"自动输入"按钮：主窗口最小化，300 毫秒后的前台窗口就是目标
- **失败提示**：搜索框关掉之后才出的错，发 `autotype-failed` 事件，主窗口显示出来并提示。错误只有错误码，不带窗口标题、网址和字段内容

### 打开网址

`open_url` 在 Rust 端取条目的网址：没有协议的补上 `https://`，解析后必须是 http 或 https 且有主机名，否则报 `unsafeUrl`。KeePass 允许的 `cmd://` 之类会运行程序，一律拒绝。用 tauri-plugin-opener 交给默认浏览器；日志里不写网址。

## KDBX 读写层（`crates/kdbx`）

M6 起替代 keepass-rs。原则是**读进来什么就写回什么，只改动过的节点**。

- **分层**：外层容器（`header.rs` 头部和 KDF 参数，`crypto.rs` 接 RustCrypto 的算法，`db.rs` 打开和保存）→ XML 树（`xml.rs`）→ 视图和编辑（`view.rs`）。算法全部来自 RustCrypto（aes、twofish、cbc、chacha20、salsa20、argon2、sha2、hmac）和 flate2，这里只负责把密钥、IV、参数接起来
- **XML 树**保留元素顺序、属性、空白、注释和不认识的元素。标了 `Protected="True"` 的值在读的时候按文档顺序用内层流解密，存成会擦除的 `Secret` 节点，写的时候用新的内层流重新加密
- **KDBX 3.1 在打开时就转成 4.x 的形式**：附件从 `Meta/Binaries` 移进附件池并改写引用编号，时间改成 Base64 秒数（KeePass 2 读 KDBX 4 只认这种写法），先校验再去掉 `Meta/HeaderHash`，内层流改用 ChaCha20。这样保存就是把树原样写出去，"重新打开后的树和保存前逐节点相同"对所有版本都成立
- **保存**一律写 KDBX 4.1：主种子、IV、KDF 种子、内层流密钥每次重新生成；加密算法、KDF 参数（包括 Argon2 的 secret 等其他项）、压缩、头部的公共自定义数据照旧。数据按 1 MiB 分块。保存要重跑一次 KDF，所以需要密钥
- **编辑**（`entry_mut`、`add_group`、`add_entry`、`add_attachment`、`move_entry`）只碰对应的节点；新节点按兄弟节点的缩进插入。M7、M8 在这上面加功能
- **坏文件只返回错误，不 panic**：所有读取都先查长度；XML 嵌套超过 256 层拒绝；Argon2 的内存先试着申请，申请不到报错而不是让进程崩溃。KDF 轮数被改得极大时只能等，KeePass 也一样
- **不支持的**：KeePass 1、ArcFour 内层流、带命名空间前缀的元素名（KeePass 不用）。外层头部里不认识的字段和 3.1 的注释字段，保存时不写回
- `kdbx` 在 dev/test 构建里也开优化（根目录 `Cargo.toml`）：KDF 循环在这个 crate 里展开，不优化会慢几十倍
- 测试：`cargo test -p kdbx` 跑全部测试库的往返、K 系列测试库和 2000 例模糊测试（环境变量 `KDBX_FUZZ_CASES` 可加大）；`cargo run -p kdbx-compat --release` 以 `keepassxc-cli` 为准做完整检查，`-- fuzz 600` 跑 10 分钟模糊测试，`-- timing` 比较解锁时间

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
| panic 不终止进程 | release 保持 `panic = "unwind"`。`kdbx` 对坏文件只返回错误（模糊测试把关），打开时的 `catch_unwind` 是最后一道防线 | 根目录 `Cargo.toml` |
| 单实例 | 再次启动时把已有窗口切到前台，新进程退出 | `src-tauri/src/lib.rs` |
| 窗口不会"隐身" | 主窗口启动时先隐藏，界面画好再显示。如果界面加载失败，5 秒后照样显示，不会留下一个看不见的进程。随开机启动（`--hidden`）时例外：程序在托盘里，随时能打开 | `src-tauri/src/windows.rs` |
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
  - `backupOnSave`：保存前把上一版留成 `名字.bak`，默认开
  - `generator`：密码生成器上次的选项（模式、长度、字符类、词数、分隔符等），不存生成的密码
  - `lockUnsaved`：有未保存的修改时锁定怎么办，`save`（默认，直接保存）或 `ask`（手动锁定先问；空闲、锁屏、睡眠这类自动锁定照样保存）
  - `quickSearchHotkey`、`lockHotkey`：全局热键，默认 `Ctrl+Alt+K`、`Ctrl+Alt+L`，空字符串表示关闭
- `recent` 由 Rust 端维护：解锁成功时更新，`forget_recent` 删除。`set_settings` 收到的 `recent` 会被忽略，免得某个窗口拿着旧列表把新列表覆盖掉
- **只放非敏感信息。**
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

### 不打扰桌面的测试

测试时 Mikoshi 的窗口不能出现在正在用电脑的人面前，不能抢焦点，也不能让 Windows 把屏幕切到别的桌面。办法是把测试窗口放到一个专门的 Windows 虚拟桌面上，平时没人看它：

1. 一次性准备：在任务视图（`Win+Tab`）里新建一个桌面，改名为"Mikoshi 测试"，一直留着。开发用的终端放在哪个桌面都行
2. 后台启动 Vite：`pnpm exec vite`
3. `pwsh tools/run-off-desktop.ps1` 启动 debug 构建。脚本从注册表（`HKCU\...\Explorer\VirtualDesktops`）按名字找到这个桌面；找不到、或者它正是眼前这个桌面时，不启动。`-DryRun` 只打印会用哪个桌面
4. 用 `tools/cdp.mjs` 操作和截图；`tools/desktop-of.ps1` 能查看每个窗口在哪个桌面

新建桌面不由脚本来做：公开的接口不能新建桌面，`Win+Ctrl+D` 会把用户的屏幕一起切过去，能悄悄新建的内部接口没有文档，每个 Windows 版本都可能变。

开发构建认环境变量 `MIKOSHI_DEV_DESKTOP`（`windows::dev_quiet()`），这时：

- **窗口由 Mikoshi 自己搬到那个桌面**：建好时搬一次，每次显示前再搬一次。Windows 只允许程序移动自己的窗口，从外面搬会被拒绝
- **快速搜索框临时有任务栏按钮**：没有任务栏按钮的窗口不属于任何虚拟桌面，会出现在用户正在看的桌面上
- **从不激活窗口**：不调 `set_focus`。tao 的 `set_focus` 会模拟按一下 Alt 键来绕过前台锁，这一下 Alt 会按进用户正在用的程序，而且能把屏幕切走。建窗口时也不拿焦点（`focused(false)`）
- **不注册全局热键，不自动输入**：否则用户按 `Ctrl+Alt+V` 时，测试实例会往用户的窗口里打字

`run-off-desktop.ps1` 另外做两件事：

- **经计划任务和隐藏的脚本宿主启动**，不是终端的子进程。终端正是用户正在打字的前台进程，它启动的程序有权抢前台，连普通的显示窗口（`ShowWindow(SW_SHOW)`）都会把屏幕切过去。换了父进程就没有这个权利
- **给 WebView2 加参数**：`--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows`，窗口所在的桌面没人看时照样渲染，截图才是最新的；同时开调试端口给 `cdp.mjs`

这些代码都在 `#[cfg(debug_assertions)]` 里，release 构建没有。

2026-11-20 逐项实测了 8 轮：只要少了"不调 `set_focus`""计划任务启动""快速搜索框进任务栏"中的任何一项，都会把窗口弄到用户面前或者切走屏幕；`focused(false)` 在有计划任务时不是必需的，留作保险。

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
