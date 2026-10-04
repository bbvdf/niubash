# Niubash Self-Discovery Audit Plan (wt74/audit)

Owner directive (2026-10-04): 「自己发现 不要等我说了才发现 拍几十个agent也不为过」—
find it yourself, do not wait for the owner to find it; dozens of parallel
discovery agents are not excessive.

## Why this exists

Every bug the owner found after the 1.3.3 release was discoverable by
*systematically exercising the product the way the owner does*: #168 (theme
rebound on 2nd source), #169 (cursor mid-path on multi-line themes), #170
(fake theme preview), #171 (collections installed only themes), the missing
`niu plugin ui` verb, #167 (clock eats the first key), the 6-second source
wall. The golden journey gate (`scripts/journey/golden-journey.py`,
`docs/journey-gate.md`) covers ONE happy path (install → wizard → trust →
theme → terminals → battery). This plan enumerates everything the journey
does NOT walk: every verb, every answer path, every state transition, every
error a user can hit, every claim a doc makes — as parallel discovery lanes.

## Rules every lane obeys

1. **Discovery, not repair.** Audit lanes DO NOT change product code
   (`src/`, `crates/`, `build.rs`, `installer/`). A finding is a
   reproduction + evidence, and the fix is a separate lane. The only files a
   lane writes are its probe scripts, its findings doc, and raw artifacts.
2. **Baseline is the product, not memory.** Assertions come from what the
   product *promises* (its own `--help`, README, plugins-guide, quickref,
   doctor output) and from what a real user would see (ConPTY screen text,
   rc/spec/registry files on disk). Never assert internals.
3. **Sandboxed HOME always.** `USERPROFILE` **and** `HOME` both point into
   the lane's sandbox (USERPROFILE wins in this product — known pitfall,
   see journey-gate.md). Never touch the real `~/.niubash` or the real
   Windows Terminal settings.json (back up first if a probe must).
4. **ConPTY for interactive, bounded CLI otherwise.** Copy the
   `golden-journey.py` driver pattern (`Session` + settle/wake/confirm/
   anchor + `Verdict` with per-assertion records) for any interactive
   probe; plain `timeout`-bounded subprocess for CLI verbs. Every probe has
   a timeout; no unbounded waits.
5. **Artifacts:** raw per-probe output under
   `target/audit-results/<lane-id>/` (untracked); durable interpretation in
   `docs/audit/findings/<lane-id>.md` (tracked). One findings file per lane
   — never a shared file, so parallel lanes cannot clobber each other.
6. **Build before you measure.** Each lane builds its own
   `target/release/niu.exe` from its worktree (stale-binary discipline, same
   rule as the release journey job).
7. **Severity classification (label every finding):**
   - **P0** — data loss (rc/registry/spec/tree destruction without consent,
     backup loss), primary flow dead (fresh install cannot reach a working
     prompt), crash/panic visible to the user.
   - **P1** — wrong output or broken promise on a first-class flow: spurious
     errors on normal startup, theme not applied as picked, preview lies,
     sync installs/loses the wrong thing, a verb's `--help` describes a
     different behavior than it does, rc corruption on a documented flow.
   - **P2** — cosmetic/wording: doc drift, missing hint, unexplained error
     code, inconsistent usage text between two screens for the same verb.
8. **Commit discipline:** one commit per lane on its own branch
   (`wt74/audit-lNN`), no push, no merge to master. The branch point is
   `wt74/audit` (so the plan and the shared probe helpers are present).
9. **Report format (findings file header):** lane id, domain, commit,
   probes run / passed / failed, then one section per finding: severity,
   one-line title, exact repro (commands + sandbox paths), expected
   (the product's own promise), actual (raw output excerpt), suspected
   owner file (read-only attribution).
10. **Network honesty.** Probes that clone/fetch inherit the network's
    truth; a TLS/network failure is recorded as environment, not a finding,
    unless the *error message itself* is the finding (domain D11).

## Worked example — domain D06 verb-surface sweep (run by the orchestrator)

Method: enumerate every verb from `src/main.rs` dispatch (launcher words,
`setup|configure|font|doctor|plugin`, engine invocation route, REPL
commands), run each `--help`/bare/missing-arg/unknown-flag form in a
sandboxed HOME with `NIU_LANG=en`, bounded timeouts, and assert exit code +
output against the product's own usage text. Raw evidence:
`target/audit-results/verb-sweep/` (81 probes; sandbox preserved).

**Verified verb inventory** (dispatch `src/main.rs:208-215, 1332-1394`):

- launcher words: `-h/--help`, `-V/--version`, `-c` (+legacy `-c -l`),
  `-C/--repl-command`, `--completion-probe`, `--install-wt-profile`
  (`--set-default`, `--quiet`), `--self-update` (`--check`, `--dry-run`),
  `--internal-{yes,head,wc}`, engine route (`ShellInvocation`), `setup`,
  `configure`, `font`, `doctor`, `plugin`, `<script>`, stdin-script mode.
- plugin verbs: `discover`, `source|sources` (list/add/trust/remove/
  update/rollback/verify/sign), `recipe|recipes` (list/show/add),
  `distro|distros|collection|collections` (list/import/remove/apply),
  `mirror|mirrors` (list|show/set), `ui`, `add`, `list`, `enable`,
  `disable`, `trust`, `update`, `rollback`, `restore`, `sync`
  (`--prune/--bootstrap/--adopt`), `clean`; retired: `info search themes
  bundle doctor review use tool tools`.
- REPL commands: `self-update`, `update-niubash`, `exit`/`logout`,
  easter-egg commands (`crates/niubash-runtime/src/easter_eggs/`).

**Findings:**

- **V1 (P1) — wizard-written rc bootstrap PATH-shadows the running niu.**
  The minimal-preset rc (`setup_wizard.rs` write path) emits
  `command -v niu >/dev/null 2>&1 && niu plugin sync --bootstrap`. On this
  machine a stale **Niubash 1.2.5** (`C:/Users/Administrator/AppData/
  Local/Programs/Winuxsh/niu.exe`) is first on PATH; `command -v niu`
  finds it, and every fresh startup (REPL and `-C` alike) prints
  `niu: unknown plugin subcommand 'sync'` because 1.2.5 predates `sync`.
  Worse than the noise: whenever the PATH niu is a *different vintage that
  also has sync*, the reconcile runs under the wrong binary's semantics
  against the shared spec/registry. Controls verified: current exe first on
  PATH → silent; no niu on PATH → line skipped silently; stale first →
  error. `NIU_SHELL` (exported from `current_exe()` at `main.rs:120-125`)
  is already available inside the process — the rc line should be
  `"${NIU_SHELL:-niu}"`. Suspected owner: rc template in
  `setup_wizard.rs`; blast radius includes the README-promised `-C`
  "AI-agent friendly" surface (rc noise pollutes scripted output).
- **V2 (P2) — top-level `niu --help` omits six existing plugin verbs:**
  `ui`, `recipe`, `distro`, `mirror`, `source`, `rollback`
  (`print_usage`, `main.rs:1303-1311`; all present in the dispatch and in
  `niu plugin --help`).
- **V3 (P2) — `docs/plugins-quickref.md` verb table documents
  `sync [--prune] [--bootstrap]` — `--adopt` is missing** (exists in
  dispatch and in `niu plugin --help`).
- **V4 (P2) — `niu plugin source` usage documents `update <id>`** but bare
  `update` (update-all) is accepted and prints `(no sources installed)`;
  the top-level usage documents `update [<id>]`. The two usage screens
  disagree about the same verb.
- **V5 (P2) — `niu --self-update --check` network failure prints**
  `niu: WinHttpSendRequest failed with Windows error 12175: ` — empty
  reason after the colon, untranslated Windows error code, no remediation
  hint (offline/proxy). Behavior is honest (nonzero exit); the message is
  not actionable (domain D11 class).
- **Positive results:** `niu plugin ui` exists on master and degrades
  gracefully without a tty (rc=1 + full CLI verb list) — the released
  1.3.3 lacked the verb entirely, so this class is exactly what the audit
  guards; all nine retired verbs exit nonzero with honest retirement
  messages; doctor's "N/M critical checks passed" count was honest in a
  broken sandbox (1/3 with two ADVICE rows); `--version | head -1`
  closed-pipe policy holds (#140); `--install-wt-profile --bogus` errors
  in arg parsing before touching the host; `setup --preset no-such-preset`
  fails with the available-preset list.

The sweep script and per-probe artifacts live in
`target/audit-results/verb-sweep/sweep.sh` (untracked); lane L01 formalizes
it into a tracked, repeatable probe.

## Domain catalog

15 domains, 32 lanes. Each domain lists its surface inventory (from
source), the probe set, the core assertions, the severity lens, and the
lane count.

### D01 — Setup wizard (3 lanes)

Surface: `crates/niubash-runtime/src/setup_wizard.rs` (3,049 lines). Q1
theme gallery (trusted external sources only) → Q2.5 plugin collection
(only when ecosystem empty; minimal/recommended/full) → Q3 niu-git →
summary + Apply/Cancel gate → rc write + backup + journal; Esc fast-forward
("every remaining question silently takes [the default]", line 668); Cancel
at Apply ("Nothing was written"); non-interactive run applies `minimal`;
`niu setup --preset <name>`; `NIU_LANG` zh/en; MinTTY hint path; `configure`
alias; re-run (reconfigure) mode; Ctrl-C after Apply does NOT cancel.

- **L02 wizard fresh-install full walk (P0 lens):** ConPTY, empty sandbox:
  welcome block, env panel, each question's rendering, collection
  full/minimal/recommended × apply, real clone progress, trust question,
  gallery listing, theme pick, rc+spec+journal written, undo receipts,
  REPL alive. Assertions: every question reachable by digits AND arrows;
  every screen text the wizard itself promises appears; exit path leaves a
  sourcing-clean rc (zero syntax errors on `source`).
- **L03 wizard cancel/Esc/mid-flow (P1 lens):** Esc at every question
  (fast-forward semantics — which defaults get taken, and is the result a
  coherent rc?), Cancel at Apply (nothing written — verify mtime/content of
  rc, spec, journal), Ctrl-C mid-clone and post-Apply (the documented
  "does NOT cancel" claim), cancel at the niu-git question (documented
  early-return), re-run wizard over an existing config (Keep semantics,
  backup written, managed blocks not duplicated).
- **L04 presets/i18n/non-interactive (P1 lens):** `setup --preset
  minimal|recommended|full` × 2 runs each (idempotence: second run must
  not duplicate blocks or re-back-up spuriously), `--preset=bogus` (error
  lists available), `--preset` with unknown flags present, non-interactive
  `niu setup` (minimal + journal), `NIU_LANG=zh` and `en` full walks
  (no mixed-language screens, no mojibake), MinTTY-hint branch.

Severity lens: P0 = rc written unsourceable / backup lost / wizard cannot
finish; P1 = a documented answer path produces a different config than the
summary promised; P2 = wording/mixed language.

### D02 — Theme lifecycle (3 lanes)

Surface: wizard gallery (`setup_wizard.rs:330-430` — themes from trusted
external sources, same-name resolution rank), `ThemePick` Keep/External,
`external_theme_activation` guarded managed block, OSH_THEME channel,
`niu plugin enable/disable <theme>`, spec `theme = "..."` field,
backup/restore via `~/.niubash/backups`.

- **L05 gallery + preview truth (P0/P1 lens — #170 class):** ConPTY walk of
  the gallery with >1 trusted source: entries listed, live preview
  rendered for the highlighted row — assert the preview matches the theme
  that will actually be activated (name, colors, line count); same-name
  theme from two sources resolves to the rank winner; picking writes the
  documented OSH_THEME guard block; #168 rebind-on-2nd-source scenario
  (add source B with same theme name; does A's pick get rebound?) —
  document findings even though wt72 is fixing the rebind.
- **L06 theme switch/remove/restore state machine (P1 lens):** A→B→A
  switch cycles (no stale blocks, single OSH_THEME line, no orphan managed
  blocks), disable active theme (fallback prompt returns), remove the
  SOURCE owning the active theme (`niu plugin source remove`) — what does
  the next startup do (guarded block must fall back silently, not error),
  restore an rc backup over a newer config, journal vs rc agreement.
- **L07 theme rendering contract (P1 lens — #169 class):** for the 5 most
  common gallery themes: multi-line prompt cursor lands at end of the
  input line (ConPTY cursor assertion), right-align segments survive
  narrow/wide terminal widths, CJK-wide glyphs do not shift the cursor,
  clock repaint does not eat the first keystroke after idle (#167 —
  document state; wt69 owns the fix).

### D03 — Plugin lifecycle core verbs (3 lanes)

Surface: `add/list/enable/disable/trust/update/rollback/restore/clean/
sync/discover` (`main.rs:1332-1394`), spec reconciliation
(`plugins/sync.rs`, 1,223 lines), trust gate (`plugins/trust.rs`),
registry lockfile (`plugins/sources.rs`, 2,373 lines).

- **L08 add flows (P1 lens):** catalog id / `owner/repo` / git url / local
  path / `--path` adopt / `--id` / `--ref` / `--checksum` (good + wrong
  checksum) / recipe-id routing / already-declared error / failed-install
  honesty (`add` must fail when the row failed) / fetch-gate notice
  wording. Network-dependent probes repeated against a local bare-repo
  fixture where possible (offline determinism).
- **L09 enable/disable/trust + managed blocks (P1 lens):** trust review
  output truth (origin/version/checksum verified), trust-then-activate,
  enable single asset vs whole source (wild `<id>/<file>.bash`), disable
  removes the guarded line only (no rc collateral), enable on untrusted
  (must refuse), degraded tree (delete files under a source → `list`
  degraded + repair hint → `restore` rebuilds exact pin).
- **L10 sync/update/rollback/restore/clean (P1 lens):** spec↔machine
  reconcile (install declared, unchanged, awaiting-trust rows), `--prune`
  deletes undeclared (and rc block removed cleanly), `--adopt` snapshots
  live state, `--bootstrap` silent-when-clean, update moves pin + rollback
  returns it (tree byte-identical? checksum verify), `clean` removes
  staging/orphans, undeclared-hints wording, startup reconcile under a
  spec whose origin is unreachable (failure surface at boot — wt71 owns
  the slow-source fix; this lane documents the failure shape).

### D04 — Plugin ecosystem sub-surfaces (3 lanes)

Surface: `plugin source` protocol (list/add/trust/remove/update/rollback/
verify/sign — `plugins/sources.rs`), recipes (`plugins/recipes.rs`, 5
categories: manager/theme/plugin/alias/completion — #171 class),
collections (`plugins/distros.rs`: list/import/remove/apply), mirrors
(`plugins/mirrors.rs`), `plugin ui` interactive menu
(`plugins/ui.rs`, 614 lines).

- **L11 source protocol + sign/verify (P1 lens):** full protocol per verb;
  verify detects a mutated tree (tamper a file → MISMATCH), sign pins
  trust, update after sign re-gates until re-signed (documented policy),
  remove uninstalls tree + record (no orphans), degraded trust refusal.
- **L12 recipes all 5 classes + entry validation (P1 lens — #171 class):**
  for every recipe in `niu plugin recipe list`: `show` fields consistent
  with `add` behavior; every git-driver recipe's `entry` names a file that
  actually exists in the upstream tree at the pinned ref (the
  `bash-preexec` seed bug class); download-driver recipes print a
  package-manager recommendation and download nothing (retraction
  compliance); `add` idempotence; state column (`not installed` /
  `manager: ...` / `info-only`) truthful after install.
- **L13 collections + mirrors + ui menu (P1 lens):** all three built-in
  collections apply end-to-end (#171: do they install more than themes?);
  partial-failure semantics (one bad entry → collected failures named,
  rest landed, nonzero exit); import from local dir + from git;
  remove imported; mirrors `set <url>` (insteadOf rewrite actually
  applied to a git fetch — use a local mirror fixture), `set none`,
  corrupt mirrors.toml degrade path; `plugin ui` ConPTY walk: sections by
  state, verb dispatch through the same runtime calls, Esc backs a level,
  Quit exits, empty sections open with explanation.

### D05 — Spec hand-editing (2 lanes)

Surface: `~/.niubash/plugins.toml` (`plugins/spec.rs`, schema
`niubash:plugin-spec@0.1.0`; fields target/id/kind/ref/enable/theme).

- **L14 spec validity matrix (P1 lens):** valid minimal spec; unknown
  keys; wrong schema string; duplicate entries (same target, same id,
  target+id conflicts); entry missing `target`; `kind` pin that stops
  matching (documented refusal); comments preserved across verb edits
  (does `niu plugin add` rewrite the file and eat the user's comments?);
  CRLF spec file; BOM.
- **L15 spec error surface at startup (P0 lens):** broken TOML in the spec
  at boot (`--bootstrap` must not crash the shell or spam); spec
  referencing a missing local path at boot; hand-edited enable list with a
  nonexistent asset at boot — every case: startup completes, prompt alive,
  error actionable, rc not corrupted.

### D06 — Verb surface + help truthness (1 lane)

- **L01 formalize the verb sweep (P1/P2 lens):** promote the worked
  example into `scripts/audit/l01-verb-sweep/` (tracked): every launcher
  word × (`--help`, bare, missing-arg, unknown-flag); plugin verbs × the
  same; engine-route boundary sample (`--bogus` → rc 2 `bash:` usage
  block; `--norc -c`; `-i -c`); usage-text-vs-dispatch diff (parse each
  usage screen's verb list and diff against the dispatch table);
  quickref/plugins-guide verb tables diff against dispatch. Findings V2-V4
  re-verified; V1 regression probe (stale-PATH rc noise) included.

### D07 — rc lifecycle (2 lanes)

Surface: rc write/backup (`write_rc_and_mark_done`, backups dir,
setup-journal), marker-delimited managed blocks (`>>> niu source ...
(managed by niu plugin enable/disable) >>>`), foreign rc content,
defaults-as-floor claims (README floor knobs `NIU_PROMPT_CWD_STYLE`,
`NIU_COMPLETION_STYLE`), winuxshrc one-time migration, `NIU_ENV`/`BASH_ENV`.

- **L16 rc backup/restore + managed blocks (P0 lens):** backup created and
  never overwritten across two wizard runs; restore round-trip; enable/
  disable edit only inside markers (foreign content between markers?
  outside markers? duplicated marker blocks? hand-deleted markers?);
  rc missing but journal present; rc present but marker-less (hand-rolled
  rc + plugin enable — where do blocks land?).
- **L17 foreign rc + floor knobs + env files (P1 lens):** rc with
  user content before/after managed blocks (ordering guarantees), foreign
  PS1 in rc (documented discard rule, unixwin/niubash#117), floor knobs
  actually shape the built-in prompt but are overridden by an external
  theme (README:127-128 claim), `NIU_ENV` vs `BASH_ENV` precedence
  (documented), winuxshrc → niubashrc migration once-only claim
  (`--help:1315`), rc sourcing wall time sanity (feeds D09).

### D08 — Interactive rendering (2 lanes)

Surface: `prompt.rs`, `prompt_segments.rs`, `interactive_menu.rs`,
reedline integration in `repl.rs`, `terminal.rs`.

- **L18 prompt geometry (P1 lens — #169 class):** ConPTY cursor-position
  assertions per prompt shape: multi-line themes (cursor at end of last
  line), right-aligned segments at widths 60/80/120/200, CJK directory
  names and CJK input (wide-char math), very long cwd (collapse rules),
  wrapping at column edge, terminal resize mid-session (SIGEVENT via
  ConPTY buffer resize; prompt redraw state).
- **L19 menus + editing surface (P2/P1 lens):** interactive_choice menus
  (digits/arrows/Esc/Enter semantics consistent across wizard, plugin ui,
  font menu), typeahead guard (keys typed while menus draw are not
  dropped — the wt61 class), completion probe ↔ real REPL completion
  agreement, syntax highlighting + autosuggest knobs
  (`NIU_AUTOSUGGEST_*`, `NIU_HIGHLIGHT_*`) visible effect vs no-effect.

### D09 — Performance sentinels (2 lanes)

Surface: `startup_trace.rs`, `Shell::new`, rc bootstrap, `sync --bootstrap`
in rc, completion/autosuggest latency.

- **L20 startup/source wall (P1 lens — 6s source class):** assert budgets,
  not eyeballs: cold `niu -c 'exit'` (zero-load claim: README:211), `niu -C`
  with rc, `source ~/.niubashrc` wall time with 1/3/5 sources installed
  (local fixtures, no network), `--bootstrap` overhead when clean vs
  degraded; emit a numeric table; budget from the 1.3.3 regression (6s
  source): assert < 2s and report actuals.
- **L21 prompt latency + first-keystroke (P1 lens — #167 class):** measure
  time from Enter to next prompt ready (themed, clock segment on), keys-
  to-echo latency after idle 1s/3s/10s (does the clock repaint eat the
  first byte? document with ConPTY timing), completion popup latency on
  10k-file directory, autosuggest keystroke overhead.

### D10 — Upgrade/migration (2 lanes)

Surface: installer (`installer/`), self-update, rc migration claims,
registry/spec vintage skew, `.setup-done` marker.

- **L22 1.2.x → 1.3.3 first run (P0 lens):** seed a sandbox with a 1.2.x
  rc + registry layout (old winuxsh dir on PATH — the V1 scenario), run
  1.3.3 first contact: wizard appears?, old rc backed up, no crash, no
  double-managed blocks, `winuxshrc`→`niubashrc` migration once-only
  (second run must not re-migrate), stale-PATH bootstrap noise (V1
  regression probe).
- **L23 self-update + installer claims (P1 lens):** `--self-update --check`
  output shape online/offline (V5), `--dry-run` downloads without running,
  REPL `self-update` / `update-niubash` spawn path, `maybe_print_update_hint`
  (when does the hint fire? does it ever lie about currency?), installer
  payload validation refusal path (`validate_installer_payload` — feed a
  garbage file via the dry-run handoff point, document the error).

### D11 — Error surface (2 lanes)

Surface: every user-reachable error string in the launcher/runtime
(`anyhow::bail!`, `eprintln!` diagnostics in `main.rs`, plugins/*, doctor,
fonts).

- **L24 error-message census — plugins (P2/P1 lens):** enumerate every
  bail/error path in `plugins/*.rs` from source; for each: trigger it in
  the sandbox; assert it names the object, the repair verb (health-style
  "name the repair"), and no lie (suggests a verb that exists); the
  self-update 12175 empty-reason class (V5); unknown-flag errors name the
  flag.
- **L25 error-message census — wizard/doctor/font/REPL (P2 lens):** same
  method for wizard questions (invalid input at each question), doctor
  rows (does each ADVICE name a working command? run the suggested command
  and see it succeed), font command on machines without fonts (honest
  recommendations), REPL errors (`niu: cho: command not found` first-byte
  eat class — document), engine-route usage errors under the `bash:`
  name.

### D12 — Docs vs reality (2 lanes)

Surface: README.md, README-zh.md, docs/plugins-guide.md,
docs/plugins-quickref.md, `.niubashrc.example`, PRIVACY.md, `niu --help`,
`niu plugin --help`, doctor output claims.

- **L26 README/quickref command census (P2/P1 lens):** extract every
  command line from README (both languages), quickref verb table,
  plugins-guide; run each in a sandbox; diff documented flags vs accepted
  flags (V3 class); documented env vars (`NIU_*` in README + config.rs)
  each actually changes behavior; documented exit codes match.
- **L27 claims audit (P1 lens):** prose promises tested one by one:
  "untrusted until trusted", "no re-downloads on new terminal" (J4),
  "byte-faithful manual sourcing" (plugins-guide guarded-line claim —
  diff against real `source` under WSL GNU bash 5.3.0 for error parity),
  PRIVACY.md claims vs actual network calls observed, `.niubashrc.example`
  lines all valid, `--help` Configuration/Environment sections true
  (winuxshrc migration claim, NIU_ENV precedence claim).

### D13 — REPL + engine boundary + easter eggs (2 lanes)

Surface: `repl.rs` (1,962 lines) REPL loop, `exit`/`logout`, egg commands
(`easter_eggs/`: about, cow, dino, matrix, party, snake, tic, typing),
history modes, `--completion-probe`, `-C` semantics.

- **L28 REPL command surface (P2 lens):** `-C` runs rc + precmd hooks +
  one line (documented order), `exit`/`logout` codes, eggs all launch and
  exit cleanly (no terminal state left broken — cursor/VT restored,
  `panic_restore` contract), Ctrl-C at prompt/child/mid-egg, Ctrl-D
  semantics, multi-line input continuation in REPL vs `-c`.
- **L29 history + completion knobs (P2 lens):** `NIU_HISTORY_MODE`
  shared/session/private actual behavior (two sessions share?), history
  persistence across restart, `--completion-probe` outputs match REPL
  Tab behavior for the same line/cursor (sample 20 lines: `niu `, `niu
  plugin `, partial words, after `|`).

### D14 — Trust/security surface (2 lanes)

Surface: trust gate (`plugins/trust.rs`), asset visibility rules
("assets hidden until trust"), signature tier, fetch-gate notices, tagged
wild candidates.

- **L30 trust boundary probes (P0 lens):** untrusted source's assets never
  appear in `list` output/never sourced at boot (spec `enable` present +
  untrusted → startup must not source); trust review shows checksum
  verified/MISMATCH truth; `sign` tier re-gate on update (documented);
  tagged candidates (`installer/test-like`) never auto-enabled; rc managed
  block existence guard (`[ -r ... ]`) actually prevents sourcing a
  deleted tree (delete tree → new terminal: no error, fallback prompt).
- **L31 tamper + downgrade paths (P0 lens):** modify a trusted tree then
  boot (behavior? silent sourcing of tampered code would be P0),
  `verify` after tamper, rollback after tamper, `--checksum` add with
  wrong digest refuses, registry record hand-edit (version/commit fields)
  → verbs degrade honestly or refuse.

### D15 — Windows Terminal profile + host touches (1 lane, Windows-host care)

- **L32 wt-profile + icons (P1 lens, WITH backup):** back up real
  settings.json first; `--install-wt-profile` (idempotence: run twice →
  one profile, GUID stable), `--set-default`, `--quiet` suppresses output,
  icon path resolution (assets present/absent), profile `commandline`
  tracks the running exe; bogus flag errors before any write (verified);
  on completion, restore backup if the probe only needed the parse path.

## Dispatch waves

- **Wave 1 (day 1, highest value per hour):** L01 (formalize sweep —
  already 70% done), L02 (wizard fresh walk), L08 (plugin add), L16 (rc
  backup/blocks), L22 (1.2.x migration + V1 regression), L05 (gallery
  truth). These cover every P0-class surface the owner actually hits.
- **Wave 2:** L03, L06, L09, L10, L14, L15, L30 (state machines + trust
  gate).
- **Wave 3:** L07, L12, L13, L18, L20, L24, L26 (rendering, recipes,
  collections/ui, perf sentinels, error/docs census).
- **Wave 4:** L04, L11, L17, L19, L21, L23, L25, L27, L28, L29, L31, L32.

Findings from Wave 1 that are P0/P1 spawn fix-lanes (not part of this
manifest); the audit lanes re-verify fixes after they land.

## In-flight collision avoidance

Audit lanes are read-only on product code, so the only shared mutable
surface is the repo tree itself. Hard rules: probes live in
`scripts/audit/<lane-id>/`, findings in `docs/audit/findings/<lane-id>.md`,
artifacts in `target/audit-results/<lane-id>/`. Per-lane do-not-touch
lists (in-flight hot zones from wt69/71/72/73/74) are pinned in
`lanes/manifest.json`.
