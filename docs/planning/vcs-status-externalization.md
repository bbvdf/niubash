---
tags: [niubash, architecture, prompt, vcs, plugins, externalization]
created: 2026-09-21
status: proposed
---

# VCS Status Externalization (git included)

> Context: niubash hardcodes git status into the host (`git_status.rs`
> daemon + native `{git}` prompt segment + `GitBackend` wizard question).
> Every other mainstream shell treats VCS status as external (posh-git,
> zsh plugins, starship, oh-my-posh); fish is the one counterexample and is
> criticized for it. The jj feature request (unixwin/oh-my-niu#8) settled
> the direction: jj support ships as an external plugin, zero host code.
> This document applies the same law retroactively to git itself.
>
> Origin discussion (2026-09-21): the two classic reasons for built-in git
> no longer hold —
> 1. *Performance* ("external means forking per prompt"): solved by mtime
>    caching (below). Steady state does zero subprocesses.
> 2. *Out-of-box experience*: oh-my-niu is bundled and enabled by default;
>    moving the segment into its `git` pack changes nothing visible.

## Target architecture

| Layer | Owns |
|---|---|
| niubash host | the prompt **engine** only: template `{cwd} {user} {host} {time} {status} {command_execution_time} {newline} {prompt_char}`, themes, right prompt, rendering. No VCS knowledge, no `git` binary calls, no daemon. |
| oh-my-niu `git` pack (bundle, default-on) | the `{git}` segment: PS1/PROMPT_COMMAND surface or pack-segment hook, `git status --porcelain --branch --ahead-behind` parsing, colors, ahead/behind/dirty glyphs. |
| external `jj` plugin (see oh-my-niu#8) | `{jj}` segment + jj completions. |
| starship modes | unchanged (segment/full takeover stay as-is). |

## The mtime cache pattern (the load-bearing trick)

A pack-provided segment function must not fork on every prompt. Pattern,
same one the jj plugin will use:

1. `test -d .git` (or `.jj`) — no repo, return instantly.
2. `stat` the index/head (or `.jj/repo/` oplog) files and compare mtimes
   against a per-repo cached key.
3. Unchanged → print the cached segment string.
4. Changed → run the VCS once, render, cache.

Steady-state cost: a builtin `stat` per prompt (~0). Dynamic cost: one
VCS invocation exactly when the repo actually changed. This matches the
native daemon's steady state without any host machinery.

## Migration plan (phased, default experience never regresses)

1. **Audit the composition path.** `prompt-core` pack + `prompt.rs` /
   `prompt_segments.rs`: how the `{git}` token receives data today
   (`current_git_status()`), and what hook surface packs already have
   (precmd hooks, PS1/PROMPT_COMMAND, `NIU_*` variables). Decide the
   pack-side segment API: prefer pure shell via PROMPT_COMMAND so no new
   host extension point is needed; only add a generic (VCS-agnostic)
   segment variable to the template engine if composition requires it.
2. **Build the pack segment.** Implement the git segment in the oh-my-niu
   `git` pack with the mtime cache; cover: branch, detached HEAD,
   ahead/behind, dirty counts, conflict/rebase states, indented themes'
   color mapping (mirror `GitPromptSymbols`).
3. **Switch the default.** Setup wizard stops asking `GitBackend`
   (native/segment/full) — presets gain a single "git segment: bundle"
   default; starship full remains an explicit choice. Generated rc for
   new setups uses the pack segment.
4. **Deprecate host machinery.** Keep `GitBackend::Native` one release as
   fallback for existing rc files (read-only compat), then delete:
   `git_status.rs` daemon, native `{git}` rendering, the wizard question.
   Expect a meaningful host code/binary-size reduction.
5. **jj plugin** (parallel, oh-my-niu#8): same pattern, own repo,
   `niu plugin add` install.

## Verification

- Prompt latency A/B: cold start and 20 consecutive prompt draws in a
  warm git repo (host native vs pack+mtime-cache) — steady state must
  show zero `git`/`jj` spawns (verify with a spawn-counting wrapper).
- Out-of-box: fresh sandbox HOME, `niu setup` non-interactive, cd into a
  git repo → branch/dirty visible in prompt, byte-identical glyphs to the
  native segment for the common states.
- Standard gates: `cargo fmt --check`, `cargo build --locked`,
  `cargo test --workspace --locked` (419 as of 2026-09-21),
  `sh scripts/check-rename-clean.sh`, GNU upstream gate
  baseline-compare (prompt is interactive-only; suite should be
  unaffected), K: smoke matrix.
- Existing rc compat: an rc written by the previous wizard must still
  render (native fallback) until the deprecation release.

## Non-goals

- No VCS-specific code in the host after migration — that is the point.
- Starship integration paths are untouched.
- The `{git}` template token name stays (themes keep working); only its
  data source moves.
