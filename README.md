# Mikoshi

一个 Windows 密码管理器，界面现代，以快捷键为中心，直接读写 KeePass 的 `.kdbx` 数据库。

- 技术栈：Tauri 2 + React + TypeScript，后端是 Rust，用 [keepass-rs](https://github.com/sseemayer/keepass-rs) 读写库文件
- 当前阶段：M1（工程骨架、界面风格）。M0 已完成，结论见 [docs/m0-report.md](docs/m0-report.md)，遗留问题见 [docs/milestones.md](docs/milestones.md) 的"待处理问题"。v1 的功能规格见 [docs/features.md](docs/features.md)
- 兼容性检查：`cargo run -p kdbx-compat --release`，说明见 [tests/fixtures/README.md](tests/fixtures/README.md)
