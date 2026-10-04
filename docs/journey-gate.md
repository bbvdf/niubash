# The Golden User Journey Gate

> **Successor spec:** [journey-spec.md](journey-spec.md) (+ the
> machine-readable [journey-steps.json](journey-steps.json)) — the phased
> P1–P10 expansion covering the blind spots J1–J6 cannot see (state
> persistence, setup re-runs, network failure, trust/spec lifecycle,
> upgrade, drift). J1–J6 below stay the release gate; the P-phases extend
> it lane by lane.

The owner's directive (2026-10-04, verbatim intent):

> 你现在的测试逻辑完完全全不是按照用户使用来的!交互式你根本不测!…必须改革测试方式
> ("Your current test logic is completely not how users use it! You don't
> test interactive at all! … the testing method must be reformed.")

The answer is this gate: **the owner's own transcripts become the automated
release blocker.** `scripts/journey/golden-journey.py` walks the exact
journey a real user walked on 2026-10-03/04 — not a synthetic shape — over
a real ConPTY (pywinpty + pyte, the `scripts/smoke-wizard-journey.py`
pattern), in a sandboxed HOME (`USERPROFILE` and `HOME` both overridden;
`USERPROFILE` wins in this product, a known pitfall, so both point at the
sandbox and the real `~/.niubash` is never touched).

## The journey (J1–J7)

| Step | The user's move | The gate asserts |
| --- | --- | --- |
| J1 fresh-install first contact | install, run `niu`, no `~/.niubash` anything | the v-banner + wizard appear: welcome block, environment panel, "No external themes installed yet" |
| J2 wizard full run | collection **full** → Apply → watch the clones → **Trust now (现在信任)** → gallery → pick `powerline-multiline` | clone progress lines appear; the trust question appears; gallery lists >60 themes; `~/.niubashrc` written with the theme, `~/.niubash/plugins.toml` written, undo receipts listed on the finish screen; the REPL prompt is alive afterwards |
| J3 activation | `source ~/.niubashrc` in the live session | ZERO syntax errors anywhere |
| J4 new terminal ×3 | open three fresh `niu` sessions | no `Cloning into` (no re-downloads), no "not declared" nags, at most the documented per-source `awaiting-trust` notices, prompt renders |
| J5 daily battery | `ls \| wc -l`, `echo hi \| cat -n`, `cd ~ && pwd`, `[[ a != b ]] && echo ok` | each produces output, not errors; prompt still alive after each |
| J6 trust + activate bash-completion | `niu plugin trust bash-completion` → `source ~/.niubashrc` → new terminal | trust reports success; ZERO syntax errors — bash_completion included |
| J7 gallery live preview (niubash#170) | re-run `niu setup`, walk the theme gallery | the pane below the highlight renders that theme's real PS1 (header follows the highlight; the old static sentence is gone; Esc fast-forwards, Cancel writes nothing) |

Exit code `0` only if every assertion holds. The run writes:

- `verdict.json` / `verdict.txt` — the per-step, per-assertion verdict;
- `transcripts/*.txt` — full screen captures (scrollback included) at
  every notable moment;
- `run.json` + `sandbox-kept.txt` — where the sandbox lived (kept for
  diagnosis when the gate is red, deleted when green).

### Known-fails are told, not hidden

The `KNOWN_FAILS` table at the top of the script registers failures with
their owning ticket. A match still makes the gate **RED** — the gate's
job is to tell the truth — but the verdict names the ticket so the red is
*expected-red until that lane lands*, not a mystery. When the fix lands,
the pattern simply stops matching and the gate goes green without edits.
Currently registered:

- `wt56-bash-completion-syntax` (lane wt56, alias family) — the
  `bash_completion: line 1376: syntax error in conditional expression`
  the owner hit on 2026-10-03; the gate observed it again on its first
  full run at J6 (fresh terminal after `niu plugin trust
  bash-completion`).
- `bash-preexec-recipe-entry` (recipe seed) — the `full` collection's
  bash-preexec recipe names entry `bash-preexec`, but upstream
  rcaloras/bash-preexec ships `bash-preexec.sh`, so the apply reports
  `1 entries failed`.

Unregistered failures stay plain RED. A red gate blocks the release
until each red is either fixed or registered — registering is labeling,
never waiving.

### The gallery live-preview golden (J7 + the offline ConPTY probe)

J7 asserts the user moment on the REAL gallery (>60 oh-my-bash themes):
the pane below the highlight renders that theme's actual prompt and
follows the highlight. Its deterministic sibling is
`scripts/smoke-theme-gallery-preview.py` — an offline ConPTY golden over
a fixture source carrying every representative theme class (single-line,
two-line, colored, right-aligned, powerline glyph, loads-but-no-PS1,
hung-forever). It snapshots the preview block per theme into the journey
manifest format (`verdict.json` / `verdict.txt` / `transcripts/`), runs
as leg `d3` of `scripts/smoke-test-1.3.0.sh`, and pins the niubash#170
degradation contract: a theme whose render fails or hangs shows
"(preview unavailable: …)" within the 1.5s bound — the gallery never
freezes, and the browse leaves no OSH_THEME anywhere.

### Network reality

The journey clones the real origins, so it inherits the network's truth:
a flaky path to github.com (TLS handshake resets, `curl 56 schannel`)
fails the gate honestly with the git error lines in the transcripts.
That is not a product bug and not a waiver candidate — re-run the gate
when the network recovers, exactly like a user would.

### Input delivery is verified, not assumed

Slow runners widen every window where niu is still drawing and the
ConPTY bridge eats keystrokes (release run 37149660449, J6: the typed
`niu plugin trust bash-completion` never echoed and the 90s wait timed
out on a screen still showing the startup banner; same family as wt61's
"menus drain input queued while they draw"). Local diagnosis of the
same gate added a second, sharper mode: **the themed prompt's
per-second clock repaint eats the FIRST input byte typed after idle**
(`echo` executes as `cho`, `niu` as `iu` — `niu: cho: command not
found` in the transcripts). So the driver hardens INPUT DELIVERY only —
four layers, all bounded — plus one sequencing rule (the output anchor
below, which owns the space *between* two sends):

- **settle** — before every send (`send_line`, `answer`, the gallery
  walk's first arrow), wait for a quiet screen (two polls, no new
  bytes, no viewport change) or a prompt-ish last non-empty row, at
  most 15s. A session that has never emitted a byte never counts as
  settled (that window is the startup-drain loss itself).
- **wake** — every REPL line is preceded by a kill-line (Ctrl-U): if
  the editor is about to eat a first byte, it eats the Ctrl-U; if not,
  the Ctrl-U harmlessly clears the input line. It also wipes any stale
  input before a retry's retype, so a resend can never concatenate two
  half-lines. Verified against the release build: Ctrl-U + line (and
  double Ctrl-U + line) execute clean.
- **confirm** — after a REPL line, wait up to 10s for its text to
  appear NEW in the rendered viewport (the editor syntax-highlights
  input, so the raw bytes never hold the word contiguously —
  `\x1b[36mecho\x1b[0m ...` — and pre-existing screen text does not
  count: the awaiting-trust nag literally spells the trust command);
  after a menu key, wait up to 10s for ANY viewport change (menus read
  raw and never echo); after Enter, wait 3s for the scroll.
- **resend once** — when the signal never came, resend the keystroke
  once and record it. This retries *delivery* only: the
  expected-output waits (`wait_for`) never retry, so a real product
  failure still fails the gate.

### Output-anchored sequencing

Release run 37153503706 (journey job, J6) exposed a hole the four
delivery layers cannot close: `echo J6_ALIVE` and the line typed right
after it executed as **one glued command**
(`❯ echo J6_ALIVEniu plugin trust bash-completion`). The second send
was delivered while the first was still being processed — the wake
Ctrl-U was the clock-repaint's eaten byte, and the text appended to the
unsubmitted input line. The settle check cannot see that state: the
typed input line *itself* starts with the prompt glyph, so the
prompt-ish matcher fires **during** execution, before the prompt has
returned.

So between two consecutive REPL sends in the same session, `send_line`
now returns only when the command it typed has **completed** — an
*output anchor*: a NEW prompt-ish last row that is neither the typed
input line nor any row carrying the command text (the command-text
guard also survives the theme clock's repaints). Bounded at 30s by
default; the known-long journey commands carry their own bound (90s for
`niu plugin trust bash-completion`, 60s for `source ~/.niubashrc`,
matching each step's expected-output wait). Every anchor outcome lands
in the `delivery` ledger as kind `anchor-wait` — one line per REPL
send, with its elapsed time: the sequencing trace a future CI flake
needs, never silent. The hidden `--stress-delay-ms N` harness
(randomized 0–N ms pause before every settle check, simulating runner
slowness — the shape that broke two release runs) validates it: normal
PASS plus three stress runs at N=800, all PASS.

The gallery's DOWN walk deliberately does not resend per key: repaint
can lag a delivered arrow, a resent arrow can overshoot the verified
row, and the walk already self-heals by polling the highlight.

Every resend (and every give-up) lands in a `delivery` ledger inside
`verdict.json` and as an `INPUT DELIVERY EVENTS` section in
`verdict.txt` — session, kind (`send_line` / `send_line-enter` /
`answer` / `anchor-wait`), attempt, keys (the waited marker, for
anchors), reason (including the settle outcome), and action (`resend` /
`undelivered` / `anchored` / `anchor-timeout`). The raw-stream and
transcript artifact formats are unchanged.

(The first-byte eat itself is product behavior worth its own ticket —
a human typing at the themed prompt after a clock tick would lose their
first keypress the same way. The gate works around it in the driver; it
does not fix it.)

## How to run locally

```sh
cargo build --release
python scripts/journey/golden-journey.py target/release/niu.exe
```

Requirements: Windows (ConPTY), `python -m pip install pywinpty pyte`,
git on PATH (the journey makes the same real clones the user's terminal
did — no offline mirror, no seeded fixtures), `ls`/`cat` on PATH for the
battery (WinuxCmd on a user machine; Git for Windows on a CI runner),
network access to github.com.

Options: `--artifacts DIR` (default
`target/journey-results/<timestamp>`), `--keep-sandbox DIR` (create the
sandbox under a directory you choose; it is kept on red, removed on
green), `--phases IDS` (comma-separated spec phase ids from
[journey-steps.json](journey-steps.json) to compose after the base gate,
e.g. `--phases P4,P7`; `all` = every registered phase; default `base` =
J1–J6, the release gate, unchanged).

### Phase composition (--phases)

The phased expansion ([journey-spec.md](journey-spec.md)) lands lane by
lane: each wave lane appends clearly-separated phase runner functions to
`scripts/journey/golden-journey.py`, registered under the spec's phase id
in `PHASE_RUNNERS` (`wt79/jw1-persistence` → `P3` + `P8-S4`,
`wt80/jw2-wizardspec` → `P4` + `P7`). The registry + the `--phases` flag
are the only shared surface, so lanes cannot collide in step code.

Selected phases run AFTER the base gate on the same sandbox — every phase
walks on the installed state J1–J6 leave — so a lane's local run is
`--phases base,P4,P7` and the whole-spec walk is `--phases all`. A
base-gate failure blocks the phases (nothing to walk on), exactly like it
blocks J2–J6 today. Phase verdicts appear in the same
`verdict.{json,txt}` as steps; expected-red phase steps carry their
registered KNOWN-FAIL labels (e.g. `wt73-168-theme-rebound`) and never
pass silently.

## How the gate blocks release

`.github/workflows/release.yml` has a `journey` job that runs on
`windows-2025`: it resolves the release tag, **preserves the current
`scripts/journey/` from master, checks out the tag's own source, builds
`niu.exe` fresh inside the job** (stale-binary discipline — the artifact
is never reused), installs `pywinpty`/`pyte`, and runs the journey. The
`release` job declares `needs: [build-windows, journey]`, so no GitHub
Release is published while the journey is red. The verdict and the full
transcripts upload as the `journey-verdict` artifact (always, even on
failure — 14-day retention) so a red gate can be diagnosed from the run
page alone.

The "preserve current gate, checkout old tag" step mirrors the packager
preservation in `build-windows`: a `workflow_dispatch` release of an old
tag must be gated by the *current* journey while building that tag's own
source. (A genuinely old tag may lack strings the current gate waits for;
the gate failing on an ancient tag is honest — release it only if you
mean it.)

## How to add a step

A step is a **real user transcript, never a synthetic shape**:

1. Record what the user actually did — the exact commands, the exact
   menu answers, the exact terminal observations (screenshots/transcripts
   beat memory; date them).
2. Append a `J7`… block in `journey()` following the existing shape: one
   `verdict.step(...)`, `step.check(...)` for every user-visible fact,
   `verdict.capture(...)` at the moments a human would look at.
3. Assert what the USER sees (screen text, files under the sandbox
   `~/.niubash`), never internals. If the honest expectation is red
   today, register a KNOWN-FAIL with the owning ticket instead of
   weakening the assertion.
4. Re-run the journey end-to-end and paste the verdict into the PR.

If your step can be probed offline (no network, no ConPTY), it belongs in
`scripts/smoke-wizard-journey.py` or the Rust tests instead — the golden
journey is for exactly the interactive, online, whole-product class.

## The standing rule

**Every interactive-class fix ships with a journey step or an `-i` e2e
test.** If a change touches the wizard, menus, prompt rendering, rc
activation, trust flow, plugin sync startup behavior, or anything a user
watches happen in a terminal, the PR carries either a new step in this
journey or an interactive end-to-end test exercising the real flow.
"Green on synthetic shapes" is not evidence for this class — that is the
directive.
