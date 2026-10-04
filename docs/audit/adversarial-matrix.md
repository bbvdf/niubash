# The Adversarial Test Matrix (lanes AM01–AM12)

Lane `wt88/advmatrix` (2026-10-02). Companion machine-readable manifest:
[`lanes/advmatrix.json`](../../lanes/advmatrix.json). Injection toolkit:
[`scripts/adversarial/`](../../scripts/adversarial/) (each tool demoed once
on a sandbox before this document was written — evidence inline and in
§3).

## 0. Why: the structural hole is not "more steps", it is the conditions

The owner's directive after the eighth owner-found bug in one day
(2026-10-04, verbatim):

> 开十个agent并行测试 不要限制测试手段 你的提示词和方法路子一定有严重问题
> ("Spin up ten agents testing in parallel. Do not restrict test methods.
> Your prompts and your approach must have serious problems.")

The owner is right about the approach, and the failure record says exactly
where: `journey-spec.md` already proved that the golden journey's shape —
**happy path, clean network, fresh state, one session at a time, a scrubbed
environment, one terminal geometry** — went green through live P0s
(#167 masked by the driver's Ctrl-U wake, #168 missed because a rebound
theme still renders). The P1–P10 phases widened the *timeline* (day-two,
day-ten state) but kept the same benign conditions. Every remaining axis of
hostility — the network that fights back, state that aged, users that
share a machine with themselves, disks that fill, terminals that resize,
paths with CJK characters in them — is still only probed where a journey
step happened to need it.

So this matrix is **condition-first, not timeline-first**: twelve lanes,
each of which takes the SAME product (wizard → sync → source → daily
battery) and changes ONE class of environmental conditions to hostile,
plus a final lane that stacks all of them. The unit of design is the
*injection mechanism* — each lane must name the concrete tool that makes
the condition reproducible offline, or it is not a lane, it is a wish.

Sibling docs stay authoritative where they overlap: journey-gate.md's
standing rules (real user shapes, known-fails labeled never waived, assert
what the USER sees) and journey-spec.md's P-phases (the timeline axis)
apply to every lane below. This document adds the axes they do not cover.

## 1. The failure record this matrix must have caught

Full records: `gh issue view N --repo unixwin/niubash`. One-line shapes,
with the lane that would have caught each FIRST:

| Issue | Shape in one line | First-catching lane |
| --- | --- | --- |
| #167 (P0) | themed prompt's per-second clock repaint eats the first typed byte (`echo`→`cho`); real humans lose keys | AM09 (first-key under slow/hung renders), AM05 (after resize), AM10 |
| #168 (P0) | sync's `activated/deactivated` materialization rewrite clobbers the user's picked theme across source/new terminals when the same theme name exists in two frameworks | AM03 (dual-framework collisions — primary), AM02 (aged state re-sync), AM04 (concurrent sync), AM10 |
| #169 (P0) | multi-line theme renders but the cursor sits mid-path, not at end of input — cursor column accounting vs rendered width | AM05 (cursor-snapshot matrix per width/resize — primary) |
| #170 | gallery "preview" is static text, not the theme's live prompt | AM05 (preview geometry under resize/NO_COLOR), AM03 (preview must show what the pick actually activates) |
| #171 | collections install only theme ecosystems; completion/plugin/alias recipes have no install path | AM12 (collection-apply integrity — primary) |
| #172 | no completions for the 100+ winuxcmd applets; L1/L2/L3 layering unplanned | AM12 (claimed completions must exist and match `--help` surface) |
| #173 (P1) | `plugin add --checksum` silently dropped — lockfile promises verification that never runs | AM12 (checksum tier under the kit), AM02 |
| #174 (P1) | failed `plugin add` persists the spec entry BEFORE fetching — every startup prints a deferred row forever, no verb can remove it | AM01 (add under reset storm — primary), AM06 (add under disk-full), AM09 (add under throttle), AM12 |
| #175 (P1) | enable/disable fake success: rc write failures swallowed (`let _ =`), disable removes only the first marker pair, duplicate blocks report in-sync | AM11 (rc-block surgery — primary), AM06 (locked rc during enable) |
| #176 (P2) | add/rc audit batch: CRLF rc rewritten whole-file to LF, hand lines inside managed blocks silently dropped, marker-approximate lines create unmanaged second blocks, plugin verbs never back up the rc, cwd-relative targets stored raw | AM11 (primary), AM07 (unicode/cwd-relative targets), AM02 |
| #163 | `-c`-mode subshell fatal expansion exits 127 (GNU: 1) | AM10 battery (engine-parity family — see §5 honesty note) |
| #164/#177 | `/usr/bin`-shaped args untranslated for winuxcmd applets; file-form args missing `.exe` after translation | AM10 battery (same family) |
| #165/#166 | `cat -n` as pipe read end drops all format options; bare `tr` in a pipe stage silently idles | AM10 battery (same family) |
| #157/#159 | setup-wizard rc generator emitted unclosed `${…//\` / missing backslash — every alias dead, syntax error every start | AM11 (rc-writer regression walk), AM08 (old rc shapes) |
| five TLS-burned release runs (2026-10-03) | github.com TLS resets burned five release journeys; product behavior under fetch failure (deferred ledger, bounded startup) untested | AM01 (primary — deterministic resets offline) |
| "6s source" (1.3.0) | bootstrap re-fetched the full clone on EVERY terminal when a fingerprint rejected | AM02 (aged state) + AM09 (throttled re-fetch churn) |
| wt80 known-fail | wizard undo receipt `niu plugin disable <theme>` fails when the theme exists in two installed sources | AM03 |
| #117 family | inherited Git Bash PS1 → `__git_ps1: command not found` every prompt (gate scrubs env; real terminals don't) | AM07 (env-poisoner `gitbash` preset) |
| #162 family | TUI child fails to launch under env/DLL path pollution | AM07 (`msys-parent`, `garbage-niu` presets) |

Honesty note (the engine-parity family): #163–#166 and #177 are *shell
semantics* bugs, not environment bugs. No adversarial lane catches them by
injection — they need the GNU-bash diff battery. AM10 therefore carries
the battery as a mandatory step (volume pipes, merged redirects, path
translation, failing expansions) executed under hostile conditions, and
its first run will also re-derive that family. The class's real owner
remains the upstream semantic suites; this matrix refuses to claim them.

## 2. The lanes

Conventions shared with the journey program: sandbox `HOME`/`USERPROFILE`
both redirected; assertions on what the USER sees (screen text, files
under the sandbox); every red either fixes a product ticket or registers a
KNOWN-FAIL — labeling, never waiving; raw artifacts under
`target/adversarial-results/<lane-id>/` (untracked), durable findings in
`docs/audit/findings/advmatrix-<lane-id>.md`.

Each lane lists: injection mechanism (concrete), probe scenarios
(3–5), the assertion set, today's-bug mapping, and harness
(journey-driver phase vs own harness — see §4).

---

### AM01 — hostile network (axis 1) · wave 1 · journey-driver phase

**Injection.** `reset-proxy.py` around every git egress (sandbox
`http.proxy`/`https_proxy`): `reset-handshake` (TLS dies at handshake),
`reset-after-bytes N` (reset mid-clone/mid-pack), `reset-every K`
(storm), plus the journey's existing cheap tier (`insteadOf` →
`http://127.0.0.1:1/` for DNS-failure) and `https_proxy=http://127.0.0.1:1`
(configured-but-dead proxy). Control run through `--mode normal` must
clone clean, so a red is attributable to the fault, not the fixture.

**Probes.**
1. `niu plugin add <url>` under `reset-after-bytes`: fetch dies mid-pack.
2. Wizard apply (collection full) under `reset-every 2`: some clones live,
   some die — partial install state.
3. Fresh terminal at startup with a declared-but-unfetched source while
   dead (DNS tier): the five-TLS-runs shape.
4. `niu plugin sync` while dead, then heal (proxy off) and sync again.
5. Dead-proxy env var pointing at a closed port, fresh terminal.

**Assertions.** Failed add leaves **zero residue**: no spec entry, no
registry record, no per-startup deferred row (guards #174 — the bug that
made hand-editing the only cure); partial tree from a dead clone is never
listed/enabled/sourced; startup reaches a prompt ≤10 s with at most the
documented one-line notices, never a hang or re-fetch storm; sync while
dead reports per-source failure readably; healed sync repairs and the next
terminal is silent; `--checksum <sha>` on an add that fails mid-fetch
rejects and cleans up (guards #173's honest-twin).

**Bug mapping.** #174 (primary), #173, five-TLS class, F5 ledger, #171's
"failed entries keep the step red" contract.

---

### AM02 — aged / corrupted state (axis 2) · wave 1 · journey-driver phase

**Injection.** `state-corrupter.py --op age-all` between sessions on a
journey sandbox that has already lived a full gate run: mixed CRLF rc,
hand line inside the managed block, week-old `bootstrap-failures.toml`
(real schema, real schannel error text), foreign Git-Bash PS1 planted,
`oh-my-bash.sh` renamed inside the trusted tree. Targeted ops for the
rest: `registry-attribution-flip` (the #168 mechanism),
`delete-tree-git` (fingerprint reject → re-fetch churn), `imperative-era`.

**Probes.**
1. Week-old install (`age-all`), fresh terminal, daily battery.
2. `registry-attribution-flip` + spec re-sync: does sync trust the
   registry's (wrong) attribution over the rc's reality?
3. `delete-tree-git` on a trusted source, three fresh terminals: count
   `Cloning into` lines.
4. `duplicate-block` + `sync`: does sync report in-sync over two blocks?
5. `truncate-spec`/`corrupt-spec-toml` at startup.

**Assertions.** Prompt ≤10 s in every case, at most the documented
notices (deferred/`tree missing — repair with niu plugin restore`),
awaiting-trust bounds derived from the sandbox registry; NO re-fetch churn
across terminals (the 6s-source class); the user's hand line inside the
managed block survives sync or is migrated with a warning — never silent
(#176); rc stays byte-stable except legal materialization (the wt87
P3-S2 calibration); corrupt spec never wedges startup and never triggers
a destructive rc rewrite (P7-S3); registry parse failures are reported,
not silently rewritten away (the #178 family, open).

**Bug mapping.** #168 (mechanism-level), #175/#176 shapes, 6s-source, F5,
#178 family, #157/#159-era rc shapes.

---

### AM03 — dual/triple framework collisions (axis 3) · wave 1 · journey-driver phase

**Injection.** Collection `full` (OMB + bash-it both installed) plus a
**third fixture source** staged locally (a minimal bash-it-clone tree with
the same theme names) — same-name themes across 2–3 managers. Switching
loops through the wizard gallery; removals while active. No new tooling
needed: the kit's `state-corrupter --init-demo` seeds the dual record
shape for setup, and the local fixture source follows the
`smoke-theme-gallery-preview.py` staged-tree pattern (offline).

**Probes.**
1. Wizard-pick `powerbash10k` (exists in OMB AND bash-it): which
   framework's block activates? spec attribution agrees?
2. Switch A(OMB)→B(bash-it, same name)→A through the wizard, byte-snapshot
   the rc after every hop.
3. `niu plugin source remove` of the framework holding the ACTIVE theme;
   re-add; undo-receipt execution from the setup journal.
4. Third-fixture source with an AMBIGUOUS theme name: execute the undo
   receipt anyway.
5. Gallery preview while a same-name theme is highlighted: which tree
   does the preview render — and is it the same one the pick will write?

**Assertions.** Exactly ONE framework's managed block activates the theme,
spec attribution == rc reality, and a second terminal/source NEVER flips
it (#168 — the wt72 fix must hold under triple collision, not just dual);
A→B→A restores the original block bytes; removal with an active theme
degrades exactly as documented (guarded loader no-ops, floor prompt, no
error storm, spec declaration dropped, no resurrection — F4); the undo
receipt either works or fails naming the ambiguity (wt80 known-fail must
not silently regress worse); the preview renders the same tree the pick
activates (#170 truthfulness under collision).

**Bug mapping.** #168 (primary), #170, wt80 known-fail, #171.

---

### AM04 — concurrency (axis 4) · wave 2 · OWN harness

**Injection.** Two (and three) real niu processes on one sandbox HOME,
started with overlapping windows. No dedicated tool needed — the harness
orchestrates processes directly (the kit's proxies still apply to
whichever process should see a hostile network). Worst offenders: a
foreground interactive session while `niu plugin sync` rewrites the rc it
is sourcing; the wizard applying while a background sync from another
terminal materializes; `plugin source remove` in process A while process
B's loader sources that tree; two wizards finishing within the same
second.

**Probes.**
1. Interactive session (themed prompt alive) + `niu plugin sync` from a
   second process, then `source ~/.niubashrc` in session one.
2. Wizard Apply (cloning) + parallel `niu plugin sync` + a third terminal
   opening mid-apply.
3. Process A: `plugin source remove oh-my-bash` while process B idles at
   the themed prompt from that tree.
4. Two wizards racing to completion (staggered 0.5 s).
5. `plugin update` on a source whose tree another process is executing
   from (the loader mid-`source`).

**Assertions.** The rc is never interleaved/corrupt: after every
collision the rc parses and contains exactly one managed block per id
(atomic write-or-backup contract; #175's duplicate-block tolerance is the
detector); sync never resurrects a removed source mid-flight (F4 under
races); at most one process wins each write and the loser reports honestly
(nonzero + named reason), never prints success silently (#175 honesty);
no process wedges: every CLI verb and every startup completes bounded
(≤10 s startup, verbs ≤ their documented bounds); registry survives
(#178 family: a failed parse line must not be silently dropped by the
loser's rewrite).

**Bug mapping.** #168 (read-modify-write clobber family), #175 (fake
success under write failure), #174 (residue visible to the other process),
#178.

---

### AM05 — terminal adversity (axis 5) · wave 2 · OWN harness

**Injection.** ConPTY resize API (pywinpty `pty.resize`) mid-prompt and
mid-menu; fixed-width runs at 40/80/120/200 columns; CJK-heavy themes
(the wizard gallery's CJK-named rows plus a fixture theme with wide-char
segments); `NO_COLOR=1` and `TERM=dumb`; the same journey walked under
three hosts: raw ConPTY (pywinpty), Windows Terminal (`wt.exe` profile
spawn), legacy conhost (`conhost.exe` hosting the exe). Reuses the
journey's pyte screen-model and transcript formats — assertions are
screen-model snapshots, not eyeballing.

**Probes.**
1. Render the themed prompt, resize 120→40→120 mid-idle (clock repaints
   firing), then send `echo MARK` with wake disabled.
2. Cursor (row,col) matrix per theme × width via pyte, against the
   expected end-of-input position (#169).
3. Gallery live preview at 40 columns and after resize (#170).
4. Whole wizard walk under `NO_COLOR=1` and `TERM=dumb`.
5. J1→J5 under conhost and Windows Terminal vs raw ConPTY.

**Assertions.** After any resize the first keystroke still executes whole
(#167 under geometry churn); the cursor sits at the logical end of the
input line for every theme × width — pyte cursor == expected cell, no
mid-path parking (#169); the preview recomputes geometry and stays the
live prompt, never a frozen static pane, at every width (#170); NO_COLOR
sessions render with zero ANSI SGR sequences and the wizard still
completes (screen-reader shape); host class changes nothing user-visible:
same rows, same cursor behavior across ConPTY/WT/conhost (the wt87
wrapped-fragment class stays dead because the harness compares
prompt-shaped rows).

**Bug mapping.** #169 (primary), #167 (resize + repaint race), #170, the
wt87 fingerprint-artifact class, #117 (foreign glyph rows).

---

### AM06 — resource exhaustion (axis 6) · wave 2 · OWN harness (VHD lifecycle) + shared asserts

**Injection.** `diskfull-vhd.ps1` (demoed: 200 MB NTFS volume, `Fill`
to real ENOSPC HRESULT 0x80070070, disposable) with the sandbox HOME ON the
VHD for install/apply probes; read-only HOME via deny-ACL (`icacls
<path> /deny Everyone:(WD,AD)` on the sandbox home between sessions);
`file-locker.ps1` holding `~/.niubashrc` FileShare::None (AV shape,
demoed: append fails `Device or resource busy`) during plugin verbs.

**Probes.**
1. `niu plugin add` + collection apply with HOME on a FULL VHD.
2. Enable/disable with the rc locked by `file-locker.ps1`.
3. Sync (materialization rewrite) with HOME read-only.
4. Startup with HOME read-only (can it even write its journal/ledgers?).
5. Wizard Apply while the VHD crosses into ENOSPC mid-clone (Fill
   racing the clone).

**Assertions.** Every failure is honest and bounded: add/apply under
ENOSPC exits nonzero, names the problem, and leaves zero spec/registry
residue (#174 under disk-full); enable/disable with a locked or read-only
rc MUST fail loudly (nonzero + reason + rc intact) — the #175 fake-success
class dies here; a backup is attempted before any verb rewrite and its
failure blocks the rewrite (#176 backup contract); startup with read-only
HOME still reaches a floor prompt ≤10 s (degraded, not wedged); the
wizard never half-writes the rc (either the old bytes or the new bytes,
never truncated — #157/#159 class under I/O failure).

**Bug mapping.** #174, #175 (primary), #176, #157/#159 under I/O failure.

---

### AM07 — weird-but-legal input (axis 7) · wave 1 · journey-driver phase (env-per-session variants)

**Injection.** `env-poisoner.py` presets as session env (demoed: Git-Bash
PS1+MSYSTEM, HOME/USERPROFILE split, both unset, 64 KB values,
CRLF+unicode values, BASH_ENV probe, garbage NIU_*); sandbox HOME paths
with spaces, CJK (`工作区`), 240+ char length; a theme name of 200 chars
in the spec; rc lines with unbalanced quotes as pre-existing user content.

**Probes.**
1. Full wizard walk in a sandbox at `...\adversarial demos\工作区\home`.
2. HOME/USERPROFILE split (`both-homes`): which one wins, and is the
   other silently created/polluted?
3. `no-home` session: startup behavior and where (if anywhere) state
   lands.
4. `gitbash` + `msys-parent` sessions: prompt #1 clean, zero
   `__git_ps1` nags (#117/#141).
5. 200-char theme name + cwd-relative `plugin add ./neighbor-dir` target
   from a different cwd.

**Assertions.** The wizard completes in every sandbox-shape; the rc
quotes every path it writes (spaces/CJK/length survive
source in a fresh terminal); USERPROFILE-vs-HOME follows the documented
precedence and never creates state in the loser home; no-home degrades
with a message, never a crash or silent real-profile writes; zero
foreign-PS1 glyphs and zero inherited nags at prompt #1 (#117); add
stores an absolute target or documents relative semantics (#176's
cwd-relative finding); the 200-char theme name either works end-to-end or
is refused with a named limit.

**Bug mapping.** #117 (primary), #162, #176 (cwd-relative), #141 partial
(env shape; the real MSYS-parent process shape stays marked partial per
journey-spec P10-S4).

---

### AM08 — upgrade / migration paths (axis 8) · wave 1 · journey-driver phase

**Injection.** `state-corrupter.py` shape seeds: `imperative-era` (pre-1.3.1:
registry + trees, NO plugins.toml), the #159-era rc (broken backslash
generator shape), the #157-era rc (unclosed `${…//\`), plus each
intermediate: 1.2.x rc → 1.3.4 binary; registry-with-pins → update in the
new binary; old `setup-journal.toml` undo receipts. All offline (local
fixture trees, no clones needed for the upgrade itself).

**Probes.**
1. Imperative-era sandbox → first terminal → `sync` → `sync --adopt` →
   `sync` again (journey P9-S1 exactly).
2. #159-era rc + new binary: fresh terminal, then `niu setup` re-run over it.
3. 1.2.x rc + 1.3.4 binary + registry pins: `plugin update` moves pins
   without re-cloning what exists.
4. Old journal undo receipts executed by the new binary.
5. Upgrade under AM01's dead network (the upgrade-time "6s source").

**Assertions.** First imperative-mode run prints NO
"installed but not declared" nag (F1), the migration one-liner appears
(F3), `--adopt` snapshots enablement+theme, the following sync is a
byte-stable no-op (F2); the new binary NEVER silently rewrites an old rc
behind the user's back (#159-era bytes survive until the user re-runs
setup); pins survive upgrade with zero re-downloads (network budget:
`Cloning into` count == 0 on upgrade-with-existing-trees); undo receipts
from the old journal parse and execute or fail naming the incompatibility.

**Bug mapping.** F1–F6 (every 1.3.1 hotfix shipped in this phase),
#157/#159, #150's startup-validation behavior (degrade with a message,
never a silent wrong shell), #173 (pins surviving).

---

### AM09 — timing / hostility (axis 9) · wave 1 · journey-driver phase

**Injection.** `throttle-git.py` (demoed 7.9 KB/s at `--rate-kb-per-s 8`)
on all git egress; `--stall-after-bytes 200` (captive portal with partial
data); `--mode blackhole` (connect succeeds, silence) with the PRODUCT's
own timeouts as the only bound — the journey driver keeps its wake/settle
machinery out of the timing assertions. Slow-render shapes reuse the
`smoke-theme-gallery-preview.py` "hung" fixture theme.

**Probes.**
1. Wizard Apply with the full collection at 10 KB/s: progress visible?
   cancel mid-slow-clone clean?
2. Startup with a declared-but-unfetched source at 10 KB/s: does startup
   stay bounded (defer) or does it block on the fetch (the #145 hang
   class returning via the plugin path)?
3. A theme whose render takes 2 s (fixture): first-key probe with wake
   disabled right after it paints (#167 under slow paint).
4. An asset that blackholes mid-listing: gallery/wizard must not freeze
   (the 1.5 s preview bound under network stall).
5. Sync under `--stall-after-bytes`: partial fetch, then timeout —
   tree state after a STALLED fetch (never half-trusted).

**Assertions.** Startup remains ≤10 s under throttle and blackhole —
defer, don't block; clone progress lines appear at 10 KB/s (not a frozen
screen); Ctrl-C mid-slow-clone leaves zero spec/registry/tree residue and
a prompt that still works (#174's Cancel twin); first-key integrity holds
after slow and after hung renders (#167); a stalled fetch leaves the
source untrusted/absent, never "enabled" (#173/#174 honesty under partial
data); the 6s-source regression (fetch churn per terminal) stays dead
under throttle.

**Bug mapping.** #167 (primary timing shape), #174, #145 class, F5/F6,
five-TLS class (slow-net cousin).

---

### AM10 — composition storm (axis 10) · wave 3 (starts after wave 1 injections stabilize) · OWN harness

**Injection.** EVERYTHING AT ONCE: `state-corrupter --op age-all` on a
week-old sandbox + `throttle-git --rate-kb-per-s 10 --stall-after-bytes N`
rotating to `reset-proxy --mode reset-every 2` mid-run + a second niu
process syncing in the background + a terminal resize mid-battery + the
`env-poisoner gitbash` preset — then the FULL daily battery, including
the engine-parity battery (volume pipes `seq 200000 | wc -l`, merged
redirects, path-translation probes, failing expansions) that honestly
owns #163–#166/#177.

**Probes.**
1. Aged state + dead network + concurrent sync: fresh terminal + battery.
2. Wizard re-pick under throttle while a background sync materializes and
   the window resizes (#168's exact compound entry door).
3. Battery under composition (each of today's engine bugs re-derived
   under hostile conditions).
4. Heal in stages (network → state → concurrency) and verify the product
   converges to the steady state AM02 asserts (repairability, not just
   survival).

**Assertions.** Startup bounded under EVERY combination (no compound
hang); the rc parses and holds exactly one managed block per id after
every compound collision; the theme the user picked is still the theme
active after the storm (#168 under composition); zero silent data loss
(user rc lines, registry pins, journal receipts all survive); every error
the user sees names the true cause (no dead-proxy error dressed as a
theme error); the battery's parity assertions match GNU shapes even under
hostility.

**Bug mapping.** The owner's thesis: "today's bugs lived in
combinations" — #168 was sync-rewrite × aged dual-framework state; #174
was failed-fetch × spec-first persistence. This lane is the regression
net for the *combination* class; every individual-lane assertion re-runs
here under composition.

---

### AM11 — rc-block surgery (axis 11, split from aged-state: the rc writer is the most-burned surface) · wave 1 · journey-driver phase

**Injection.** `state-corrupter.py` targeted ops + `file-locker.ps1`:
`duplicate-block`, `approximate-marker`, `hand-line-in-block`, `crlf-mix`,
`truncate-rc`, `foreign-ps1`; locked rc during verbs; plugin verbs
(enable/disable/sync materialization) driven through each shape.

**Probes.**
1. `disable` on a duplicated block: BOTH marker pairs gone, zero residue.
2. `approximate-marker` (missing `>>>`): enable/disable/sync must report
   the malformed block, not silently create a second unmanaged block
   (#176).
3. `hand-line-in-block` + sync: the line survives or is migrated with a
   warning — never dropped silently (#176).
4. `crlf-mix` rc + any verb rewrite: the file's original line endings
   survive (no whole-file LF rewrite) (#176).
5. Every verb under `file-locker.ps1`: nonzero exit + named reason +
   original rc bytes intact; a backup written before every rewrite
   (#175/#176).

**Assertions.** All block operations scan ALL marker pairs (#175's
root cause: they stop at the first); enable/disable failures are
nonzero and name the cause (no `let _ =` swallowing, no success print
after a dropped write); sync reports duplicates as out-of-sync instead of
in-sync; the managed-block invariant "user lines are never silently
lost" holds for every op; rc byte snapshots before/after each verb prove
the smallest-possible-diff contract.

**Bug mapping.** #175 (primary), #176 (primary), #157/#159 (writer
regression), #168's rewrite-tell (`selection materialized` never on
unchanged spec).

---

### AM12 — collection-apply integrity (axis 3+1 combo: the #171/#172/#173 surface) · wave 1–2 · journey-driver phase

**Injection.** Collection apply run under AM01's proxies (each entry's
clone independently failable via `reset-every`), with a locally staged
fixture collection manifest (the `smoke-theme-gallery-preview.py`
staged-tree pattern) declaring theme AND completion AND plugin AND alias
entries, including deliberately-failing entries (404 ref, bad checksum,
missing entry file).

**Probes.**
1. Apply the fixture collection under `reset-every 2`: per-entry
   outcomes.
2. An entry with `--checksum` pinned: correct vs corrupted tree.
3. Completion entries: after apply, Tab-completion probe on ≥10 applets
   (`tr`, `grep`, `find`, …) vs their `--help` surface (#172).
4. Failure sweep: every intentionally-failing entry → startup transcript
   after the failed apply.
5. Trust flow over a partially-failed apply: trust the survivors, fresh
   terminal.

**Assertions.** Per-entry failure semantics: failures are listed per
entry, the apply is NOT all-or-nothing, and every failure leaves zero
residue (no spec row, no startup deferred line — #174); checksum tier
enforced: corrupted tree refused + cleaned, honest tree accepted (#173 —
the lockfile must keep its promise); claimed completions exist and fire
(#172 L1): the post-apply completion probe returns suggestions matching
the applet option surface, and the journey's completion assertion step
(#171) holds for non-theme entries; trust gate stays bounded over the
survivors; the theme ecosystem remains usable after partial failure
(floor prompt, gallery intact).

**Bug mapping.** #171 (primary), #172, #173, #174.

---

## 3. The injection toolkit (all demoed once, 2026-10-02)

Evidence under `target/adversarial-demo/` (untracked); commands and full
results in [`scripts/adversarial/README.md`](../../scripts/adversarial/README.md).

| Tool | Demo result (one line) |
| --- | --- |
| `reset-proxy.py` | baseline `normal` clone rc 0 through the proxy; `reset-after-bytes 2048` → `error: fetch failed` rc 128, empty worktree (proxy log: `RST mid-transfer at 202 bytes, dropping 4239-byte chunk`) |
| `throttle-git.py` | `--rate-kb-per-s 8` → per-connection `avg 7.9 KB/s`, clone completes slowly; `--stall-after-bytes 200` → `Operation too slow` rc 128 in 3 s; `--mode blackhole` → same bounded failure in 4 s with `http.lowSpeedLimit` set |
| `diskfull-vhd.ps1` | `New` mounted M: (200 MB, diskpart path; `New-VHD` auto-fallback works), `Fill` → `ENOSPC hit after 187 files … HRESULT 0x80070070`, 2 MB write refused, `Status` OK, `Remove` detached+deleted |
| `state-corrupter.py` | `--init-demo` seeded a week-old dual-source install; `--op age-all` produced mixed CRLF + in-block hand line + real-schema schannel bootstrap memo + foreign PS1 + renamed tree file; `duplicate-block` doubled the markers (4); `registry-attribution-flip` flipped the adapter; `--op restore` rolled the files back |
| `env-poisoner.py` | `gitbash,garbage-niu` child observed `PS1=\[…`, `MSYSTEM=MINGW64`, `NIU_LANG=zh_CN.bogus`, `TERM=/`; `no-home` child observed both HOME and USERPROFILE unset |
| `file-locker.ps1` | append while locked → `Device or resource busy` rc 1; `-Test` → `LOCKED … being used by another process` rc 9; after release → append rc 0 |

Design rules (see README): the kit injects, lanes assert; deliberate
kills RST while clean closes FIN (a `normal` control run must clone clean
so reds are attributable); state-corrupter writes the product's REAL file
shapes (marker pair, spec/registry/bootstrap schemas read from source);
everything backs up before mutating; no stuck processes (every fixture is
bounded and killed).

## 4. Harness sharing: which lanes ride the journey driver

The golden journey driver already owns: sandbox HOME redirect, ConPTY +
pyte screen model, transcript/verdict artifacts, `send_line(wake=False)`,
`SandboxSeed`, output-anchored sequencing, `--phases` registration. Seven
lanes need only ONE new driver capability — an **injection fixture**:
spawn/kill a kit process (or run a corrupter op) around a phase, exactly
like `break_network()/heal_network()` in journey-spec P5-S7:

| Lane | Harness | Needs |
| --- | --- | --- |
| AM01 hostile-network | journey phase | injection fixture (proxy lifecycle) + `netfail` (exists) |
| AM02 aged-state | journey phase | `seed` (exists) + corrupter ops |
| AM03 framework-collisions | journey phase | none new (local fixture sources + seed) |
| AM07 weird-legal-input | journey phase | `env-per-session` (exists as capability; parameterize `build_env`) |
| AM08 upgrade-paths | journey phase | `seed` (exists) |
| AM09 timing-hostility | journey phase | injection fixture (throttle lifecycle) |
| AM11 rc-block-surgery | journey phase | `seed` + injection fixture (locker lifecycle) |
| AM12 collection-apply | journey phase | injection fixture + staged fixture collection |
| AM04 concurrency | **own harness** | multi-process orchestration; reuses sandbox env + kit |
| AM05 terminal-adversity | **own harness** | ConPTY resize API + alternate hosts (wt.exe/conhost); reuses pyte + transcript format |
| AM06 resource-exhaustion | **own harness** (VHD/ACL lifecycle per process); assertions shared with AM11 | `diskfull-vhd.ps1` + `file-locker.ps1` + deny-ACL |
| AM10 composition-storm | **own harness** | orchestrator over everything; lands LAST |

The own-harness lanes still emit the journey manifest format
(`verdict.json`/`verdict.txt`/`transcripts/`) so the release gate and
lane verdicts stay one readable shape. Every lane registers its steps as
phases in the driver once its harness stabilizes, per the
journey-gate.md "How to add a step" rules.

## 5. Dispatch recommendation (the "ten agents" question)

**Dispatch all twelve lanes at once — but in two readiness classes, so
nobody blocks on anybody.** The owner's instinct (parallel, unrestricted)
is correct and the kit makes it safe: every lane owns exclusive dirs
(`scripts/adversarial/probes/<lane-id>/`, findings
`advmatrix-<lane-id>.md`, artifacts `target/adversarial-results/<lane-id>/`)
and product code stays READ-ONLY (findings, never fixes — fixes are
dispatched per-ticket after lane evidence lands, same discipline as the
wt74 audit fleet).

- **Wave 1 — immediately (8 lanes, kit-complete):** AM01, AM02, AM03,
  AM07, AM08, AM09, AM11, AM12. These ride the journey driver + the
  already-demoed kit and between them cover every P0/P1 in the record
  (#167/#168/#169-adjacent/#173/#174/#175/#176). Highest-value first
  probes: AM01-S1 (#174), AM11-S1/S5 (#175/#176), AM03-S1 (#168),
  AM09-S3 (#167).
- **Wave 2 — same day, harness-first (4 lanes):** AM04, AM05, AM06,
  AM10 start by writing their own harness stubs (process orchestration,
  resize API, VHD lifecycle, composition orchestrator) — self-contained
  work, zero shared files, no waiting. AM10 begins consuming wave-1
  injections once AM01/AM02/AM04 shapes stabilize; it should be the last
  to run probes and the first to re-run before any release.

One standing order for every lane: a control run (no injection, same
fixture) must pass before any red is reported — that is what keeps the
owner's "don't trust a green matrix" lesson pointed the right way in
both directions. And per repo discipline: no lane pushes or merges; the
captain commits per-author, per-family after WSL/ConPTY verification.
