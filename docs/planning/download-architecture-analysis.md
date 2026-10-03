---
tags: [niubash, architecture, plugins, download, package-manager, layering]
created: 2026-10-03
status: proposed (wt49/dlarch 设计评估，待 owner 裁决)
---

# 下载职责的架构归属：shell 内部还是插件管理器层？

> Owner 之问（2026-10-03）："这部分下载我们是不是应该以插件的形式挂载？就是下载的部分是集成在
> shell 内部还是挂载到插件管理器的层次上？我的确没有见过 shell 承担如此多的下载职责的。"
>
> 本报告不改任何产品代码。全部结论来自：业界源码/文档调研 + 本仓库调用面盘点 +
> 二进制体积实验（方法与数据见 §3）。

## TL;DR

1. Owner 的直觉**对引擎成立，对产品层不成立**：业界没有任何 shell *引擎* 自带 HTTP 下载；
   但**以"发行版/平台"形态交付的产品**（rustup/cargo、Node/npm、VS Code）普遍内置或捆绑下载能力。
   我们的真实结构恰好是后者：rubash 引擎已零网络依赖，全部下载在 niubash 产品层。
2. 引导问题（"管理器自己怎么装"）**对我们不存在**：lazy.nvim 的 bootstrap 片段是"没有安装器"
   时的补偿机制；我们有 Inno Setup 安装器，管理器随产品分发，离线也可用。
3. 全部 6 个下载/网络调用面都已在**冷路径**（CLI 子命令 / 一次性向导），从不在 REPL 热路径；
   缺的不是进程分离，是**编译期边界**：网络栈（ureq/rustls）与 REPL/高亮/补全同居一个 crate，
   且三个传输栈并存（ureq、curl.exe、WinHTTP）。
4. **船长推荐（一条）：选项 B 硬化版** —— 把 `plugins/*` 提为独立 workspace crate
   （`niu-pkg`：download + recipes + sources + trust + registry IO），网络传输置于 cargo feature
   之后；保持单二进制分发；同时写明迁移到独立 exe（选项 C）的触发条件。
   体积代价 1.76 MB（约 19%），由 feature 门控给出可验证的"零网络 shell 构建"形态。

---

## 1. 业界对照表

| Shell / 编辑器 | 引擎自身有下载吗 | 下载在哪层 | 引用 |
| --- | --- | --- | --- |
| **zsh** | 无 HTTP。可选加载模块 `zsh/net/tcp`（`ztcp`）、`zsh/net/socket`、`zsh/zftp`（zshmodules）理论上可裸 TCP/FTP，但**没有任何生态用它装东西** | zinit / antigen / antidote 全部 shell out 到 `git clone` / `curl` / `wget` 子进程 | zshmodules(1)；[zinit](https://github.com/zdharma-continuum/zinit) README；[antidote](https://antidote.sh/) |
| **fish** | 引擎无 HTTP 客户端。`fish_update_completions` 现代版本只解析**本地** man 页；fish 2.x 曾联网补取 man 页，后被移除（次级来源，未能在现行 CHANGELOG.rst 中定位原文，见文末未验证声明） | fisher（脚本层插件管理器）由用户自举：`curl -sL …/fisher.fish \| source && fisher install …` | fish 官方命令文档；[fisher](https://github.com/jorgebucaran/fisher) README |
| **nushell** | 无。`plugin add` 只**注册本地二进制**到 plugin registry（`$nu.plugin-path`），不获取 | 获取靠 `cargo install`（cargo 生态）；社区包管理器 nupm 是**独立的脚本层项目** | [plugin add 文档](https://www.nushell.sh/commands/docs/plugin_add.html)；[nupm discussion #106](https://github.com/nushell/nupm/discussions/106) |
| **PowerShell** | 引擎（System.Management.Automation）无包下载 | PowerShellGet / PSResourceGet 是**随附但可独立更新的模块**（不是编译进引擎）；下载逻辑在模块内 | [Microsoft Learn: PowerShellGet vs NuGet](https://learn.microsoft.com)；[PowerShell Gallery](https://www.powershellgallery.com) |
| **neovim + lazy.nvim** | nvim 本体零下载 | lazy.nvim（lua 插件层）下载全部插件；**lazy.nvim 自己的安装（bootstrap）是 init.lua 配置文件里的 git-clone 片段** —— 引导是配置文件职责，不是引擎职责 | [lazy.nvim installation](https://lazy.folke.io/installation)：`if not fs_stat(lazypath) then vim.fn.system({"git","clone",…}) end` |
| **oh-my-zsh** | zsh 引擎不参与 | 脚本框架层：安装 `sh -c "$(curl -fsSL …/install.sh)"`；`omz update` = 对 `$ZSH` 跑 git pull；后台更新检查（`tools/check_for_upgrade.sh` 的 prompt/auto/background 模式） | [ohmyzsh README](https://github.com/ohmyzsh/ohmyzsh)；`tools/check_for_upgrade.sh` |
| （对照）**mason.nvim** | — | "安装器引擎"本身是一个 **nvim 插件**，由 lazy.nvim bootstrap；registry 是纯 YAML 数据仓库 | 本仓库 `docs/planning/lazy-family-source-study.md` §6 |

**平台产品反例（对 owner 直觉的制衡）**——引擎不下载是 shell 世界的定律，不是软件世界的：

| 产品 | 形态 |
| --- | --- |
| rustup / cargo | rustup 自带下载器并**下载 cargo**；cargo 又下载 crate。套件=工具链+包管理器，同一分发 |
| Node.js + npm | npm 是独立可执行，但**随 Node 发行版捆绑安装**，`npm install` 就在产品内 |
| VS Code | 扩展市场下载在产品内（不是扩展）；产品自更新也在产品内 |
| Windows Terminal | 字体/主题获取在设置 UI 内 |

结论：**"shell 引擎不下载"是行业定律；"shell 发行版捆绑一个包管理器"是平台常态。** niubash 的
定位是后者（README："AI-native bash-compatible shell **for Windows**, built on rubash + winuxcmd"——
它是一个发行版/套件，不只是一个解释器）。

## 2. 我们的调用面清单与当前分层

盘点方法：对 `D:/repo/niubash` 全仓 grep `ureq|WinHttp|curl|wget|TcpStream|ToSocketAddrs|socket|`
及 `git clone|http_get|download`（排除测试与注释），逐个打开源文件核对。**全部调用面共 6 个**，
无遗漏（rubash 仓库同样 grep 过：引擎零网络）。

| # | 调用面 | 位置 | 传输 | 触发路径 | 语义域判定 |
| --- | --- | --- | --- | --- | --- |
| 1 | `http_get_bytes()` / `install_executable()` | `crates/niubash-runtime/src/plugins/download.rs:167,236` | **ureq+rustls**（进程内，纯 Rust） | `niu plugin add/tool` 中 `driver = "download"` 的 recipe —— 目前仅 **2 行**（fzf v0.74.4、starship v1.26.0） | **管理器** |
| 2 | `git_clone_to()` / `git_fetch_commit_to()` | `crates/niubash-runtime/src/plugins/sources.rs:530,558` | **外部 `git.exe` 子进程**（要求 PATH 上有 git） | 全部 git 树来源：4 个 git-driver recipe + manager 来源（oh-my-bash 等）+ collections | **管理器** |
| 3 | 字体下载 `download()` | `crates/niubash-runtime/src/fonts.rs:189`（源 `:15`） | **外部 `curl.exe` 子进程**（System32，Win10 1803+） | `niu font` + setup 向导字体题 | **产品工具**（非管理器；font 是终端配置不是插件） |
| 4 | self-update 检查+安装器下载 `http_get()` | `src/self_update.rs:370`（WinHTTP）；后台检查 `maybe_print_update_hint()`（`src/main.rs:998` 调用，24h 节流，`NIU_UPDATE_CHECK`/CI 可关） | **WinHTTP**（OS 栈，零第三方依赖，仅 Windows） | `niu --self-update`、REPL `self-update` 命令、启动后台线程 | **产品自维护**（不能放管理器层：管理器坏了谁来修管理器） |
| 5 | 向导装 niu-git | `setup_wizard.rs:1193 install_niu_git`（命令串 `:163`） | **wpm 子进程**（`wpm install niugit`，仅在用户显式选 Install 时） | setup 向导 Q3 | **委托既有包管理器**（wpm 自己下载） |
| 6 | 向导 collections apply | `setup_wizard.rs`（`apply_plugin_collection` → `plugins::distros::apply`） | 继承 #1/#2 | setup 向导末段 | **管理器** |

关键结构事实（对分层判断很重要）：

- **调用面 1/2/5/6 全部经由 `niu plugin …` CLI 子命令或一次性向导**（`src/main.rs:1340-1362`
  分发），**REPL 热路径（shell.rs/repl.rs 的解释/补全/高亮循环）零调用**。进程行为上，
  "工具态的 niu"与"shell 态的 niu"已经分离；共享的只是同一个 exe 和同一个 crate。
- **REPL 内唯一可达的网络路径是 #4 的 self-update**（WinHTTP、自维护域）——这与 oh-my-zsh
  的后台更新检查同构（脚本层做后台检查在 shell 生态是被接受的）。
- recipe 索引 498 行中：489 行 `manager =`（骑在 manager 源上，走 #2 的 git）、4 行 git-driver、
  **2 行 download-driver**。即：**纯 Rust HTTP 通道今天服务 2 个 recipe**，而主力通道（git 树）
  本来就依赖外部 git.exe —— "无外部依赖的纯 Rust 传输"只覆盖最窄的一条道。
- `sha2`/`zip` 不独属下载：`sources.rs:30`（信任树校验）用 sha2，`fonts.rs:211` 用 zip 解包。
  真正只属于 HTTP 通道的依赖是 ureq/rustls/ring/webpki-roots/url + tar/flate2。
- 依赖声明：`crates/niubash-runtime/Cargo.toml:37-39`（ureq "2" tls / flate2 rust_backend / tar）。

## 3. 体积问题（实测，不动产品码）

在 `%TEMP%/dlsize` 建独立最小工程（两个 bin 共享依赖，profile 对齐 niu 的
release：thin LTO + codegen-units=1 + panic=abort），每个 API 面都被真实引用防 LTO 消除：

| 构建 | 大小 | 增量 |
| --- | --- | --- |
| 空 bin（基线） | 122,880 B | — |
| + ureq/rustls/ring/webpki-roots/url（TLS 栈） | 1,739,264 B | **+1.62 MB** |
| + flate2 + tar + zip + sha2（完整下载栈） | 1,885,696 B | **+1.76 MB 合计**（非 TLS 部分 +146 KB） |

对照：`target/release/niu.exe` = 9,261,056 B（8.83 MiB）。**下载栈 ≈ niu.exe 的 19%**，
其中 TLS 传输占 1.62 MB（ring 汇编 + rustls + 根证书表）。注意 sha2/zip 因 #2/#3 共享，
即便把 HTTP 通道整体搬走，它们仍会留在树里 —— 可搬走净额 ≈ 1.6 MB。

体积判断：19% 显著但非致命；且搬去独立 exe（选项 C）**不省产品总量**（TLS 栈只是搬家）。
真正能兑现"零下载 shell"承诺的形态是 feature 门控（见 §5）。

## 4. 核心矛盾分析

### 4.1 引导问题：谁安装管理器？

- **lazy.nvim 模式**：init.lua 里的 bootstrap 片段（`fs_stat` 检查 + `git clone`）。
  它是**没有安装器时的补偿**：nvim 已在，但管理器要自举，于是把自举代码放进配置文件。
  代价：首次启动需要网络+git、片段本身要容错、用户看得见这段魔法。
- **我们模式**：管理器编译进 runtime，随 Inno Setup 安装器分发。首次启动零网络零 git，
  bootstrap 语义由安装器承担。`vcs-status-externalization.md` 已经用过同一论证
  （"开箱体验"由捆绑层兜底）。
- **判定：引导问题对我们不存在。** 选项 D（首个插件=包管理器自己）解决的是一个我们没有的
  问题，还要把信任协议（ed25519 签名、staging、registry schema @0.3.0 —— 全是 Rust 侧资产，
  `sources.rs`/`trust.rs`）降级到 bash 脚本层重写，纯负收益。

### 4.2 耦合问题：引擎应该零网络吗？

**已经是。** rubash（`D:/repo/rubash`）`Cargo.toml` 无 ureq/reqwest/tokio/rustls，`src/` 无
WinHttp/TcpStream —— 引擎干净，无需动作。真正的耦合债务在 niubash 层：

1. **三个传输栈并存**：ureq（插件下载）、curl.exe（字体）、WinHTTP（self-update）。
   三套超时/重试/代理/UA 策略，doctor 排障要查三处。2026-10-03 ruling 选 ureq 是对
   "不驱动 wpm/apt"的裁决，不是对"多栈并存"的裁决。
2. **网络代码与 REPL 同 crate 同二进制**：交互 shell 进程常驻 TLS 栈（攻击面/沙箱标注），
   尽管热路径从不触网。
3. **历史上第三次换位**：lazy-family-source-study §9 原文先否决过"niubash 内置下载"
   （理由：wpm index 已带 per-platform sha256，`driver = "wpm"`），后被 2026-10-03 ruling
   推翻为纯 Rust 内置。owner 今天的疑问是对这次换位的自然回望 —— 值得一次性把边界
   **定成结构**而不是再换一次主意。

### 4.3 正确分层（四选项）

- **A — 现状（全在 runtime，模块级边界）**：`plugins/download.rs` 确实已是自包含模块、
  只有 `recipes.rs` 一个调用者。但"模块"不是"层"：fonts/wizard 绕过它各用各的传输，
  依赖无门控，谁也不能阻止下一个网络调用直接进 REPL 代码。
- **B — 独立 plugin-manager crate，编译进产品**：物理边界 + feature 门控；单二进制不变。
  注：任务描述说"当前实际就是这样"——**形似而未遂**：它是 runtime crate 里的兄弟模块，
  不是独立编译单元，无门控。
- **C — 独立可执行 `niu-pkg`（随产品安装，shell 子进程调用）**：进程级隔离；
  shell exe 减 ~1.6 MB；管理器可独立更新。代价：安装器/自更新要维护两个 exe 的版本
  同步；信任状态（registry/签名/staging）跨进程共享的协调成本；向导 apply 与
  `niu plugin` 的进度输出要走子进程管道；发行矩阵翻倍。
- **D — 插件层 bootstrap（lazy.nvim 模式）**：见 §4.1，被否定。

### 4.4 利弊矩阵

| 维度 | A 现状 | B crate 边界（内置） | C 独立 exe | D 插件层自举 |
| --- | --- | --- | --- | --- |
| 引导 | 安装器承担（好） | 同 A | 安装器须装两件 | **坏**：首启需网络+git，bash 层自举容错差 |
| 体积 | +1.76 MB | +1.76 MB（feature 可关 → 0） | shell -1.6 MB，产品总量不变 | shell -1.76 MB，管理器本体在别处 |
| 耦合 | 网络码与 REPL 同 crate；三传输栈并存 | 编译期禁止 REPL 触网；传输策略归一门 | 进程隔离（最强） | 引擎/产品全脱钩（理论最强） |
| 跨平台 | 好（ureq 纯 Rust） | 同 A | 好，但每个平台多一个产物 | 差：自举脚本要覆盖三平台 shell 差异 |
| 用户体验 | 单命令 `niu plugin add`，向导内联 | 同 A（零可见变化） | 同表面（子进程对用户透明），但失败模式多一层 | 首启体验最差 |
| 信任协议 | Rust 侧完整 | 同 A | 完整但跨进程共享状态 | **弱化**（脚本层重写 ed25519/staging） |
| 改造成本 | 0 | 低（提 crate + feature，一次机械移动） | 中高（分发/版本/IPC/向导改造） | 高（推倒重来） |
| 长期演进 | 边界会继续被绕过 | **C 的前置**：CLI 形 API 原样暴露成 main() 即可 | 终态之一 | 生态未成熟前不可行 |

## 5. 船长推荐（一条）

**选项 B 硬化版：把包管理器提为独立 workspace crate 并 feature 门控，保持单二进制分发。**

具体形态：

1. 新 crate `niu-pkg`（或 `niubash-pkg`）：迁入 `plugins/{download,recipes,sources,trust,
   catalog,distros,spec,sync,assets 的安装侧,descriptors}`。依赖只允许：anyhow/serde/toml/
   sha2/ed25519-dalek/ureq/flate2/tar/zip + path_utils。**禁止**依赖 reedline/crossterm/
   repl/shell —— 编译期保证"管理器不认识 shell"。
2. cargo feature：`network = ["ureq", "flate2", "tar"]`。`niu build --no-default-features`
   产出**可验证的零网络 shell 二进制**（git 树来源仍走外部 git 子进程——那是用户机器上的
   git，不是我们进程里的 TLS 栈）。这一步把 owner 的直觉落成可测试的构建形态。
3. 传输统一收口进 `niu-pkg`：#3 字体从 curl.exe 迁到 ureq（owner 已在迁），#1/#3/#6 共用
   一个 client（超时/UA/重试/代理一套策略）；#4 self-update 保留 WinHTTP（OS 栈零依赖、
   自维护域，且是 Windows 专属通道）——但把"为什么两个栈"写进 crate 文档。
4. 写明 **C 的迁移触发条件**（进 crate README）：(a) 产品把 shell 二进制体积/网络能力列为
   硬约束（沙箱、企业策略、AV 误报网络型 shell）；(b) 管理器需要独立于 shell 的发布节奏；
   (c) 第三方需要 headless 驱动包管理器。届时 `niu-pkg` 的函数接口原样挂一个 `main()`
   即成独立 exe —— B 是 C 的无损前置，A 不是。

**理由（按 owner 的三个潜在关切排）：**

- *"没见过 shell 承担这么多下载"* —— 业界定律约束的是**引擎**，我们的引擎（rubash）已经零网络。
  niubash 是 shell 发行版（对照 rustup/npm/VS Code），捆绑包管理器是常态而非异类。需要修正的
  不是"有没有"，而是"边界是否显式"：当前它是 runtime crate 里一个没有门控的模块，配着三个
  传输栈 —— 这才是让人（正确地）觉得"shell 在下载"的原因。B 把"产品=shell+包管理器"变成
  显式结构，管理器有名字、有边界、可单独演进。
- *"以插件形式挂载？"* —— 即选项 D。业界确实这么做（lazy.nvim/fisher/nupm 全是插件/脚本层），
  但他们全部是为"宿主已存在、没有安装器"设计的自举补偿，且无一例外把下载实现降级为
  git/curl 子进程调用。我们的信任协议（ed25519 + staging + checksum lock）是 Rust 资产，
  搬进 bash 插件层是纯降级。**不采。**
- *体积/风险* —— 19% 的占比用 feature 门控回应（可归零）；进程隔离（C）的真实收益
  （1.6 MB、TLS 不常驻 shell 进程）小于其分发/版本/向导改造成本，且 B→C 无损，
  不必现在付。

**与仓库既有法律的一致性**：`plugin-externalization-readiness.md` 明令"不要为了让 bundle
显得更大而外部化 native builtin / 语义"；`vcs-status-externalization.md` 的"宿主只留引擎"
针对 prompt 能力。包管理器两者都不是：它不是 shell 语义（不该进引擎），也不是可 source 的
脚本资产（不该进 bundle）——它是**产品工具层**，正确的家是独立 crate + feature 门控。

## 附：本次评估的证据物

- 体积实验：`%TEMP%/dlsize`（独立工程，未触碰仓库）；方法与原始数字见 §3。
- 调用面源码锚点见 §2 表；`src/self_update.rs`、`plugins/download.rs`、`plugins/sources.rs`、
  `fonts.rs`、`setup_wizard.rs` 均已逐行核对。
- 业界引用链接见 §1 表内。
- 未验证项（诚实声明）：fish 2.x 联网取 man 页的具体 changelog 原文未能从
  fish-shell 仓库现行 CHANGELOG.rst（只覆盖较新版本）中定位，该条为次级来源；
  不影响任何结论（fish 现状"只解析本地 man 页"是一手确认的）。

---

## 终局裁定（2026-10-04，owner 最终裁决；本节取代上文所有"待裁定"选项）

Owner 裁定：**收缩，不是搬家。** niu 是纯 bash 生态插件管理器，
shell 本体承担零网络/HTTP 下载职责——本报告评估的选项 A/B/C（内置
驱动 / 独立 crate + feature / 独立 exe）全部作废，HTTP 依赖整体移除：

- **删除**：`plugins/download.rs` 整模块与 `ureq`/`flate2`/`tar`/`zip`
  依赖（wt50/dlretract）。锁文件经普通 `cargo build` 增量修剪（未跑
  wholesale `cargo update`；windows-sys 0.59/0.61.2 钉未动，0.52 随
  ring/ureq 树离开）。
- **归属**：可执行工具归真实包管理器——Windows 上 wpm 第一顺位
  （owner 更正 2026-10-03），winget/scoop 为 wpm 不携带物（GUI、字体）
  的补充；其他平台 apt/dnf/yum/brew，非 Windows 构建零 wpm 字符串。
- **插件驱动**：`niu plugin add <git-url>`（git clone）是唯一扩展安装
  入口；recipe 索引降为目录元数据，download 行的 `add` 打印包管理器
  推荐而非 fetch。
- **字体**：检测+推荐（winget/scoop/brew/nerdfonts.com），无下载无解压。
- **镜像**：HTTP prefix 通道删除，仅存 git insteadOf（git-only）。
- **自更新**：self_update（WinHTTP）为产品自身更新通道，不属扩展安装，
  保留直连。

§3 的体积实验与 §2 的调用面盘点作为历史证据保留；其"缺的是编译期
边界"的判断以最彻底的形式（整个删除）落地。
