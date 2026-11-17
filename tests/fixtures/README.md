# KDBX 测试库

**这里只能放测试数据。** 所有密码和密钥文件都是公开的。不要把真实的密码库放进来。

所有测试库的密码都是 `mikoshi-test`。只用密钥文件的库例外，它们不设密码。

## 运行

```bash
cargo run -p kdbx-compat --release
```

报告写在 `target/kdbx-compat/report.md`。命令返回 0，说明没有出现已知问题以外的差异。

## 目录

| 路径 | 内容 | 怎么来的 |
|---|---|---|
| `src/content-31.xml` `content-40.xml` `content-41.xml` | 测试库的内容，KeePass 2 XML 格式 | `generate.py` 生成 |
| `keys/` | 各种格式的密钥文件 | `generate.py` 生成 |
| `kxc/` | keepassxc-cli 导入上面的 XML 得到的库 | `generate.py` 生成 |
| `gui/` | 用 KeePassXC、KeePass 2 图形界面手工做的库 | 手工，见下文 |
| `fixtures.toml` | 每个库的密码、密钥文件，以及预期的格式、加密算法、KDF | 手工维护 |

重新生成 `src/`、`keys/`、`kxc/`（需要 keepassxc-cli 在 PATH 里，或者设置 `KEEPASSXC_CLI` 环境变量）：

```bash
python tests/fixtures/generate.py
```

三个版本的 XML 内容基本相同，区别只在于有没有用到各版本特有的字段。KeePassXC 会自动选能装下内容的最低版本，所以导入后分别得到 KDBX 3.1、4.0 和 4.1。

## 需要手工制作的库

keepassxc-cli 只能生成 AES-256 + AES-KDF 的库。下面这些需要在图形界面里做，做好后放进 `gui/`，文件名要和这里一致。还没做的会在报告里显示为 skip。

### KeePassXC（2.7 以上）

先导入内容：菜单 **数据库 → 导入 → KeePass 1 / KeePass 2 XML**（不同版本菜单位置略有不同），选 `src/content-41.xml`，密码填 `mikoshi-test`。然后在 **数据库 → 数据库安全设置 → 加密设置** 里改设置，再 **另存为**：

| 文件名 | 加密算法 | KDF | 其他 |
|---|---|---|---|
| `keepassxc-argon2d-chacha20.kdbx` | ChaCha20 | Argon2d | |
| `keepassxc-argon2id-twofish.kdbx` | Twofish | Argon2id | |
| `keepassxc-aeskdf-kdbx4.kdbx` | AES-256 | AES-KDF (KDBX 4) | |
| `keepassxc-argon2id-pw-keyfile.kdbx` | AES-256 | Argon2id | 在 **数据库凭据** 里再加上密钥文件 `keys/xml-v2.keyx` |

解锁时间保持默认（1 秒左右）即可，这样也能顺便测出真实设置下的解锁速度。

如果你的 KeePassXC 导入后保存成了 4.1 而不是 4.0，把 `fixtures.toml` 里对应的 `format` 改成 `4.1` 就行。

### KeePass 2（2.48 以上）

**文件 → 导入 → KeePass XML (2.x)**，导入 `src/content-41.xml`，然后在 **文件 → 数据库设置 → 安全** 里改设置，**另存为**：

| 文件名 | 加密算法 | KDF | 其他 |
|---|---|---|---|
| `keepass2-argon2d-aes.kdbx` | AES-256 | Argon2d | |
| `keepass2-argon2id-chacha20.kdbx` | ChaCha20 | Argon2id | |
| `keepass2-kdbx31.kdbx` | AES-256 | AES-KDF | 保存时选 KDBX 3.1 格式（**文件 → 另存为** 对话框里可选；没有这个选项就跳过） |
| `keepass2-keyfile-only.kdbx` | AES-256 | Argon2d | 新建一个空库：不设密码，只用 KeePass 生成的密钥文件，密钥文件存为 `gui/keepass2-keyfile-only.keyx`；然后把 XML 导入进去 |

KeePass 2 生成的库最能检验"KeePass 2 写、Mikoshi 读写、KeePass 2 再打开"这条链路，也是唯一能覆盖 KeePass 2 专有字段的来源。没装 KeePass 2 的话，这一组可以先不做。

## 用你自己的库检查（不会写文件）

```bash
cargo run -p kdbx-compat --release -- check "D:\path\to\your.kdbx"
```

程序会提示输入主密码。它在内存里把库打开、保存、再打开，然后列出保存后会变化的内容。**输出只有路径（分组 / 条目 / 字段名），没有任何值，也不往磁盘写任何东西。** 用到密钥文件就加上 `--keyfile <路径>`，只用密钥文件、没有密码时再加 `--no-password`。
