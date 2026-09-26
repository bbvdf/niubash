# oh-my-niu 生态集成与 setup 体验升级 — 设计文档

状态：设计稿（未实现、未提交）
日期：2026-09-26
输入：`crates/niubash-runtime` 现状代码；`D:/repo/rubash/target-ecosys/REPORT.md`（第一轮生态验收）；`D:/repo/rubash/target-ecosys2/REPORT.md`（577 文件 conformance 全扫 + 117 插件冒烟 + 冲突矩阵）。

## 0. 总原则："别人的更好，我们的作为保底"

外部生态（oh-my-bash 主题/插件、bash-completion）是**主选**；niubash 自带的
原生内容（原生 TOML 主题、原生 CommandDef 补全、原生 prompt）是**保底默认**，
只在外部生态缺失或不可用时回落。落到产品语义上就是三条铁律：

1. **优先级永远外部在前**：bundle 里外部资产的名字排在原生资产前面；同名冲突
   时外部胜出（原生仍可通过显式前缀 `native:<name>` 指定）。
2. **保底链永远可用**：外部资产缺席（未装 bundle、引擎缺陷未修、脚本 source
   失败）时，静默回落到原生路径，Tab/提示符不能因适配器故障而坏掉。回落必须
   显式告知（doctor / setup 摘要里注明"当前使用保底层"）。
3. **失败可回滚**：setup 每一步的写入（rc、bundle 版本、适配器安装）都记入
   setup journal，Apply 完成界面打印逐条 undo 命令；bundle 回滚复用现有
   `niu plugin rollback` 通道。

## 1. 现状盘点（file:line 锚点）

### 1.1 插件/bundle 机制（`crates/niubash-runtime/src/plugins/mod.rs`）

| 机制 | 位置 | 说明 |
|---|---|---|
| 官方 bundle 名 | `mod.rs:16-23` | `oh-my-niu`，legacy 名兼容 |
| 版本/协议常量 | `mod.rs:72-78` | `PLUGIN_INDEX_SCHEMA = niubash:plugin-index@0.1.0`、`PLUGIN_API_VERSION`、签名策略 `unsupported` |
| bundle 清单/布局 | `mod.rs:361-422` | `BundleToml` + `BundleLayoutToml`：`packs/aliases/completions/prompts/keybindings/themes` 六个逻辑目录，均可被 bundle.toml 覆盖 |
| inventory 解析链 | `mod.rs:603-647` | env 覆盖 `NIU_PLUGIN_BUNDLE_PATH` → plugin lock（`~/.niubash/plugin-lock.toml`）→ 应用旁 `bundles/` → 编译期 fallback |
| 未信任外部 bundle 跳过 | `mod.rs:652-654` | `skip_untrusted_external_bundle` |
| 外部 bundle 激活 | `mod.rs:659-694` | `activate_external_bundle`（写 lock，保留 `previous_path`） |
| 框架插件（`plugins/<name>/plugin.toml`） | `mod.rs:779-858` | `FrameworkPluginToml`，entry 必须是 plugin 本地相对路径（`mod.rs:859-876`） |
| 加载排序/默认 | `mod.rs:903-920` | `framework_plugin_sort_key`：prompt-core(0)→git(10)→工具(20/30)→hints(40)→theme-minimal(50)→theme-*(60) |
| 编译期兜底 packs | `mod.rs:921-1088` | git/docker/kubectl/npm/zoxide/…/prompts/themes |
| bundle 安装（staging + sha256 + lock + previous） | `mod.rs:2327-2425` | `apply_plugin_bundle_update_from_path` |
| bundle 回滚 | `mod.rs:2426-2462` | `apply_plugin_bundle_rollback` |
| 安装校验（schema/api/pack 契约/index 对账） | `mod.rs:2463-2681` | `validate_bundle_inventory_for_update` 等 |
| source pack 契约 | `mod.rs:2745-2792` | **entry 只允许 `.niu`/`.winux`（`mod.rs:2768`）**，须声明 `shell:source` 权限 |
| 补全资产加载 | `mod.rs:3002-3055` | `plugin_completion_defs`：bundle TOML → 编译兜底（`git_completion_def` 在 `mod.rs:3057`） |
| 主题目录/单主题 | `mod.rs:3172-3318` | `plugin_theme_catalog`（user + bundle TOML）、`plugin_theme`、`bundle_asset_path`（`mod.rs:3346-3365`） |
| rc 启停 pack | `mod.rs:3512-3573` | `active_pack_names` / `enable_pack_in_rc` / `disable_pack_in_rc`（编辑 `NIU_PLUGINS=(…)` 行） |

外部 bundle 通道（`plugins/external.rs`）：`add_bundle`（git clone，注册
`~/.niubash/external/registry.toml`，**默认不信任**）`external.rs:115-197`；
`trust_bundle` `external.rs:200-212`；`remove_bundle` `external.rs:216-234`。
CLI 已有：`niu plugin add/trust/use/remove/rollback`（`src/main.rs:1219-1267,
1506-1511`）。

### 1.2 setup 向导（`setup_wizard.rs`，线性流程）

- 常量与预设模型：`WizardConfig` `setup_wizard.rs:158-173`、`Preset`
  `setup_wizard.rs:177-219`（bundle `presets/*.toml` 可扩展，
  `load_presets` `setup_wizard.rs:1596-1624`）。
- 现有流程 `run_wizard_inner` `setup_wizard.rs:488-870`：环境探测 → 字体
  （540-615）→ 预设三选一+custom（617-663）→ starship 引擎（666-722）→
  wpm 工具套餐（724-807）→ WT profile（809-816）→ 摘要+确认（818-830）。
- 主题选择只在 custom 流里（`custom_flow` `setup_wizard.rs:911-1121`），
  数据源是 `theme::list_available_names()`（`setup_wizard.rs:940`），预览用
  `choice_preview`（`WizardIo` `setup_wizard.rs:295-388`，底层
  `interactive_menu::interactive_choice_ex` 已支持高亮跟随预览）。
- 预览渲染 `theme_preview_line` `setup_wizard.rs:1956-1967`（原生 TOML 主题
  专用）；Nerd Font 判定是**硬编码名单** `nerd_font_theme`
  `setup_wizard.rs:1969-1983`。
- rc 生成 `generate_rc` `setup_wizard.rs:1708-1882`：写
  `NIU_THEME/NIU_THEME_PLUGIN/NIU_PLUGINS`（1824-1833），bundle 定位与
  source（1851-1865），备份到 `~/.niubash/backups`（`write_primary_rc`
  `setup_wizard.rs:1918-1947`）。
- 非交互路径直接落 minimal 预设 `setup_wizard.rs:532-537`。

### 1.3 原生 prompt / 主题 / 补全（保底层）

- 主题解析链 `theme.rs:53-71`：user（`~/.niubash/themes/*.toml`，
  `theme.rs:103`）→ bundle（`themes_dir` TOML，经
  `plugins::plugin_theme`）→ 编译默认。`list_available_names`
  `theme.rs:76-99`。
- prompt 双通道：原生模板渲染 + **bash 兼容通道**
  （`prompt.rs:588` 注释："Bash-compatible prompt values rendered from
  PS1/PS2 after the shell has run public Bash prompt hooks such as
  PROMPT_COMMAND"）。starship full 模式已演示"外部接管 PS1、原生禁用"的先例
  （`GitBackend::StarshipFull` `setup_wizard.rs:148-154`）。
- 原生补全栈：`NiubashCompleter` 插件管线（`completion/completer.rs:62-90`
  `load_completion_dirs_with_bundle_and_definitions`：bundle defs → 注入
  defs → 用户目录）；`ExternalCompletionPlugin`
  （`completion/external.rs:269-332`）读 TOML `CommandDef` 且**自动导入
  clap/cobra 形态的 `.bash` 脚本**（`completion/bash_import.rs:29-45`，mtime
  缓存 `<cmd>.parsed.toml`）；`RuntimeCompletionPlugin`
  （`completion/runtime.rs:17-27`）跑外部补全命令。用户目录优先级：用户
  `.toml` > `.bash` 自动导入（`external.rs:334-345` 注释）。
- 引擎侧（rubash，`Cargo.toml:23` + `[patch] Cargo.toml:51-52` 指向
  `../rubash`）已修 `complete -D/-E/-I`（rubash#133 CLOSED）。

### 1.4 fonts / doctor / CLI

- `fonts.rs`：`FONT_OPTIONS` `fonts.rs:30`、`menu_labels` `fonts.rs:63`、
  `nerd_font_installed` `fonts.rs:70`、`install` `fonts.rs:76`、
  `niu font` `fonts.rs:103`。
- `doctor.rs:17-166 run_doctor`：4 项 critical（winuxcmd/command links/rc/
  **plugin bundle**，108-123）+ advisory（字体/终端/语言）。尚无补全与
  生态体检项。
- `src/main.rs:164-170`：`niu setup|font|doctor|plugin` 分发。

### 1.5 生态验收结论（设计输入）

target-ecosys（第一轮）：

- 许可证：oh-my-bash **MIT**（可官方捆绑）；bash-completion
  **GPL-2.0-or-later**（不可 vendor 进 MIT 仓库，只能适配器/用户自装）；
  fzf/starship/bash-preexec 宽松。
- bash-completion 2.18：`complete -D` 动态入口当时被 rubash#133 拒（**已修**）；
  引擎剩余断点：桶#1 引号家族（`bash_completion:188`、`ssh.bash:457`，加载
  即死）、桶#4/#5 `_filedir` 载波崩溃/泄漏、桶#6 `compgen -F` 不执行函数、
  桶#8 补全函数内 cword/cur 空。452 文件中 439 个（97.3%）已可隔离加载。
- oh-my-bash 主题：5/5 主题 PS1 全空，唯一根因 **rubash#148**（`eval
  arr=(glob)` 在 nullglob 下不做路径展开，级联杀死 `_omb_module_require`
  lib 层；issue 已建，OPEN，修复在途）+ C-2（`-i` 管道 stdin 时 `$-` 缺
  `i/s`）。PROMPT_COMMAND 钩子值双侧已一致（与 `prompt.rs:588` 通道吻合）。

target-ecosys2（全量 conformance）：

- 语料 577 文件，`-n` 全量 A 级分歧仅 2（其中 A-2：zsh 风格 `${(...)` 使
  **git-completion.bash 整体不可 source**，P0）。
- 117 插件冒烟：88 PASS-IDENTICAL / 28 FAIL-CONSISTENT（宿主框架函数缺失，
  **非引擎问题**）/ 1 环境差异。**5 个 no-op 垫片函数即可让 28 个框架插件
  转绿**：OMB 侧 `_omb_module_require`、`_omb_util_command_exists`、
  `_omb_util_print`；bash-it 侧 `cite`、`about-plugin`。
- 符号冲突全生态仅 3 个：alias `g`（bashmarks × git）、alias `d`
  （bash-it dirs × bashmarks）、func `set_xterm_title`（bash-it xterm ×
  OMB xterm）。
- 精选 bundle 候选（双侧一致、零外部依赖、无冲突）：OMB `git`、`bashmarks`、
  `golang`、`npm`、`dotnet`、`ansible`；bash-it `colors`、`base`、`proxy`、
  `pack`、`dirs`。
- 测量纪律：oh-my-bash checkout 必须 `core.autocrlf=false`（无
  .gitattributes）；WSL 基线须裁剪 PATH。

## 2. 分层设计与保底链

```
主题层    主选：bundle 内 oh-my-bash 主题（ven/oh-my-bash，MIT）
          保底：bundle 原生 TOML 主题（themes/）→ 编译默认 classic
          通道：OMB 主题走 bash 兼容 PS1/PROMPT_COMMAND（prompt.rs:588），
                原生主题走现有模板渲染

补全层    主选：bash-completion（GPL，用户侧获取，适配器探测/装/source）
                + git-completion.bash / kubectl(cobra) 等单脚本接入
          保底：原生 CommandDef（bundle completions/*.toml + 编译
                git/docker/kubectl/npm）+ path/command/variables 补全器
          入口：complete -D 动态注册（rubash#133 已修）

插件层    主选：官方 bundle 精选 OMB/bash-it 插件 + omb-compat 垫片包
          保底：编译期原生 packs（git/docker/kubectl/npm/…）
          治理：3 组符号冲突检测；外部 bundle 走 add→trust→use
```

每一层都是一个"探测 → 主选可用即用 → 不可用回落保底并告知"的状态机，
状态收敛到 doctor 与 setup 摘要。

## 3. 主题层设计

### 3.1 bundle 资产：vendor oh-my-bash

- 官方 bundle 新增目录 `ven/oh-my-bash/`：`LICENSE.md` + `lib/`（22 文件）+
  `tools/`（含 git-completion.bash，供补全层复用）+ `themes/`
  （82 个 `*.theme.sh`）。**不打包** `oh-my-bash.sh` 之外的安装器逻辑；
  加载走 niubash 自己的入口（见 3.3）。bundle 构建管线负责：固定 commit、
  LF 规范化（OMB 无 .gitattributes，CRLF 会杀死 source）、生成主题清单。
- `BundleLayoutToml`（`plugins/mod.rs:378-392`）新增
  `omb_themes_dir`（默认 `"ven/oh-my-bash/themes"`）与
  `omb_root_dir`（默认 `"ven/oh-my-bash"`）；`bundle_asset_path`
  （`mod.rs:3346-3365`）映射两个新逻辑目录。
- OMB 主题不走 `PluginExportsRecord.themes` 逐个注册成 pack（82 个 pack 会
  淹没 `niu plugin list`）。改为**目录级清单**：`ven/oh-my-bash/
  themes.index.toml`（构建期生成：name、文件、是否需要 nerd font、一句话
 描述——数据来源为验收矩阵与主题头部注释），由新函数
  `omb_theme_entries()` 一次性加载。

### 3.2 主题解析链与 OMB_THEME 映射

`theme.rs:53 by_name` 现链 user→bundle-native→default。新链：

1. `~/.niubash/themes/<name>.toml`（用户覆盖，永远最高）
2. OMB 主题（`omb_theme_entries()` 命中）→ **主选**
3. bundle 原生 TOML（现有 `plugin_theme` `mod.rs:3293`）
4. 编译默认（classic）

规则：

- `NIU_THEME=<name>` 平铺命名，OMB 与原生同名时（如 `agnoster` 两侧都有）
  **OMB 胜**（原则 1）；显式 `native:<name>` 可选原生版。
- **OMB_THEME 兼容**：rc 里出现 `OMB_THEME=<name>` 且 `NIU_THEME` 未设时，
  等价映射为 NIU_THEME（omb 侧命名即平铺名，无需改名表；仅记录
  `NIU_THEME_SOURCE=omb`）。反向：选中 OMB 主题时 loader 同时导出
  `OMB_THEME`/`OSH`/`OSH_THEME`，让依赖这些变量的 OMB 插件（bashmarks 等）
  正常工作。
- 选中 OMB 主题时 rc 不写 `NIU_THEME_PLUGIN=`（那是原生 theme-* pack 通道），
  改写 `NIU_THEME_SOURCE=omb`；摘要在主题行标注来源。

### 3.3 加载通道与 canary 保底

- OMB 主题依赖 OMB lib 层（`_omb_util_*`、`_omb_module_require`），其根因
  `eval arr=(glob)` 正是 **rubash#148**。因此加载入口设计为：bundle loader
  （`oh-my-niu.niu`）在 `NIU_THEME_SOURCE=omb` 时，设
  `OSH="$NIUBASH/ven/oh-my-bash"`、`OSH_THEME=<name>` 后 source OMB 的
  `oh-my-bash.sh` 主体（保留其官方加载路径，不自造子集）。主题随后设置
  PS1/PROMPT_COMMAND，由 bash 兼容通道（`prompt.rs:588`）渲染——验收已证
  PROMPT_COMMAND 钩子值双侧一致。
- **canary 保底**（对应原则 2）：`niu setup` 选 OMB 主题时与每次 doctor，
  在子 shell 里跑 canary（source 主题 → 检查 `PS1` 非空且
  `PROMPT_COMMAND` 含 `_omb_util_prompt_command_hook`）。canary 失败
  （#148 未修/引擎回归）则：setup 页面把该主题标灰并注明
  "等待引擎 rubash#148"，默认选择回落原生对应主题；已有 rc 则启动时告警一
  次并回落 `native:<同名或 classic>`。原生映射表（agnoster→原生 agnoster、
  robbyrussell→p10-lean、brainy→multiline classic 等）放清单文件，可运营
  期补齐。
- 原生主题标注为**保底默认**：bundle 未安装或 canary 失败时的默认链不变
  （`theme.rs` 编译默认 + bundle 原生主题）。

### 3.4 setup 主题页：全量画廊

升级 `custom_flow` 主题块（`setup_wizard.rs:939-1066`）：

- 数据源换成新函数 `theme_gallery_entries()`：user + OMB（82）+ 原生 bundle
  主题 + 编译原生，合并去重（同名 OMB 优先），每条带
  `source: user|oh-my-bash|native(保底)|compiled(保底)` 徽标、nerd-font
  标记（改硬编码名单 `nerd_font_theme` `setup_wizard.rs:1969` 为清单字段）、
  可用性（canary 结果）。
- 交互沿用 `choice_preview`（`setup_wizard.rs:324`）逐条高亮预览：
  - 原生主题：现有 `theme_preview_line`（`setup_wizard.rs:1956`）。
  - OMB 主题：新 `omb_theme_preview()` —— 子进程
    `niu -i` source 主题后捕获渲染后的 PS1 样串（走 prompt.rs:588 同一
    渲染路径的文本化输出），**按主题名做进程内缓存**（向导一次会话内每主
    题只跑一次；82 主题仅在高亮停留时逐个触发）。canary 失败的主题预览
    显示"等待 rubash#148"占位而非空白。
- 默认选中项：bundle 在位且 canary 通过 → 上次选择或 `robbyrussell`
  （OMB 最常用纯文本主题）；否则原生 `p10-classic`/`classic`（现逻辑
  `setup_wizard.rs:953-957` 保留为保底分支）。

## 4. 补全层设计（适配器架构）

### 4.1 探测 → source → 注册

新模块 `crates/niubash-runtime/src/completion/bash_completion.rs`：

- `probe() -> BashCompletionCandidate { path, origin, version }`，探测顺序：
  1. `NIU_BASH_COMPLETION_PATH`（显式覆盖，含 rc 数组形式）
  2. niu 托管安装：`~/.niubash/external/bash-completion/`（见下）
  3. 系统共享：Git-for-Windows
     `C:/Program Files/Git/usr/share/bash-completion/bash_completion`、
     MSYS2 同路径变体
  4. 包管理器落点：scoop `~scoop/apps/.../persist/...`、winget 用户包目录
     下 `bash-completion/`（存在则用；两者都不是首选安装通道）
  5. 用户 rc 声明路径
- `niu completion install`：`git clone --depth 1` 固定 tag（2.18+）到
  `~/.niubash/external/bash-completion`，登记进现有 external registry
  （复用 `plugins/external.rs` 的注册表结构；GPL 内容只落在用户目录，
  MIT 仓库零 vendor）。`niu completion uninstall/status` 对应 remove/查询。
- **source 与注册**：setup 生成的 rc 在 bundle loader 之后追加：

  ```sh
  # bash-completion (external, primary); native stays as fallback
  if [ -r "${NIU_BASH_COMPLETION_PATH:-$HOME/.niubash/external/bash-completion/bash_completion}" ]; then
    . "$_"
  fi
  ```

  bash-completion 自身经 `complete -D -F _comp_complete_load` 完成动态注册
  （引擎 rubash#133 已修，**此前提已满足**）。`complete -p` 注册数与
  `-D` 条目在 doctor 里可见。
- **激活门槛**：桶#1 引号家族（`bash_completion:188`、`ssh.bash:457`）未修
  前整文件 source 即死。适配器必须先做**引擎就绪探测**（子 shell source
  最小探针，或按引擎版本门控），不就绪时 `niu completion status` 报
  "installed, engine parser not ready（等待 rubash 引号家族修复）"，rc 不
  写 source 行，保底层继续服务。

### 4.2 引擎桥：函数补全优先、原生保底

Tab 管线（`NiubashCompleter`）新增插件 `BashScriptCompletionPlugin`，插在
`ExternalCompletionPlugin` **之前**（改 `completer.rs:62-90` 装配序）：

1. 查询引擎 `complete` 注册表（rubash 引擎 API：按命令名取
   `-F funcname/-W wordlist/-C command` 注册）；
2. 命中 `-F`：在引擎内以 COMP_WORDS/COMP_CWORD/COMP_LINE 执行函数、收集
   COMPREPLY（依赖引擎桶#6/#8 修复）；命中 `-W/-C/-G` 直接产生候选；
3. 未命中或执行失败 → 落回原生命令/外部 TOML/路径补全（**保底，原则 2**）。

bash-completion 注册过的命令（git、ssh、tar…）由引擎函数补全主选；
未注册命令继续走原生 `CommandDef`。适配器任何失败不阻断 Tab。

### 4.3 单脚本接入点（git / kubectl 等）

| 工具 | 主选路径 | 保底路径 | 引擎依赖 |
|---|---|---|---|
| git | bash-completion `completions/git` 或 bundle `ven/oh-my-bash/tools/git-completion.bash`（挂到动态加载目录） | bundle `completions/git.toml` + 编译 `git_completion_def`（`mod.rs:3057`） | A-2：zsh `${(...)` 解析（target-ecosys2 Issue 1，P0）修前 git-completion.bash 整体不可 source |
| kubectl | `kubectl completion bash` 输出入 `~/.niubash/completions/`，经引擎 `complete -F`（`__start_kubectl` 是函数型，**不走** bash_import） | 原生 simple def（`mod.rs:3052`） | 桶#6/#8 |
| clap/cobra `opts=` 形态 | 已有：`ExternalCompletionPlugin::load_dir` 自动导入 `.bash`（`bash_import.rs`） | — | 无（已工作） |
| 通用 | bash-completion 452 文件（439 已可加载） | 原生 + 路径/命令补全 | 桶#1（两个文件级断点） |

新增 CLI：`niu completion status|install|uninstall|register <cmd> [--script <path>]`
（`src/main.rs:164-170` 分发处扩展），`register` 负责把单脚本放入
`~/.niubash/completions/` 并按形态选择引擎函数路径或 bash_import 路径。

## 5. 插件层设计

### 5.1 精选生态 packs 进官方 bundle

- 目录形态沿用框架插件：`plugins/<name>/<name>.plugin.sh` + `plugin.toml`
  （`FrameworkPluginToml` `mod.rs:444-465`）。精选集（target-ecosys2 建议，
  全部 PASS-IDENTICAL、零外部依赖）：
  `omb-bashmarks`、`omb-git`、`omb-golang`、`omb-npm`、`omb-dotnet`、
  `omb-ansible`、`bit-colors`、`bit-base`、`bit-proxy`、`bit-pack`、
  `bit-dirs`。前缀消歧：`omb-`/`bit-` 避免与原生 `git`/`npm` pack 撞名。
- **契约放宽**：`validate_source_pack_contract`（`mod.rs:2745-2792`）现要求
  entry 以 `.niu`/`.winux` 结尾（`mod.rs:2768`）。新增允许 `.sh`（生态
  pack 原文），仍强制 `shell:source` 权限与 plugin 本地相对路径检查；同步
  更新 `framework_plugin_source_entry`（`mod.rs:859-876`）无需变（仅拼路径）
  与安装校验测试。
- `framework_plugin_sort_key`（`mod.rs:909-920`）给生态 pack 排 55（在
  theme-* 之前、工具之后），`framework_plugin_default`（`mod.rs:903`）保持
  false（保底默认仍是原生 pack，生态 pack 显式 opt-in）。
- 每个 vendor 目录带来源 LICENSE（OMB MIT / bash-it MIT）与 commit 记录，
  进 `index.toml` 的新 `[provenance]` 段（安装校验 `mod.rs:2520` 顺带对账）。

### 5.2 omb-compat 垫片包（5 函数解 28 插件）

- 新 pack `plugins/omb-compat/`，sort key 5（`prompt-core` 之后、`git` 之
  前——垫片必须先于一切生态插件加载），entry 为 `.niu` 垫片脚本，定义：

  - `_omb_module_require`（登记已加载、返回 0；不做真实加载——主题不走此
    路径，见 3.3，插件包按验收结论自包含）
  - `_omb_util_command_exists`、`_omb_util_print`
  - `cite`、`about-plugin`（bash-it 侧 no-op）

  可选追加 `_omb_util_print_prompt`、`_omb_deprecate_defun_print/put`、
  `group`、`url`（验收 err 里出现过的次级缺失，装上更稳）。
- 效果：外部 bundle 用户（`niu plugin add oh-my-bash` 原仓库）与官方精选
  packs 共享同一垫片；28 个 FAIL-CONSISTENT 插件转绿。

### 5.3 符号冲突检测（g / d / set_xterm_title）

- bundle 新增 `conflicts.toml`（进入 index 对账）：

  ```toml
  [[conflicts]]
  symbol = "alias:g"   packs = ["omb-bashmarks", "omb-git"]
  [[conflicts]]
  symbol = "alias:d"   packs = ["omb-bashmarks", "bit-dirs"]
  [[conflicts]]
  symbol = "func:set_xterm_title" packs = ["omb-xterm", "bit-xterm"]
  ```

- 检测实现放 `plugins/mod.rs`（近 `active_pack_names` `mod.rs:3512`）：
  `detect_symbol_conflicts(inventory, active) -> Vec<ConflictWarning>`。
  除 curated 表外做**通用 alias 交集**：对启用中的
  `exports.aliases=true` pack 逐个 `load_bundle_aliases_from_path`
  （`mod.rs:2945`）取别名集合，两两求交（`g`/`d` 可被通用算法自然发现；
  `set_xterm_title` 类函数冲突靠 curated 表）。
- 接入两个点：
  1. `niu plugin enable`（CLI 路径 `main.rs` → `enable_pack_in_rc`
     `mod.rs:3537`）：冲突时列出双方与符号，要求
     `--force` 或改选（bashmarks 与 git 二选一等提示）。
  2. 启动加载器：仅告警一次（后加载者胜出，bash 语义本就如此），把"谁覆
     盖了谁"写进 doctor。

### 5.4 `plugin trust` 在 setup 里的引导

- setup 插件节扫描 external registry（`external.rs:59 read_registry`）：
  - 存在 registered-but-untrusted：显示 bundle 路径与
    `plugin_permission_review_text`（`mod.rs:2163`）摘要，选项：
    [查看权限] [执行 `niu plugin trust <name>`] [跳过]。确认信任后可选
    `niu plugin use <name>`（调 `activate_external_bundle` `mod.rs:659`，
    lock 自动保留 previous_path，可 `niu plugin rollback`）。
  - 未信任前 resolver 本就跳过其 packs（`mod.rs:652`），无需新增安全语义。
- 摘要与 doctor 里呈现 trust 状态。

## 6. setup 体验：分节向导

### 6.1 结构

`run_wizard_inner`（`setup_wizard.rs:488`）从线性流程改为**分节向导**：

```
niu setup
 ├─ 0 环境探测（EnvProbe 不变，setup_wizard.rs:222-293）
 ├─ 1 预设快车道（首次运行：recommended/poweruser/minimal/custom）→ 命中预设
 │     则按预设填节默认值，仍进节目录让用户微调
 ├─ 2 节目录（可乱序进入，Esc 快进语义沿用 WizardIo setup_wizard.rs:295）
 │    ├─ 主题 Theme     （全量画廊，§3.4）
 │    ├─ 补全 Completion（§4 探测/install/单脚本接入，缺省=保底说明）
 │    ├─ 插件 Plugins   （精选 packs 多选 + 冲突检测 + trust 引导，§5）
 │    ├─ 字体 Font      （现字体节原样搬入 setup_wizard.rs:540-615）
 │    ├─ 工具 Tools     （wpm 套餐，setup_wizard.rs:724-807）
 │    └─ 终端 Terminal  （WT profile，setup_wizard.rs:809-816）
 ├─ 3 摘要 + Apply/Cancel（print_config_summary 扩展，setup_wizard.rs:1124）
 └─ 4 Apply：写 rc（备份不变）+ setup journal + 打印 undo 命令
```

- 单节重跑：`niu setup --section theme|completion|plugins|font|tools|terminal`
  （`main.rs:164` 分发加参数；等价 presets 通道 `apply_preset`
  `setup_wizard.rs:873` 的节级版本）。非交互仍走 minimal 预设
  （`setup_wizard.rs:532-537` 不变）。
- 插件节需要**多选菜单**：`interactive_menu` 现为单选；新增
  `interactive_multi_choice`（空格切换、回车确认）或退化为逐项 yn——作为
  WP-S1 的一部分。
- 每节文案进 `zh()` 表（`setup_wizard.rs:1989`）。

### 6.2 预览与信任提示

- 主题节：高亮实时预览（原生 + OMB 子进程缓存，§3.4）。
- 补全节：探测结果即时显示（找到 bash-completion：路径/版本/将注册的
  complete -D；未找到：提供 install 或"使用保底补全"两选）。
- 插件节：pack 行内显示来源徽标（oh-my-bash/bash-it/原生保底）、权限高亮
  （复用 `permission_detail` `mod.rs:2263` 的 risk 着色）、冲突即时警告。

### 6.3 回滚（setup journal）

- `write_rc_and_mark_done`（`setup_wizard.rs:1182`）扩展：Apply 时同步写
  `~/.niubash/setup-journal.toml`：节→变更记录（rc 备份路径、选择的主题/
  packs、bundle 版本 before/after（读 `PluginLockToml` `mod.rs:499`）、
  适配器安装记录、信任的 external bundle 名）。
- 完成界面打印逐条 undo：

  ```
  niu plugin rollback oh-my-niu        # bundle 回到 <prev-version>
  niu completion uninstall             # 移除 bash-completion 克隆
  niu plugin remove <external-name>    # 撤销信任/使用的外部 bundle
  cp ~/.niubash/backups/.niubashrc.<stamp>.bak ~/.niubashrc
  ```

  bundle 回滚直接复用 `apply_plugin_bundle_rollback`（`mod.rs:2426`）与
  `niu plugin rollback`（`main.rs:1506`），不新造机制。

## 7. doctor 生态体检项

`doctor.rs:17 run_doctor` 新增（advisory 为主，不提高 critical 门槛）：

| 行 | 内容 | 判定 |
|---|---|---|
| bash-completion | `probe()` 结果：路径 + 版本 + "engine ready/等待引擎修复"；未装则 "native fallback active（保底层在岗）" | advisory |
| complete -D | 子 shell `complete -p` 含 `-D` 注册条数（>0 = 动态加载入口活） | advisory |
| bundle 完整性 | 现有 plugin bundle 行（`doctor.rs:108-123`）升级：inventory source/trust_source、lock `checksum_sha256`（archive 安装）或 git HEAD（克隆安装）对账，失配告警 | 升级现有 critical |
| OMB 主题 canary | 抽当前 OMB 主题跑 canary（§3.3）；失败提示 rubash#148 状态与保底回落 | advisory |
| 符号冲突 | `detect_symbol_conflicts` 结果（§5.3） | advisory |

## 8. 工作包拆解（按依赖排序）

> 尺寸为估算的净代码量（Rust + bundle 资产管线 + 测试）。每个 WP 独立可合
> 并、可回滚；无引擎依赖的先行。

### 阶段 A（无引擎依赖，立即可做）

**WP-A1 `omb-compat` 垫片包** — ~100 行（bundle 资产 + 测试）
`plugins/omb-compat/`（新 pack）+ `mod.rs:909` sort key 5。
验收口径：target-ecosys2 阶段 3 的 28 个 FAIL-CONSISTENT 插件 source 转绿。

**WP-A2 source pack 允许 `.sh` entry + 精选生态 packs** — ~300 行
`mod.rs:2768`（扩展名允许集 + `shell:source` 仍强制）、`mod.rs:903-920`
（排序/默认）、vendor 管线（commit 固定 + LF 规范化 + LICENSE +
`index.toml` 对账 `mod.rs:2520` 扩展）。

**WP-A3 符号冲突检测** — ~200 行
新 `detect_symbol_conflicts`（`mod.rs` 近 3512）+ `conflicts.toml` 校验 +
`niu plugin enable` 告警/`--force`（`main.rs` enable 路径）。

**WP-A4 完成层探测与 CLI** — ~350 行
新 `completion/bash_completion.rs`（probe/install/uninstall/status，registry
复用 `external.rs` 结构）+ `main.rs:164-170` 分发 + rc source 行生成
（带引擎就绪门控）。激活默认关闭直至引擎就绪。

### 阶段 B（bundle/主题资产，#148 修复合入前可做，激活靠 canary 门控）

**WP-B1 vendor oh-my-bash 主题 + 布局扩展** — ~250 行（不含资产本体）
`BundleLayoutToml`（`mod.rs:378-392`）+ `omb_theme_entries()` 清单加载 +
`bundle_asset_path`（`mod.rs:3346`）映射 + `theme.rs:53` 解析链插入 OMB 层
+ `OMB_THEME` 映射与 `native:` 前缀 + canary（`niu theme canary <name>`
子命令）。
依赖：**激活**依赖 rubash#148（OPEN，在途）；vendor/清单/解析链无依赖。

**WP-B2 主题全量画廊（setup 主题节）** — ~350 行
`theme_gallery_entries()` + `omb_theme_preview()`（子进程缓存）+
`custom_flow` 主题块替换（`setup_wizard.rs:939-1066`）+ 徽标/i18n
（`zh()` `setup_wizard.rs:1989`）。原生部分即时可用；OMB 预览随 #148 点亮。

### 阶段 C（setup 重构 + doctor）

**WP-C1 分节向导** — ~500 行（重构为主）
`run_wizard_inner`（`setup_wizard.rs:488`）分节化 + 节目录菜单 +
`--section` 参数（`main.rs:164`）+ `interactive_multi_choice`。预设快车道
与非交互路径行为不变（回归保护：现有 wizard 测试
`setup_wizard.rs:2190` 起的用例全绿）。

**WP-C2 setup journal + undo** — ~200 行
`write_rc_and_mark_done`（`setup_wizard.rs:1182`）journal 写入 + 完成界面
undo 命令；复用 `apply_plugin_bundle_rollback`（`mod.rs:2426`）。

**WP-C3 doctor 生态体检** — ~200 行
`doctor.rs:17` 新行（§7 表）；复用 WP-A4/B1 的 probe/canary/冲突检测。

### 阶段 D（引擎依赖，排在引擎修复之后）

**WP-D1 引擎函数补全桥** — ~300 行
`BashScriptCompletionPlugin`（`completion/` 新文件）+ `completer.rs:62-90`
装配序 + 引擎 complete 注册表查询桥（rubash 侧 API 配合）。
依赖：引擎桶#6（compgen -F 执行函数）、桶#8（COMP_WORDS/cword）、桶#4/#5
（`_filedir` 载波）。

**WP-D2 单脚本接入点** — ~150 行
`niu completion register <cmd>` + kubectl/git 挂载（§4.3 表）。
依赖：git 主选路径依赖引擎 A-2（zsh `${(...)` 解析，target-ecosys2
Issue 1）；kubectl 依赖 WP-D1。

**WP-D3 bash-completion 激活开关** — ~80 行
WP-A4 的门控翻转：引擎桶#1（`bash_completion:188`/`ssh.bash:457` 引号
家族）修复后默认写 rc source 行，doctor 从 "not ready" 转 "ready"。

### 引擎侧（rubash 仓库）依赖清单

| issue/桶 | 状态 | 门控的 WP |
|---|---|---|
| rubash#133 `complete -D` | **CLOSED（已修）** | 前提已满足（WP-A4 设计基础） |
| rubash#148 `eval arr=(glob)` nullglob 路径展开 | **OPEN（在途）** | WP-B1 激活、WP-B2 OMB 预览 |
| 桶#1 `\'` 引号家族（bash_completion:188、ssh.bash:457） | 待修/待建 issue | WP-D3（bash-completion 整体 source） |
| 桶#4/#5 `_filedir` CTLESC 崩溃/字面泄漏 | 待修 | WP-D1 |
| 桶#6 `compgen -F` 不执行函数 | 待修 | WP-D1 |
| 桶#8 补全函数内 COMP_WORDS/cword 空 | 待修 | WP-D1 |
| A-2 zsh `${(...)` 解析（git-completion.bash P0） | 待修 | WP-D2 git 主选 |
| C-2 `-i` 时 `$-` 缺 `i/s` | 待核实（真交互路径） | WP-B1 主题门（`case $-` 判定） |

## 9. 许可证与合规

- **oh-my-bash / bash-it：MIT** — 可进官方 bundle，逐目录带 LICENSE 与
  版权头（主题文件多自带 MIT 头，vendor 管线不得剥离）。
- **bash-completion：GPL-2.0-or-later** — 只走"用户侧获取 + niu 出 MIT
  适配器"通道（`niu completion install` 克隆到用户目录）；niubash 仓库与
  官方 bundle **零 vendor**。doctor/setup 文案注明来源与许可。
- **测量纪律继承**（target-ecosys2 §固化建议 6）：vendor 管线强制
  `core.autocrlf=false` 检出 + LF 规范化（OMB 无 .gitattributes）；
  WSL 基线一律裁剪 PATH 后再比对。

## 10. 风险与未决

1. **OMB 主题加载深度耦合 #148**：不修则主题层只剩 vendor+画廊（预览灰显）
   ——按 canary 门控分阶段交付，不阻塞其余 WP。
2. **垫片 `_omb_module_require` 的 no-op 语义**：对自包含插件成立（验收结
   论），但若某插件真的依赖 lib 模块函数，垫片会静默缺函数。验收冒烟矩阵
   （88 PASS 侧）作为回归基线；`niu plugin doctor` 对 sourced pack 增加
   "加载后 undefined function 引用"抽检（后续项，不在本期 WP 内）。
3. **主题预览子进程成本**：82 主题逐个子 shell 预览，单次 ~200-500ms；靠
   高亮触发 + 会话内缓存控制在可感知范围内；若仍慢，退化为清单文件里的
   静态 PS1 摘录（构建期采集）。
4. **`niu completion install` 依赖 git**：与 `niu plugin add` 同前提
   （`external.rs:159-167` 已按 git 存在处理），无 git 时给出手工路径指引。
5. **scoop/winget 落点探测的稳定性**：非官方安装通道，路径漂移风险高；
   定位为 best-effort（探测顺序第 4 位），status 输出始终显示最终命中的
   origin。

## 9. 引擎依赖门控——状态更新（2026-09-26 深夜，captain 复核）

本文档第 8 节门控表按生态验收时点的引擎状态撰写。当日修复轮之后：

| 引擎依赖 | 设计文档时点 | 当前状态 |
|---|---|---|
| rubash#133（complete -D/-E/-I） | 已修 | ✅ 已关（e63f2d43） |
| A-2 / zsh `${(M)...}`（D2 的 git P0） | OPEN | ✅ 已关（242509e1）——git-completion.bash 3557 行双侧 source rc 0 |
| 桶#1 引号家族（D3 的激活开关） | OPEN | ✅ 已关（5aed4af9）——bash-completion 全量加载 rc 0、注册 131=机制面齐平 |
| 桶#4/#5 _filedir panic/泄漏（D1） | OPEN | ✅ 已关（37124dca） |
| 桶#6 compgen -F/COMP_*（D1） | OPEN | ✅ 已关（7c3e27c3）——git 补全候选 3=3 |
| rubash#148（eval glob，B1/B2 主题激活） | OPEN | ⏳ 修复在途（Q4 车道） |
| 桶#8/OSTYPE 政策项（验收矩阵环境项） | 环境 | 📋 升格为 rubash#154（uname/arch 内置 + RUBASH_IDENTITY 人设，待 owner 拍板默认人设） |

即：**A1–A4、C1–C3、D2、D3 的引擎依赖已全部就绪**；唯一硬门控剩 #148（B1/B2 主题激活，canary 分阶段方案不受阻）；#154 决定 452 矩阵最后 5 个环境项的归零方式。

## 10. 身份人设披露条款（owner 拍板 2026-09-26 深夜，rubash#154）

默认人设 = MSYS2 兼容（生态分流依赖），但**必须显著披露**：`niu --help` 身份节、doctor 常显当前 persona（含 uname -s/OSTYPE 生效值与切换方法）、README"身份与兼容"小节、仓库根 SKILL.md（AI 消费方同样需要知道）。补全冲突的分层事实（已核实）：引擎裸启动 `complete -p` 为空——外部脚本注册进引擎表后按 GNU 后注册者胜语义天然接管；宿主原生补全为保底层，Tab 管线维持"引擎注册表优先、原生 CommandDef 兜底、`native:<name>` 显式可达"；实现验收须含"宿主原生不反向覆盖引擎注册"的顺序测试。
