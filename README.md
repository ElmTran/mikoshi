# Mikoshi

Windows 上的密码管理器。直接读写 KeePass 的 `.kdbx` 密码库，和 KeePassXC、KeePass 2、手机上的 KeePassDX 用同一个文件；界面简洁，以快捷键为中心。

- 打开、浏览、搜索 KeePass 密码库，复制用户名、密码、验证码（TOTP）
- 在任何程序里按 `Ctrl+Alt+K` 呼出快速搜索框，按 `Ctrl+Alt+V` 自动输入账号密码
- 新建和编辑条目、历史版本、回收站、分组、附件、密码生成器、新建密码库
- 保存时只改动过的地方，其余内容原样写回：别的客户端写进去的数据不会丢
- 不联网，没有账号，没有云同步；文件放在哪里、怎么同步由你决定

## 安装

1. 下载 `Mikoshi_x.y.z_x64-setup.exe`，运行它
2. 默认装在 `%LOCALAPPDATA%\Programs\Mikoshi`，只给当前用户安装，不需要管理员权限；安装向导里可以换目录
3. 安装包没有代码签名，第一次运行时 Windows SmartScreen 会提示"无法识别的应用"：点"更多信息"→"仍要运行"，以后不会再提示
4. Mikoshi 的界面用的是 Windows 自带的 WebView2。Windows 11 和更新过的 Windows 10 都已经有了；没有的话，安装包会从微软下载安装（只在安装时联网，Mikoshi 本身从不联网）

卸载：Windows 设置 → 应用 → Mikoshi → 卸载。设置文件在 `%APPDATA%\com.elmtran.mikoshi\`，卸载后要彻底清掉可以手动删除；它不含任何密码。

## 开始使用

- **打开已有的库**：选择 `.kdbx` 文件，输入主密码；用了密钥文件的，点"使用密钥文件"选上它。Mikoshi 会记住最近打开的库和每个库的密钥文件位置（只记路径）
- **新建密码库**：解锁界面上的"新建密码库…"。设主密码，可以再加一个密钥文件（选现有的，或新生成一个）。新库是 KDBX 4.1、AES-256、Argon2id，加密强度按这台电脑的速度调整到解锁大约 1 秒
- 支持 KDBX 3.1、4.0、4.1，密码、密钥文件或两者都用；不支持 KeePass 1 的 `.kdb`

## 日常使用

- **搜索**：`Ctrl+F` 或直接在顶部搜索框输入。多个词都要匹配；`title:`、`user:`、`url:`、`tag:` 把一个词限定在某个字段；引号让词里可以有空格（`title:"招商 银行"`）。拼不全也能找到（`gthb` 能找到 GitHub）
- **复制**：选中条目后，`Ctrl+B` 复制用户名、`Ctrl+C` 复制密码、`Ctrl+T` 复制验证码、`Ctrl+U` 复制网址。密码和验证码不进 Windows 的剪贴板历史（`Win+V`）和云剪贴板，默认 10 秒后自动清掉；如果在这之前你又复制了别的东西，不会被误清
- **快速搜索框**：在任何程序里按 `Ctrl+Alt+K`。输入几个字，`Enter` 复制密码，`Ctrl+Enter` 自动输入，`Ctrl+B` 复制用户名，`Ctrl+T` 复制验证码，`Esc` 关闭
- **自动输入**：在登录页按 `Ctrl+Alt+V`。Mikoshi 按窗口标题和浏览器地址栏里的网址找到对应的条目，输入"用户名 Tab 密码 Enter"（可以按 KeePass 的写法为每个条目定制序列）。找到多个时让你选，一个都没有时打开快速搜索框。目标窗口以管理员身份运行时，Windows 不允许输入，Mikoshi 会提示
- **自动锁定**：一段时间没有操作（默认 5 分钟）、锁屏（`Win+L`）、电脑睡眠时自动锁定；`Ctrl+L` 或 `Ctrl+Alt+L` 手动锁定。锁定后内存里解密过的内容全部清掉
- **托盘**：关掉主窗口只是隐藏，Mikoshi 留在托盘里，全局热键照常可用；退出在托盘菜单里

## 编辑和保存

- `Ctrl+N` 新建条目，`Ctrl+E` 或 `Enter` 编辑。标准字段、标签、过期时间、自定义字段（每个都能单独设为受保护）、验证码（粘贴 `otpauth://` 地址，或手填密钥）、图标
- 每次修改前，旧版本自动存进历史，条数上限沿用库里的设置；在历史记录里可以查看和恢复任意一个版本
- `Delete` 把条目或分组移到回收站，在回收站里再删就是永久删除（会先确认）；可以清空回收站
- 分组可以新建、改名、移动、删除；条目和分组都能拖到别的分组里
- 附件可以添加、导出、删除
- 密码生成器：`Ctrl+G`，或编辑时密码栏旁边的钥匙按钮。可以选长度和字符种类、排除容易看错的字符，也可以生成几个英文单词连起来的口令短语；显示强度估算
- **保存**：默认每次修改都立即写入文件。先写临时文件、核对无误后再替换原文件，上一版留作 `名字.kdbx.bak`（可以在设置里关掉）。也可以在设置里关掉"修改后立即保存"，改成按 `Ctrl+S` 时才保存，标题栏会显示有没有未保存的修改
- **文件被别处改过**（比如同步盘刚同步过来、或 KeePassXC 同时开着）：保存前会提示，让你选重新加载（放弃自己的修改）、覆盖，或取消。锁定时如果遇到这种情况，你的修改另存成 `名字.mikoshi-conflict-时间.kdbx`，两边都不会丢

## 快捷键

全部可以在设置里修改。

**全局**（在任何程序里都有效）

| 操作 | 默认 |
|---|---|
| 快速搜索框 | `Ctrl+Alt+K` |
| 自动输入 | `Ctrl+Alt+V` |
| 锁定 | `Ctrl+Alt+L` |

**主窗口**

| 操作 | 默认 |
|---|---|
| 搜索 | `Ctrl+F` |
| 复制用户名 | `Ctrl+B` |
| 复制密码 | `Ctrl+C` |
| 复制网址 | `Ctrl+U` |
| 打开网址 | `Ctrl+Shift+U` |
| 复制验证码 | `Ctrl+T` |
| 自动输入选中的条目 | `Ctrl+Shift+V` |
| 显示/隐藏密码 | `Ctrl+H` |
| 新建条目 | `Ctrl+N` |
| 编辑条目 | `Ctrl+E` 或 `Enter` |
| 删除 | `Delete` |
| 保存 | `Ctrl+S` |
| 锁定 | `Ctrl+L` |
| 密码生成器 | `Ctrl+G` |
| 设置 | `Ctrl+,` |

方向键、`Home`、`End`、`PageUp`、`PageDown` 在分组和条目之间移动，`Tab` 在三栏之间切换。

## 从 KeePassXC 或 KeePass 2 迁移

- 不用导入导出：直接打开原来的 `.kdbx` 就行，几个程序可以轮流用同一个文件
- **Mikoshi 只写 KDBX 4.1**。KDBX 3.1 或 4.0 的库第一次保存时会提示，同意后文件变成 4.1：KeePass 2.48 以上、KeePassXC 2.8 都能打开，更老的版本打不开
- 加密算法、密钥派生参数、压缩方式沿用原来的设置；Mikoshi 不提供修改这些设置和修改主密码的功能，需要时用 KeePassXC
- 保存时只改动过的地方，其他客户端写进去、Mikoshi 不认识的数据原样保留
- 验证码：认 KeePassXC 的 `otp` 字段、KeePass 2 自带的 `TimeOtp-*` 字段和老的 `TOTP Seed` 写法；在 Mikoshi 里修改验证码会统一写成 `otp` 字段
- 永久删除会记进库的"已删除对象"列表，同步工具和其他客户端据此知道东西是被删掉的
- Mikoshi 没有的：浏览器扩展、SSH Agent、密码泄露检查、导入导出、KeeShare、数据库设置、Windows Hello 解锁、YubiKey、多个库同时打开，以及 macOS 和 Linux 版本

## 安全上的做法

- 密码、受保护字段和验证码密钥只在程序的 Rust 部分里，界面只拿到标题、用户名这类信息；复制、自动输入、计算验证码都在 Rust 里完成。只有你点"显示"或打开编辑时，才把那一个条目的值交给界面，用完就丢
- 内存里的秘密在释放时擦除；锁定就是把解密过的库整个丢掉
- 日志和错误信息里不会出现任何密码和条目内容
- 不联网，不加载任何远程资源；正式版关闭了开发者工具，页面有严格的内容安全策略
- 只运行一个实例，再次启动会切到已经开着的窗口

## 第三方软件

Mikoshi 用到的开源组件和它们的许可证见 [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt)（安装目录里也有一份）。口令短语的词表来自 [EFF](https://www.eff.org/dice)，使用 CC BY 3.0 US 许可。

## 开发

Tauri 2 + React + TypeScript；后端是 Rust，用自己写的无损读写层 `crates/kdbx` 读写库文件。

- 文档：功能规格 [docs/features.md](docs/features.md)、里程碑 [docs/milestones.md](docs/milestones.md)、工程结构和约定 [docs/architecture.md](docs/architecture.md)、M0 结论 [docs/m0-report.md](docs/m0-report.md)
- 需要 Rust 1.90 以上（MSVC 工具链，要装 Visual Studio 生成工具的"使用 C++ 的桌面开发"）、Node.js、pnpm

```bash
pnpm install
pnpm tauri dev
pnpm check
```

依次是：装依赖；启动程序；跑全部检查（类型、ESLint、clippy、格式、测试）。

- 不打扰正在用电脑的人的测试方法（窗口放到另一个虚拟桌面）：见 architecture.md 的"不打扰桌面的测试"
- KDBX 兼容性检查：`cargo run -p kdbx-compat --release`，说明见 [tests/fixtures/README.md](tests/fixtures/README.md)
- 依赖变了之后重新生成第三方许可说明：`python -I tools/third-party-notices.py`
