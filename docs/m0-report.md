# M0 报告：keepass-rs 能不能用

日期：2026-11-17　对象：keepass-rs 0.17.0（`save_kdbx4` + `totp`）　参照：keepassxc-cli 2.8.0

## 结论

**能用，建议继续用上游 keepass-rs，不需要 fork，也不需要换方案。**

- **读取**：KDBX 3.1、4.0、4.1 都能打开，5 种密钥文件格式和"密码 + 密钥文件"组合也都没问题。读出的内容和 KeePassXC 看到的逐项比对，只有 K2、K3、K4 三处不一致。
- **保存**：常见数据都完整保留。保存后用 keepass-rs 重新读，结果和原来完全相同，附件逐字节一致，历史记录也在。发现的损失都在冷门数据上（K1、K2、K6、K7）。
- **兼容性**：3 种加密算法 × 3 种 KDF，共 9 种组合，keepass-rs 写出的文件 KeePassXC 都能打开，看到的数据也一致。用 keepass-rs 改过的库（编辑条目、加历史、新建分组和条目、加附件、移进回收站），KeePassXC 也全部读对了。
- **发现 9 个问题（K1–K9）**：没有一个影响常见数据。多数能在 Mikoshi 里绕开，另外几个值得给上游提 PR，见下文。
- **规格要改一处**：keepass-rs 只能写 KDBX 4.1，所以 4.0 的库保存后也会升级，不止 3.1。见"需要改规格的地方"。

## 测了什么

### 测试库

| 来源 | 数量 | 覆盖 |
|---|---|---|
| keepassxc-cli 导入生成（`tests/fixtures/kxc/`） | 10 | KDBX 3.1 / 4.0 / 4.1；只用密码、只用密钥文件（XML v1、XML v2、32 字节二进制、64 位十六进制、任意文件）、密码 + 密钥文件 |
| keepass-rs 自己写（每次运行生成） | 9 | AES-256 / Twofish / ChaCha20 × AES-KDF / Argon2d / Argon2id |
| 自写的最小 KDBX 4 写入器（每次运行生成） | 3 | KeePassXC 不会保留的内容：未知 XML 元素、KeePass 2 专有字段、格式错误的时间戳 |
| 图形界面手工制作（`tests/fixtures/gui/`） | 0 / 8 | KeePassXC 和 KeePass 2 自己写出的 Argon2、ChaCha20、Twofish 库，**还没做**，见"还没验证的" |

每个库的内容一样，共 25 个条目、14 个分组：标准字段（含中文、emoji、XML 特殊字符）、自定义字段（含受保护、空值、中文字段名、多行、首尾空格、纯空格）、标签、过期时间、颜色、覆盖 URL、自定义图标、附件（空文件、4 KB 随机、64 KB 随机、中文文件名、压缩存储、两个条目共享同一附件、只在历史版本里存在的附件）、3 个历史版本、TOTP（5 种 otpauth 写法和 KeePass 2 原生字段）、自动输入（自定义序列、3 条窗口关联、关闭自动输入）、三级嵌套分组、分组设置（禁止搜索、禁止自动输入、折叠）、回收站（含已删除的条目和分组）、模板分组、已删除对象记录、库级 / 分组级 / 条目级 CustomData，以及 KDBX 4.1 的新字段（分组标签、PreviousParentGroup、QualityCheck、图标名称）。

### 怎么比对

只用 keepass-rs 自己的数据模型比对是不够的：它不认识的元素，在保存前和保存后两次读取中都看不到。所以每个库都从三个角度比：

1. **keepass-rs 读到的 vs KeePassXC 读到的**：拿 keepass-rs 的数据模型和 `keepassxc-cli export` 导出的 XML 逐项比对，受保护字段比明文；附件用 `keepassxc-cli attachment-export` 导出后逐字节比对；TOTP 比较两边同一时刻生成的验证码。
2. **保存前 vs 保存后的原始 XML**：直接解密文件里存的 XML，归一化后比对。这一步能发现库不认识、因而被丢掉的元素。
3. **KeePassXC 读原文件 vs 读 keepass-rs 保存后的文件**：确认别的程序看到的数据没变。

归一化只去掉不承载数据的差异：时间戳编码（3.1 用 ISO 字符串，4.x 用 Base64）、元素顺序、空元素和缺失、布尔值大小写、标签分隔符、附件池编号。字段值按原样比较，空白也算在内。

另外还测了三项：
- 错误识别：错误密码、错误密钥文件、截断、头部或正文翻转一个比特、空文件、随机内容、KeePass 1 格式
- 编辑后保存：再让 KeePassXC 读回来，确认改动都在
- 解锁耗时

## 结果

### 读取

10 个测试库都能打开，内容和 KeePassXC 一致。例外只有：
- 纯空白的字段值读成空字符串（K2）
- 两种 TOTP 写法算不出码（K3、K4）

7 个附件和 KeePassXC 导出的逐字节一致。只存在于历史版本的那个附件，keepassxc-cli 导不出来，只用 keepass-rs 自己读到的数据核对过。

### 保存（打开 → 保存 → 重新打开）

- 用 keepass-rs 重新读，数据模型和原来完全相同，所有附件（包括历史版本里的）都一致
- 存储的 XML 每个库比了约 800 项，损失只有 K1、K2，和"未知元素"测试库里的 K6、K7
- KeePassXC 读保存后的文件，看到的和读原文件一样（K2 除外）
- 一类无害差异：keepass-rs 会把 `QualityCheck=True` 写出来，而这个字段不写时默认就是 True

### 编辑后保存

用 keepass-rs 改标题和密码、把旧版本存进历史、加受保护的自定义字段、新建分组和条目、加附件、把条目移进回收站，然后用 KeePassXC 读回来，**8 项检查全部通过**。移动条目时，keepass-rs 会自动写上 `PreviousParentGroup`。

### 错误识别

| 情况 | KDBX 4.x | KDBX 3.1 |
|---|---|---|
| 密码或密钥文件错 | 能识别 | 报成"填充错误"，和文件损坏区分不开（K5） |
| 截断 | 能识别为损坏 | 截在头部能识别；截在正文报成填充错误（K5） |
| 头部翻转一个比特 | 能识别（头部哈希不符） | 报成密码错误（K5，3.1 格式本身不校验头部） |
| 正文翻转一个比特 | 能识别 | 能识别（正文开头）；末尾的结束标记不校验，内容不受影响 |
| 空文件、随机内容 | 能识别为"不是 KeePass 文件" | 同左 |

### 性能

解锁耗时（release 构建，同一台机器）：

| KDF 参数 | keepass-rs | keepassxc-cli |
|---|---|---|
| AES-KDF，1000 万轮 | 252 ms | 177 ms |
| Argon2d，64 MiB，2 次迭代（KeePass 2 默认） | 96 ms | 49 ms |
| Argon2id，64 MiB，10 次迭代 | 216 ms | 174 ms |
| Argon2id，1 GiB，2 次迭代 | 1083 ms | 1762 ms |

和 KeePassXC 在同一量级，不用担心。debug 构建如果不单独优化依赖，会慢 7 到 25 倍（上面四项依次是 6.2 s、0.8 s、2.7 s、7.8 s）。工作区的 `Cargo.toml` 已经加上 `[profile.dev.package."*"] opt-level = 3`，开优化后 debug 构建和 release 一样快。

keepass-rs 文档推荐的 `target-cpu=native` **不需要，也不该加**：x86_64 上 `aes` crate 会在运行时检测 AES-NI（1000 万轮只要 0.25 秒，说明已经在用硬件加速）；加了这个参数，编出来的程序在老 CPU 上会直接崩溃。

## 问题清单

| ID | 问题 | 影响 | 建议 |
|---|---|---|---|
| K1 | 条目或分组设了自定义图标后，原来的标准图标编号（IconID）在保存时丢失 | 很小。只影响不支持自定义图标的客户端，它们会显示 0 号图标 | 提 PR；不阻塞 |
| K2 | 只含空白的字段值（如 `"   "`）读成空字符串，保存后就真的变空了 | 小。只影响未受保护的字段，密码等受保护字段不受影响。首尾带空格的普通值没问题 | 提 PR（读取时就丢了，Mikoshi 绕不开） |
| K3 | otpauth URI 没写 `digits` 时，keepass-rs 默认生成 8 位；标准和 KeePassXC 都是 6 位 | 会算出错误的验证码 | Mikoshi 自己解析 TOTP（反正 KeePass 2 原生 TOTP 也得自己实现），同时提 PR |
| K4 | 小写的 Base32 密钥解析失败 | 这类条目算不出验证码 | 同 K3 |
| K5 | KDBX 3.1 区分不了密码错误和文件损坏 | 错误提示没法说准 | 3.1 的库提示"密码或密钥文件不对，也可能是文件已损坏"。可以给上游提 PR，在去掉填充之前先检查开头的校验字节 |
| K6 | keepass-rs 不认识的 XML 元素在保存时丢失 | KeePassXC 同样会丢；KeePass 2 和 KeePassXC 都不写这类元素，插件数据走的是 CustomData（这部分保留完好） | 接受 |
| K7 | KeePass 2 的"下次强制修改主密码"（`MasterKeyChangeForceOnce`）在保存时丢失 | 很小。只有 KeePass 2 用户、且开了这个选项时才有影响 | 提 PR（在 Meta 里加一个字段）；不阻塞 |
| K8 | 分组下直接出现不认识的元素时，**整个库打不开** | 现在的 KeePass 2、KeePassXC 都不会写出这种文件。风险在于以后格式扩展时给分组加了新字段 | 提 PR（给分组子元素加一个兜底分支，改动很小）；上游不收就在 fork 里打补丁 |
| K9 | 时间戳格式错误（例如 Base64 不足 8 字节）时 keepass-rs 直接 panic，不返回错误 | 正常程序写出的文件不会这样，但一个坏文件就能让程序崩溃 | Mikoshi 用 `catch_unwind` 包住所有 keepass-rs 调用，release 构建不能设 `panic = "abort"`；同时提 PR |

所有问题都写进了 `kdbx-compat` 的已知问题列表。命中已知问题的结果标为"ok, known issues"，**出现其他差异时命令返回非零**，所以以后升级 keepass-rs 或改代码时，可以直接把它当回归测试用。

## 对后续开发的要求

下面这些在对应里程碑里落实：

- **M1**：release 构建保持 `panic = "unwind"`（K9）；不加 `target-cpu=native`
- **M2**：
  - 打开文件时 `catch_unwind`（K9）
  - 3.1 的库按 K5 合并提示
  - 显式拒绝 KeePass 1 的 `.kdb` 文件：keepass-rs 能读 KDB，但规格不支持
- **M3**：TOTP 自己实现，不用 keepass-rs 的 `get_otp`（K3、K4）
- **M6**：
  - 保存前把版本设为 KDBX 4.1；从 3.1 升级的库，受保护字段的内层加密从 Salsa20 换成 ChaCha20（KeePass 自己升级时也这么做）
  - 历史记录（含条数和大小上限）、回收站都由 Mikoshi 自己实现：keepass-rs 提供 `History::add_entry` 和 `move_to`，但不会自动记录历史，也不会自动进回收站
- **贯穿**：keepass-rs 里受保护的值是用 `SecretBox` 存的，释放时会擦除；但未受保护的字段（用户名、URL、备注）和解密过程中的中间缓冲区不会擦除。这在库内部，规格 §6 写的是"尽量"，可以接受

## 需要改规格的地方

features.md §3.8 原文：

> 从 KDBX 3.1 打开的库保存后会变成 KDBX 4。第一次保存前提示用户，并说明 KeePass 2.35 以上、KeePassXC 2.3 以上能打开

建议改为：

> keepass-rs 只能写 KDBX 4.1，所以 KDBX 3.1 和 4.0 的库第一次保存后都会变成 4.1。保存前提示用户，并说明 KeePass 2.48 以上能完整支持；KeePassXC 2.8 已实测能打开

KeePass 2.48 引入 KDBX 4.1，这是 keepass.info 的说明。更老的 KeePass 和 KeePassXC 能不能打开 4.1 没有实测。按格式约定，只升次版本号时旧版应该还能打开，但会丢掉 4.1 的新字段。这条要等你确认后再改 features.md。

## 还没验证的

1. **图形界面做的库**（`tests/fixtures/gui/`，做法见 [tests/fixtures/README.md](../tests/fixtures/README.md)）：
   - 目前 Argon2、ChaCha20、Twofish 只验证了"keepass-rs 写、KeePassXC 读"这个方向。反过来，KeePassXC 自己写出的这类文件，keepass-rs 能不能读，还没测
   - 因为 keepass-rs 自己写的文件 KeePassXC 能正确读出，说明两边算法实现一致，所以风险低
   - 但 KeePass 2 写出的库完全没测过
2. **KeePass 2 能不能打开 keepass-rs 写的文件**：本机没装 KeePass 2。
   - keepass-rs 写的文件放在 `target/kdbx-compat/saved/`、`variants/`、`foreign/` 下，用 KeePass 2 打开抽查即可，密码都是 `mikoshi-test`
   - 有一个已知区别：keepass-rs 把整段数据写成一个块，KeePass 是每 1 MiB 一块。格式允许这样写，KeePassXC 也能读
3. **KeePassDX**：留到 M8 的人工验收。
4. **你自己的库**：强烈建议跑一次（见下）。它会告诉你真实数据里有没有命中 K1–K9，特别是 K8（命中的话会直接打不开）。全程只在内存里操作，不写任何文件，输出也只有路径，没有任何值。

## 怎么跑

```bash
cargo run -p kdbx-compat --release
```

```bash
cargo run -p kdbx-compat --release -- check "你的库.kdbx"
```

```bash
cargo run -p kdbx-compat --release -- timing
```

```bash
python tests/fixtures/generate.py
```

依次是：跑全部测试库，报告写到 `target/kdbx-compat/report.md`；检查任意一个库，提示输入主密码，不写文件；测解锁耗时；重新生成测试库（需要 keepassxc-cli）。

## 新增的文件

| 路径 | 内容 |
|---|---|
| `Cargo.toml` | Rust 工作区；keepass 0.17 依赖；dev 构建下优化依赖 |
| `crates/mikoshi-core/` | 核心库（目前为空），M1 开始往里写 |
| `crates/kdbx-compat/` | 本次的验证工具 |
| `tests/fixtures/` | 测试库、密钥文件、生成脚本、清单、说明 |
| `tools/kxc.py` | 非交互调用 keepassxc-cli 的小工具（它在标准输入没重定向时会卡住） |
