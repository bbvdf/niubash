# Changelog

All notable changes to Niubash are documented in this file.

## [Unreleased]

### Fixes

- **Download driver refused verified Windows archives** (wt49/smokesweep,
  found by the 1.3.0 smoke suite): recipes declare extension-less bins
  (mason shape, `bins = ["fzf"]`) but Windows archives ship `fzf.exe`, so
  `niu plugin recipe add fzf` failed with "declares bin 'fzf' but it is
  not in the archive" after a successful sha256-pinned download. Bin
  verification now resolves `<bin>.exe` on Windows (exact name preferred),
  the tool registry records the resolved on-disk name, and a failed
  install no longer leaves `.staging/<id>.unpacked` residue behind.
  Audit ledger: `docs/download-surface-audit.md`.

### Tests

- **1.3.0 smoke suite** (wt49/smokesweep): `scripts/smoke-test-1.3.0.sh`
  runs the release checklist end-to-end (install chain, download channel,
  defaults-as-floor, setup preset, basics) against a sandboxed HOME with
  timeout guards, network legs skippable; `tests/smoke_1_3_0.rs` is the
  CI mirror with per-invocation deadlines and opt-in network legs
  (`NIU_SMOKE_NETWORK=1`).

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
