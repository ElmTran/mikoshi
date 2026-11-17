# Mikoshi

一个 Windows 密码管理器，界面现代，以快捷键为中心，直接读写 KeePass 的 `.kdbx` 数据库。

- 技术栈：Tauri 2 + React + TypeScript，后端是 Rust，用 [keepass-rs](https://github.com/sseemayer/keepass-rs) 读写库文件
- 当前阶段：M2（打开、解锁、只读浏览）已完成，下一步是 M3（搜索、复制、TOTP、自动锁定）。现在能打开真实的 `.kdbx`，浏览分组、条目和详情；**不会写入库文件**。复制和验证码从 M3 接入
- 文档：功能规格 [docs/features.md](docs/features.md)、里程碑和待处理问题 [docs/milestones.md](docs/milestones.md)、工程结构和前后端约定 [docs/architecture.md](docs/architecture.md)、M0 结论 [docs/m0-report.md](docs/m0-report.md)

## 运行

需要 Rust 1.90 以上（MSVC 工具链，要装 Visual Studio 生成工具的"使用 C++ 的桌面开发"）、Node.js、pnpm，以及 Windows 自带的 WebView2。

```bash
pnpm install
```

```bash
pnpm tauri dev
```

```bash
pnpm check
```

依次是：装依赖；启动程序；跑全部检查（类型、ESLint、clippy、格式、测试）。

KDBX 兼容性检查另有一条命令，说明见 [tests/fixtures/README.md](tests/fixtures/README.md)：

```bash
cargo run -p kdbx-compat --release
```
