# Changelog

All notable changes to Niubash are documented in this file.

## [Unreleased]

### Added

- WinuxCmd applet completions are compiled into the shell (niubash#172 L1):
  `grep --col<Tab>` offers `--color`/`--colour` with descriptions, `ls --color`
  offers `always`/`auto`/`never`, `wpm <Tab>` lists its 20 subcommands, and
  every one of the 178 WinuxCmd 1.1.5 applets (including `[`) carries its real
  flag set with no plugin enabled. Definitions are generated from
  `winuxcmd --help` transcripts by `scripts/generate-winuxcmd-completions.py`,
  embedded from `crates/niubash-runtime/assets/completions/winuxcmd/`, and
  load as the base layer: bundle/pack and user-dir TOMLs still override per
  command. Generator fixes en route: bracket command names (`[`), headerless
  help (`top`), multi-alias specs (`pr -F, -f`), `EXIT STATUS:`/footer
  pollution of descriptions, optional-value flags (`--color[=WHEN]`) no longer
  swallow filename completion, path-valued flags decided by spec hints instead
  of description prose, and wpm `Commands:` parsing. Golden coverage:
  `crates/niubash-runtime/tests/winuxcmd_completions.rs` plus
  `scripts/test-winuxcmd-completions.py` (corpus replay must be
  byte-identical).

## [1.3.4] - 2026-10-05

### Fixed (engine rubash 1.3.4)

- Theme choices survive `source` and new terminals: the wizard/gallery pick
  is pinned into the spec, sync reconciles same-name claims toward the rc
  (the "activated/deactivated" revert is gone), and the defaults-as-floor
  never resurrects a framework over a claimed theme.
- Multi-line theme prompts place the editing cursor at end-of-input-line:
  the theme's right-align cursor surgery is served through the line
  editor's own right-prompt channel (escape-excluded width math).
- A 6-second first prompt after picking `full` is gone: startup fetches run
  under a hard 3s budget (kill-on-close job object, credential-prompt
  guard) and memoize; in-sync machines never fetch at all.
- `tr -d/-s/-c` in pipelines actually runs (the translate fast path had
  read `-d` as a character set); `for f in "${arr[@]:-}"` yields
  per-element words; `/usr/bin/seq`-style file operands resolve through
  `.exe` existence (directory forms unchanged); parser throughput +13%
  (nvm -n 18.0x -> 15.6x vs GNU).

### Fixed (product, found by the audit lanes and the golden journey)

- `plugin add --checksum` is honored through add and adopt; failed adds
  roll back their spec entry and legacy strands are pruned; enable/disable
  surface rc-write failures and collapse duplicate managed blocks.
- The wizard rc's bootstrap line references the running binary
  (`${NIU_SHELL:-niu}`), so a stale niu on PATH can no longer hijack it;
  doctor warns when the PATH-resolved niu differs from the running exe.
- `--help`/usage/docs truth pass (ui/recipe/distro/mirror/source/rollback
  verbs listed; sync --adopt documented; WinHttp 12175 gets a reason).

### Added

- Theme gallery live prompt preview (isolated render, 1.5s bound) in the
  wizard and `niu plugin ui` (verb now registered) - highlight moves, the
  pane below renders that theme's actual prompt.
- Collections install curated independent recipes (complete-alias,
  fzf-git.sh, bash-sensible, git-flow-completion - upstream-audited).
- 178/178 winuxcmd applet completions embedded (zero-config Tab completion
  for every bundled command; user TOMLs keep highest priority).
- The golden user journey is a required release gate: install -> wizard ->
  trust -> theme -> new terminals -> daily battery, plus persistence,
  wizard-rerun, spec hand-editing and remove-active-source phases
  (`--phases all`), all driven from the tag's own fresh build.
- Per-asset timing budgets (437 themes/plugins/completions, zero
  sampling) wired as a second release gate; budgets ratchet down only.

## [1.3.3] - 2026-10-04

### Fixed (engine rubash 1.3.3)

- Keystrokes typed while the prompt rebuilds are no longer eaten: external
  children spawned by PROMPT_COMMAND (the theme clock's date/git/awk probes)
  used to consume the first pending console input record - a human typing
  right after a prompt refresh lost their first key (`echo` -> `cho`). A
  typeahead guard now sweeps pending input during prompt machinery and
  reinjects it verbatim before the editor reads.
- `tr` with options in a pipeline actually runs (`tr -d b` was silently
  passed through - the translate fast path had read `-d` as a character
  set).
- `for f in "${arr[@]:-}"` yields per-element words like GNU (bash-it
  theme loading works).
- Parser throughput: nvm -n ~13% faster (backslash-continuation scan
  admission gate + binary-search line stamping), GNU ratio 18.0x -> 15.6x.

## [1.3.2] - 2026-10-04

### First three-platform release (engine rubash 1.3.2)

- **Linux (x86_64 + aarch64, glibc) and macOS (arm64 + x86_64) portable
  tarballs** join the Windows packages. Every platform's build job
  smoke-runs the binary it ships, on that OS, before any artifact is
  attached. Windows asset names and the self-update link logic are
  unchanged.

### Engine fixes

- Alias expansion no longer leaks into compound array assignments OR
  `[[ ]]` conditional / extglob pattern words - the oh-my-bash +
  bash-completion interactive corruption family is closed (the
  `bash_completion: line 1376` syntax error).
- Single-quoted words keep their integrity through pipeline and
  command-substitution stages (three stacked root causes fixed).
- `cat` applies all formatting options when reading a pipe (`cat -n`
  in a pipeline numbers lines).
- `cd` reaches external children (interactive reader no longer restores
  the process cwd per line).
- `-c` subshell fatal expansions exit 1 like GNU, not 127.
- `\u` renders the OS account name, env-independent.
- Virtual system-root arguments (`/usr/bin`, `/etc`, `/tmp`, ...) are
  resolved through the root map for every child class.
- Console-attached bare rubash renders the real PS1/PS2 channel instead
  of a placeholder REPL (GNU interactivity rule, error survival, exit
  codes).

### Product fixes (found by the new golden user journey gate)

- Same-name themes (powerline-multiline exists in both frameworks) now
  route to the owning framework's native loader and actually apply.
- The curated bash-preexec recipe entry matches the real upstream file;
  all curated entries are pinned against upstream roots.
- A failed collection apply journals and reports the failure honestly
  with the retry command.
- The golden user journey (fresh install -> wizard -> trust -> theme ->
  new terminals -> daily battery) now runs as a required release gate.

## [1.3.1] - 2026-10-03

### Fixed

- **Interactive arithmetic corruption** (engine rubash 1.3.1): aliases no
  longer expand inside compound array assignments. Under oh-my-bash, the
  convenience alias `1='cd -'` leaked into the literal
  `OMB_VERSINFO=(1 0 0 0 ...)` and corrupted it to
  `(_omb_directories_cd - 0 ...)`, killing version arithmetic with
  `-: arithmetic syntax error` in interactive sessions (any theme,
  powerbash10k included). GNU parity: assignment words are never
  alias-expanded (parse.y parse_compound_assignment keeps the whole
  `( ... )` in one ASSIGNMENT_WORD token).
- **Plugin sync state machine**: the startup "installed but not declared"
  nag now prints only when a spec exists (imperative mode is silent);
  `niu plugin sync --adopt` declares installed sources with a
  round-trip-stable snapshot (enablement + theme); the setup wizard ends
  spec-managed; `niu plugin add` adopts an already-installed source
  instead of erroring "remove it first".
- **No re-download loop**: the startup bootstrap memoizes failed installs
  (per origin+ref ledger); retries happen only on an explicit
  `niu plugin sync`. The bash-it fingerprint now matches the real
  upstream layout (its `lib/composure.bash` was removed upstream).
- `niu plugin source remove` also drops the spec declaration (no
  resurrection), and duplicate spec declarations of one source merge
  instead of flipping the rc block every sync.

## [1.3.0] - 2026-10-03

### Plugin system - the bash ecosystem, managed

- Declarative spec `~/.niubash/plugins.toml` (single source of truth) +
  `niu plugin sync` reconciliation, startup bootstrap, and a
  commit-pinned lockfile - the lazy.nvim spec/lock conventions.
- Any-plugin generality: `niu plugin add <owner/repo | url | path>`;
  source adapters for oh-my-bash, bash-it, bash-completion and bpkg;
  framework assets enable only through their native loaders (loader
  fidelity - no shims, no bypass paths).
- Recipe index (500+ curated rows across managers/themes/plugins/
  completions/prompts), collections/distros (`niu plugin distro
  import`), menu TUI (`niu plugin ui`), read-only `niu plugin discover`.
- Mirror pipeline: transport-layer git `insteadOf` rewriting
  (`niu plugin mirror set <url>`); lockfiles keep canonical URLs.
- Trust model: tree-checksum verification, explicit `niu plugin trust`,
  and a trust question in the setup wizard.

### Download retraction (BREAKING)

- niu no longer downloads anything except git clones: ureq, flate2,
  tar and zip are gone, the download module is deleted, and the binary
  shrinks ~16%. The smoke suite asserts a zero-network surface.
- Fonts become detection + recommendation (winget nerd-fonts packages,
  scoop, brew, nerdfonts.com).
- Executable tools become package-manager recommendations only: `wpm`
  first on Windows, native managers (apt/dnf/yum/brew) elsewhere.
  The `niu plugin tool` verbs are retired.

### Setup wizard - one-run out-of-box

- Plugin collection page (recommended = oh-my-bash + its default theme
  + completions; full adds the second framework and bash-preexec with
  fzf/starship as suggested installs).
- Post-install theme pick: after a collection applies, the wizard asks
  the trust question and then offers the freshly installed themes - a
  complete look in ONE run, no second `niu setup`.
- defaults-as-floor: enabled external frameworks own the prompt; the
  built-in default is the floor, never an override.

### Engine (rubash 1.3.0) and bundled components

- TMPDIR child boundary fixed: invented defaults stay shell-only and a
  genuinely exported TMPDIR crosses in Windows-native form - Bun-
  compiled TUIs (opencode etc.) launch again.
- Associative kvpair compound assignments, `:` in redirect filenames,
  command-substitution case-depth regions, physical-line diagnostics
  across continuations, and spawn diagnostics preserved verbatim.
- WinuxCmd 1.1.5 bundled: xargs stops its own option parsing at the
  utility name; mv/mktemp/cp/ls accept POSIX drive-form operands; yes
  dies with 141 on a broken pipe like GNU coreutils.

### Fixes / tests

- Unix cross-check compile break in the setup wizard (a baseless
  cfg gate); a one-character zh translation key mismatch.
- Release smoke suite now 17 legs, fully offline, including the
  one-run wizard journey under ConPTY.

## [1.2.5] - 2026-10-02

### Fixes

- **`$(case y in (b|case) ...)` family** (rubash #380, P0): the comsub
  body is a fresh command stream — its first word is at command position
  — and the esac keyword rule looks at the PREVIOUS token, not forward
  evidence. All paren-list keyword patterns now parse and run.

- **posix round-trip stickiness** (rubash #383): the set_posix_mode
  walk is ported — enable arms inherit_errexit and friends, disable
  resets only the two GNU resets (inherit_errexit stays sticky).

- **SIGPIPE-shaped lingerer status** (rubash #382): `yes | head -3;
  echo ${PIPESTATUS[0]}` prints 141 like GNU.

- **Fatal expansion inside $( ) under -c** (niubash #154): reports 1
  (EXECUTION_FAILURE), not 127.

- **Foreign Git Bash PS1 discarded** (niubash #117): a PS1 carrying
  `__git_ps1`/MSYS title escapes is unset before rc — the own theme
  renders, no more command-not-found per prompt.

- **Test suites rebuilt against GNU 5.3.0 probes** (rubash #373:
  88 reds -> 0; #374 first pass 174 -> ~119).

### Performance

- Pipeline-floor round 2 (rubash): __RUBASH_CURRENT_LINE single-writer
  + $_ equal-skip rebind — p-null -4.7%, p-f1 -5.7%.

- Engine bump: niu 1.2.5 builds against rubash 1.2.5 (c2f8e8a6).
## [1.2.4] - 2026-10-01

### Fixes

- **Distributable binaries no longer require VCRedist**: release builds
  now statically link the CRT (`-C target-feature=+crt-static`). niu.exe
  previously imported VCRUNTIME140.dll — not an OS component — so on a
  clean Windows without VCRedist 2015+ the loader failed with
  STATUS_DLL_NOT_FOUND (0xC0000135) before any shell code ran. This was
  the winget validation sandbox failure on winget-pkgs#437563 (and the
  same exit code had masked it behind the portable-tree issue in #150).
  Imports are now OS in-box DLLs only (verified with objdump).
## [1.2.3] - 2026-10-01

### Fixes

- **External-pipeline data loss** (#141, #155, P0): the Windows
  broken-pipe hard-kill window for non-final pipeline members armed
  unconditionally at call time, killing healthy producers mid-stream
  while their consumer was still reading (`seq 200000 | wc -l` returned
  ~17k with rc=0, drift per run). The window now arms only after the
  downstream member is observed to have exited, plus a 100ms natural-exit
  grace (rubash 2c781657). Verified: seq 5000000 x5 full count;
  `yes | head` lingerer termination intact.
- **`$()` children see pre-opened fds 3/4** (rubash #368): the nvm-exec
  `3>&1`/`1>&4` juggle protocol holds; fd-1 dup snapshots escape the
  substitution to the real stdout.
- **`${assoc[*]@A}` keeps the assignment body** (rubash #371);
  **case-pattern keywords stay inert** (rubash #372, issue308 residue
  green).
- Engine bump: niu 1.2.3 builds against rubash 1.2.3 (a6eb8451).
## [1.2.2] - 2026-10-01

### Fixes

- **Pipe data loss on external-to-external pipelines** (#141, #155): the
  external-command stdio planner and the captured-output drain were
  rewritten (rubash #370 family). On 1.2.1, `seq 200000 | wc -l` could
  return ~24k-33k lines (or empty) with rc=0 while the writer took EPIPE;
  compound bodies (`{ seq 200000; } | wc -l`) were unaffected. Engine
  release builds now pass 8/8 at 200000 with empty stderr.
- **bats-core self-suite hang** (rubash #364): an assignment value coming
  from a parameter-expansion result no longer re-executes `<(cmd)` text
  found in the EXPANDED value (GNU subst.c:11358-11381 semantics);
  bats_pipe.bats 155/155 TAP byte-identical to GNU.
- **Adjacent `$((...))` arithmetic substitutions mis-sliced** (rubash
  #376, P0): `echo "$((1+1)):$((2+2))"` now prints `2:4` — the whole-word
  admission uses a real paren-depth span scanner (GNU parse.y:3877
  parse_matched_pair) instead of pairing the first `$((` with the last
  `))`.
- **Quoted compound-assignment elements globbed** (rubash #369):
  `arr=("$x")` with `x='*'` stores the literal `*`; quoting state now
  survives transport to the element glob gate.
- **`exec N>&M` fds honored by external commands** (rubash #370):
  `exec 3>&2; helper >&3` lands on stderr for external children, not
  stdout.
- Engine bump: niu 1.2.2 builds against rubash 1.2.2 (98bc65ba).

## [1.2.1] - 2026-10-01

### Fixes

- **WinGet portable install could not start `niu`**: the manifest shipped the
  release zip with `InstallerType: zip` + `NestedInstallerType: portable`, and
  WinGet's portable shim only copies the single declared `niu.exe` into
  `%LOCALAPPDATA%\Microsoft\WinGet\Links`. The bundled `winuxcmd/usr/bin`
  tree next to the executable is left behind, so `niu.exe` started from the
  Links directory found no WinuxCmd and aborted during startup validation
  (winget-pkgs PR #437563, exit code 0xC0000135). The manifest now installs
  the Inno Setup `-setup.exe`, which lays down the full directory tree and
  registers the PATH entry.
- Engine bump: niu 1.2.1 builds against rubash 1.2.1.

## [1.2.0] - 2026-09-25

### Fixes

- **Drive-letter colons are preserved when niu splits the shell `PATH`**
  (`3c7b4b1`); PATH entries like `C:/tools/bin` no longer get mangled into
  `C` + `/tools/bin` during PATH processing
- GNU-aligned invocation surface for stdin scripts: fd0 handling and `-i`
  history flag match GNU bash behavior (`746b74d`, rubash-side fixes)
- Engine bump: niu 1.1.5 builds against rubash 1.2.0 (published to crates.io),
  which carries the `/dev/stdout` `/dev/stdin` `/dev/null` redirect semantics
  fixes (`8c0dded3`..`a38268f6`) and the gate suite now runs 86/86 green

## [1.1.3] - 2026-09-16

### Fixes

- **`ln -s` with `./` / `../` targets produced links that Explorer could not open**.
  winuxcmd stored the link text verbatim; NT only resolves reparse-point targets
  with backslash separators, so any forward slash (`./bds`, `../x`, `dir/file`)
  failed native resolution with "The filename, directory name, or volume label
  syntax is incorrect" (WinuxCmd #1101, fixed in v1.0.8, niubash #109)
- **`niu -lc 'cmd'` (and any bundled short option containing `-l`/`-i`) failed**
  with `-l: invalid option`; bundled short options now expand correctly
  (`-lc`, `-cl`, `-ilc`, `-ic`) (rubash, PR #112)
- `niu -c -l`, `niu -c` argument handling and `$(type -t)` stdout leak from
  the v1.1.2 issue batch (#106/#107/#108, rubash PR #111)

## [1.1.0] - 2026-09-12

### Highlights

One week of intensive work after v1.0.1: IDE-style completion menu, colorized
plugin CLI, NIU_ENV/BASH_ENV one-shot env files, startup-overhead fix, release
binary size reduction (LTO + panic=abort), and upstream rubash v1.1.0 with the
CTLESC `\x11` leak fix that resolves `NIU_BINDKEYS` parsing.

### Features

- **IDE-style completion menu** with descriptions; humanized plugin CLI views
  (`26610e7`, `a6c1fee`)
- **Colorized plugin CLI**: TTY-gated ANSI styling for plugin list/action
  feedback (`cb72d52`, `3ab1b42`)
- **NIU_ENV / BASH_ENV opt-in env files** for one-shot mode (#82, `ee1c410`)
- **Interactive shell easter eggs** (`30efd9a`)
- **Demo bundle** for plugin development (`f7d2943`)

### Fixes

- **Startup overhead**: skip framework hook dispatch when the runner is
  undefined (#80, `6fbee9f`)
- **CTLESC `\x11` leak** (upstream rubash v1.1.0): quoted assignment fast path
  leaked `\x11` into stored values, breaking `NIU_BINDKEYS="Ctrl+X:..."` parsing
  (`a313729b` in rubash)
- **Hook runner resilience**: hook runners survive user `set -eu` (`6ba7af0`)
- **REPL heredoc-body scanning**: fix completeness check for heredoc bodies
  spanning command-substitution boundaries (`6ba7af0`)
- **WinuxCmd auto-activation**: portable first-run activation with absolute-path
  activate script (`ca574ed`)
- **Panic hook**: best-effort console restore under `panic=abort` (`270d16b`)
- **Rename brand gate**: restore working version drift gate (`92e6724`)
- **$BASH path**: set `$BASH` to the running executable path (`0ea5513`)
- **Completion engine**: wire rubash completion engine, drop hardcoded builtin
  list (`316a8f6`)
- **Heredoc diagnostics** (upstream rubash v1.1.0): warning line numbers now
  use computed `warning_line` matching GNU `make_cmd.c:627` (`a313729b` in
  rubash; remaining gaps tracked in rubash issue #72)
- **Chinese path crash** (#84): `host_path_to_shell_path_with_root` panicked
  with `byte index is not a char boundary` when the shell root byte length
  fell inside a multi-byte character in the current directory path. Added
  `is_char_boundary` guard before slicing. Also fixed
  `longest_common_prefix` in the completion module which had the same class
  of bug when decrementing `prefix_len` through multi-byte characters.

### Build

- **Release binary size**: enable thin LTO + `panic=abort` (-35% size,
  `f94c48d`)
- **Warning suppression**: crate-level `#![allow]` for 11 legacy rubash warnings
  to unblock downstream CI (`a313729b` in rubash)

### Documentation

- Locale default decision for `${#var}` UTF-8 counting (`2f86b3e`)
- Trim redundant README sections; drop stale arm64/version claims (`ceb431e`)
- Add Built-ins & Fast Paths page (`1e35971`)
- Translate architecture page to English (`5a02b7b`)

### Dependencies

- Rubash upgraded from v1.0.0 to **v1.1.0**

## [1.0.1] - 2026-09-04

Initial stable release.
