# How Real Shells Test Interactive Rendering — and the Niubash Adoption Plan

Lane wt74/rtstudy (2026-10-02). Owner directive (verbatim intent): 看看别人的
shell 是怎么测试的，不要给我想当然 — *study how real shells test, do not
guess.* Every claim below is read off the cited source file at the cited
line. No behavior was inferred from memory, docs, or a suite diff.

Studied sources (snapshot, fetched 2026-10-02 via codeload tarball; github.com
TLS was being reset, so tarballs + raw fetches carried the day):

| Project | Snapshot |
| --- | --- |
| fish-shell/fish-shell | `master` @ `35c290272cc9c4c51518d0b87810a404d1ce66e1` |
| zsh-users/zsh | `master` @ `8cc5eade5ff80de1dca7bd27509eddca4f849f29` |
| nushell/reedline | `main` @ `d7bcb12ff583d151906fbf819e4bc844c1afaefc` (+ pinned vendored **0.50.0** — the exact version niubash builds against) |
| atuinsh/atuin | `main` @ `d440a2e616825533d3eee0db2af7d22d68dcc5c2` |
| oils-for-unix/oils | `master` raw fetch (`spec/stateful/{bind,harness}.py`) |
| alacritty | `alacritty_terminal/tests/ref.rs` raw fetch (fixture-snapshot precedent) |
| GNU readline | vendored tree `D:/repo/rubash/third_party/bash/lib/readline/` |

---

## 1. fish — two-tier: pexpect byte streams + tmux real-screen captures

fish is the deepest reference because it tests rendering at **two distinct
layers** and knows which layer owns which bug class.

### Layer 1: pexpect PTY scripts (`tests/pexpects/*.py`)

**Mechanism.** Each test is a standalone Python script driving a real fish on a
real PTY via `pexpect_helper.SpawnedProc` (wraps `pexpect.spawn`, encoding
utf-8, `delaybeforesend = None` to kill pexpect's default 50 ms send delay —
`tests/pexpect_helper.py:184-187`). Assertions are `expect_re` over the **raw
PTY byte stream** (`tests/pexpect_helper.py:227-259`), not screen text.

**The prompt counter — no stale-prompt ambiguity.** The test environment
redefines `fish_prompt` to print a monotonic counter (`prompt 1>`, `prompt
2>`, …), incremented by the `fish_prompt` *event* (`tests/interactive.config:2-14`).
`expect_prompt()` (`tests/pexpect_helper.py:265-283`) matches `prompt N>` for
the exact N — the Nth prompt in the session. A test can never accidentally
match a *previous* prompt that happens to still be on screen. This is the
single cheapest defense against the "matched the old prompt, test passed by
mistake" flake class.

**The harness plays the terminal.** When fish asks the terminal a question,
the *test* answers it:

- Cursor position query (CPR): `send_cursor_position_report(y, x)` sends a
  fabricated `\x1b[{y};{x}R` (`tests/pexpect_helper.py:197-200`).
- Primary Device Attributes: the harness sends `\x1b[?123c`
  (`tests/pexpect_helper.py:202-203`).

This is used for real behavior tests: `tests/pexpects/scrollback.py:17-19`
answers the CPR with `(y=10, x=5)`, then asserts fish's scrollback-push emits
**`\x1b[9S\x1b[9A`** — arithmetic *derived from the fabricated cursor row*
(9 = y−1). The second case reports `(y=15, x=5)` and expects
`\x1b[14S\x1b[14A` (`scrollback.py:22-25`). The cursor position is an
**input** to the test, and the expected escape stream is computed from it.
No screen emulator needed.

**Cursor state is also asserted in-band.** `tests/pexpects/cursor_selection.py:16-50`
never looks at the screen: it binds `begin-selection`, dumps the selection via
`commandline --current-selection`, and asserts `<e>`, `<ch>`, … for
beginning/middle/end-of-line selections in inclusive and exclusive modes. The
editor's own cursor/selection model is tested through the editor's
introspection surface, which is deterministic and instant — the screen layer
is reserved for things only the screen can prove.

**What the raw-stream layer asserts:** terminal size fallback
(`terminal.py:20` spawns with `dimensions=(0, 0)` and asserts `80 24`), SIGWINCH
handling (`terminal.py:27-38`: `setwinsize(40, 50)` then `20 70`, asserting
`$LINES $COLUMNS` follow), torn escape sequences across writes
(`torn_escapes.py:25` sets `fish_escape_delay_ms 2000` and splits escapes
across sends — the #8628 regression), prompt repaint loops
(`prompt_redraw_loop.py:9-33`: a prompt that kills itself with SIGABRT must
not loop; the session must still report `still alive!`), flow control
(`terminal.py:44-46`: Ctrl-S must be ignored mid-line).

**Failure reporting is a replayable trace.** On mismatch,
`report_exception_and_exit` (`tests/pexpect_helper.py:285-368`) prints the
escaped unmatched buffer, renders it "as the tty would show it", and prints
the **last 10 input/output messages with relative millisecond timestamps**
(`:343-366`). A flake diagnosis starts from a timeline, not from a guess.

**Timing/flake strategy:** per-test default timeout 5 s (`:28`), per-call
overrides (`terminal.py:67` sets `timeout=1` for a *should-fail* match —
negative assertions get their own short window); `shouldfail=True` as the
assert-absent primitive (`:248-256`); each pexpect script is a separate
`python3` process (`tests/test_driver.py:369-376`) so one hang cannot wedge
the suite; whole-suite concurrency = CPU count behind an asyncio semaphore
(`test_driver.py:126-129, 213-225`); every test gets a fresh sandboxed HOME
with XDG dirs redirected and `TERM`/`LANG` scrubbed
(`test_driver.py:36-92, 355-365`); exit code 127 is the cross-suite "skip"
convention (`test_driver.py:383-384`).

### Layer 2: tmux — the real terminal emulator (`tests/checks/tmux-*.fish`)

**Mechanism.** `isolated-tmux-start` (`tests/test_functions/isolated-tmux-start.fish:1-77`)
launches fish inside a private tmux server (own socket, own `mktemp -d` cwd,
`.tmux.conf` forcing emacs mode) at a **fixed geometry `-x 80 -y 10`**
(`:41`). The test then does `isolated-tmux capture-pane -p` — the *rendered
screen as text* — and asserts it row by row with `# CHECK:` comments processed
by littlecheck.

**Multi-line prompt with a documented screen grid.** `tests/checks/tmux-multiline-prompt.fish:12-21`
comments the *expected screen state* row-by-row (`[y=0] prompt-line-1 … [y=7]
prompt-line-2>`) before asserting it — the test doubles as documentation of
what a two-line prompt must look like after three commands.

**THE cursor-position assertion.** Same file, `:29-37`: enter tmux copy-mode,
`previous-prompt` twice, then

```
isolated-tmux display-message -p '#{copy_cursor_y} #{copy_cursor_line}'
# CHECK: {{[46]}} prompt-line-1
```

a literal **cursor row + the content of the line the cursor is on**. This is
fish's answer to "where is the cursor after a multi-line prompt", and it is a
first-class assertion, not a side effect.

**Right prompt + blank-row pinning.** `tests/checks/tmux-empty-prompt.fish:15-26`
pipes `capture-pane -p | string replace -r '$' '+'` — appending `+` to every
row so trailing whitespace AND blank rows are pinned exactly. The 10 `#CHECK:`
lines assert the right prompt's vertical position against an empty left
prompt, row by row.

**Wrap/multiline editing under resize.** `tmux-autosuggestion-multiline-resizing-prompt.fish:32-46`
types one key into a 10-line wrapped buffer under a multi-line prompt and
`capture-pane`s the exact continuation indent of all 10 rows
(`2.2 if true`, then 8 rows of `            echo helloN`). `tmux-repaint.fish:33-58`
asserts the prompt counter after each forced repaint binding.

**Timing/flake strategy here is explicit and honest:** `tmux-sleep` is 0.3 s
locally, 1 s on CI (`isolated-tmux-start.fish:24-27`); `sleep-until` polls a
predicate up to 100 times (`:29-39`); startup waits up to 50 × 0.2 s for a
non-empty first screen (`:68-75`); and the flakiest screen-position test
carries its own admission:

```
# disable on github actions because it's flakey
#REQUIRES: test -z "$CI"
```

(`tmux-multiline-prompt.fish:4`) plus tmux-version and less-version guards
(`:3-5`). The test stays in the tree for local runs; CI does not lie about
flakiness, it routes it.

### Layer 0: unit tests of the cursor layout model (`src/screen.rs`)

fish's Rust rewrite unit-tests the *math* that keeps the cursor right:
`test_escape_code_length` (`src/screen.rs:2087-2122`) — zero-width escapes
must not count toward column math; `test_prompt_truncation`
(`src/screen.rs:2196-2255`) — `calc_prompt_layout` must produce
`PromptLayout { line_starts: vec![0, 17, 24, 41], last_line_width: N }` for a
given prompt and width; `test_compute_layout` (`:2325+`). `line_starts` +
`last_line_width` **is** the multi-line-prompt cursor model: which screen row
each prompt line starts at, and the cursor column on the last one.

---

## 2. zsh — zpty + sentinel prompt + in-band ZLE state dumps

**Mechanism.** zsh's completion/ZLE tests run the real shell under the
`zsh/zpty` module (`Test/comptest:22` — `zpty zsh "$comptest_zsh -f +Z"`)
with:

- a sentinel prompt `export PS1="<PROMPT>"` (`Test/comptest:21`), waited for
  before anything else (`:24-27`);
- **fixed terminal geometry**: `stty 38400 columns 80 rows 24 tabs -icanon
  -iexten`, `TERM=vt100`, `KEYTIMEOUT=1` (`:36-39`);
- deterministic completion styling via zstyle markers so listing output can
  be parsed back out: `list-colors "lc=<LC>" "ec=<EC>\n" "rc=<RC>"` etc.
  (`:44-49`).

**The cursor introspection widget — zsh's crown pattern.** A custom widget
`zle-finish` (`Test/comptest:79-89`) captures the line editor's state and
prints it as structured text:

```
local buffer="$BUFFER" cursor="$CURSOR" mark="$MARK"
…
print -lr "<WIDGET><finish>" "BUFFER: $buffer" "CURSOR: $cursor"
(( $+mark )) && print -lr "MARK: $mark"
```

The test sends keys, then Ctrl-X, and asserts on `BUFFER:`/`CURSOR:`/`MARK:`
lines (`comptest()` parses them at `:171-203`). `complete-word-with-report`
(`:60-66`) does the same for the buffer split at the cursor
(`<LBUFFER>…</LBUFFER>\r\n<RBUFFER>…</RBUFFER>`). **Editor cursor position is
asserted in-band through `zle` variables** — same philosophy as fish's
`commandline --current-selection`, never through pixels.

**Key-sequencing determinism.** `zletest` (`:205-220`) inserts an explicit
11 ms wait between multi-key groups (`(( first++ )) && { sleep 2 & } | read
-t 0.011 -u 0 -k 1`, `:210`) so `KEYTIMEOUT`-bound escape-sequence
disambiguation resolves identically on every machine — timing is *pinned by
the harness*, not left to scheduling luck.

**Internal debug messages are failures.** `zpty_handle_dputs`
(`:102-138`) detects any leaked DPUTS assertion from a debug build in the
captured output and fails the test, printing `DPUTS:{file.c:line: msg}`.

**Highlighting tests normalize, they don't match bytes.**
`Test/X04zlehighlight.ztst:9-17` empties PS1/PS2 so ZLE output is
unambiguous; `zpty_line` (`:25-47`) rewrites terminal-specific `me`/`sgr0`
into a canonical `\e[0m` before comparing — expected-vs-actual comparison
happens **after normalizing known-varying escape sequences**, so the suite
survives terminfo differences.

**The harness plays the terminal here too.** `Test/X06termquery.ztst:6-19`
(`termresp`) *injects* fabricated terminal responses (OSC 11/10 colors,
XTGETTCAP replies, DA1, kitty protocol flags) into the pty and asserts the
parsed `.term.*` state via `typeset -p`.

**Prompt expansion tests are pure strings.** `Test/D01prompt.ztst:20-36`
tests `%d`, `%~`, `%h`, `%{...%}` (zero-width markers collapse: `a%{...%}b:
ab`) via `print -P` against literal expected text — prompt *expansion* is
string semantics, no pty needed.

**Process isolation:** every `.ztst` file runs in its own zsh process
specifically to "protect from catastrophic failure of an individual test"
(`Test/runtests.zsh:4-6, 17-24`); exit > 128 is reported as a signal death
(`:23-26`). There is no built-in per-test timeout — isolation is by process,
not by watchdog.

---

## 3. GNU readline — the canonical cursor model, untested upstream

The vendored bash tree's readline (`lib/readline/display.c`) is the
specification niubash's rendering must honor, and it is honest about how hard
this is:

- **The two tracked coordinates**: `_rl_last_c_pos` (visible cursor column;
  an *absolute cursor position* in multibyte locales but a *buffer index*
  otherwise — "This is an artifact of the donated multibyte support. Care
  must be taken when modifying its value") and `_rl_last_v_pos` (visible
  cursor row relative to the start of the input line)
  (`display.c:192-197`). `_rl_vis_botlin` is the number of physical lines the
  input occupies minus 1 — the row the cursor belongs on when idle
  (`display.c:198-201`).
- **The prompt-invisible problem is *the* cursor bug class.**
  `PROMPT_ENDING_INDEX` exists specifically to decide "whether the current
  cursor position is in the middle of a prompt string containing invisible
  characters" (`display.c:124-131`), driven by `prompt_last_invisible`
  computed while expanding the prompt's `\001`/`\002` ignore markers
  (`display.c:260`, `611-653`). A prompt renderer that miscounts invisible
  bytes puts the cursor *in the middle of the visible prompt* — exactly the
  owner's cursor-in-the-middle-of-path class.
- **The display update is a two-buffer diff** ("Keep two buffers; one which
  reflects the current contents of the screen, and the other to draw what we
  think the new contents should be… then place the cursor where it belongs",
  `display.c:146-152`, `rl_redisplay` at `:808`), with `CR_FASTER`
  (`display.c:120-122`) deciding backup-vs-CR+forward from the two tracked
  positions.
- **Reset points are explicit functions**: `rl_on_new_line()` zeroes
  `_rl_last_c_pos = _rl_last_v_pos = 0` and `_rl_vis_botlin`
  (`display.c:2741-2751`); `rl_on_new_line_with_prompt()` reconstructs state
  when someone else printed the prompt — and its own comment concedes "This
  still doesn't work exactly right" (`display.c:2778-2787`);
  `rl_reset_after_signal()` re-preps the terminal and signals
  (`signals.c:586-591`).
- **End-of-input cursor placement**: `_rl_update_final()`
  (`display.c:3431-3452`) homes to `_rl_vis_botlin` and *compensates the
  extra CRLF when the cursor is the only thing on an otherwise-blank last
  line* (`_rl_vis_botlin && _rl_last_c_pos == 0 && visible_line[…] == 0`) —
  i.e. readline itself special-cases `(row, col) == (last_row, 0)`.

**Testing reality (verified, not assumed):** readline ships **no automated
test suite** — only the manual `examples/rltest.c` harness; the bash tree's
83 `tests/*.tests` files contain no readline-display test (verified by
inventory). The canonical model is enforced *only* by downstream shells
(fish's tmux layer, zsh's zpty layer, and the modern-Rust patterns below).
That is precisely the gap niubash must fill for itself.

---

## 4. reedline (the SAME editor crate niubash pins) and atuin — the modern Rust patterns

### reedline

**In-crate unit tests (upstream).** reedline main carries 848 `#[test]`s.
The test writer is an enum behind the painter: `W::Terminal` / `W::Sink` /
`W::Capture(Vec<u8>)` — "Captures all output into a buffer so tests can assert
on the exact escape-byte stream the painter emits" (`src/painting/painter.rs:69-86`,
`capture()` at `:110`, `captured()` at `:115-124`). And the cursor query has a
test answer: `W::sink_with_cursor_at((row, col))` (`painter.rs:103`) — a sink
that replies to a cursor measurement with a fabricated position.

**The stale-anchor regression test is the owner's bug class, upstream.**
`test_prompt_does_not_climb_when_winsize_under_reports`
(`painter.rs:1996`, regression for nushell/reedline#1205): the believed
screen height is 10 but the fabricated cursor reports row 35; each repaint
must re-anchor at 35 and grow the height to 36 — asserted as
`anchors == vec![35; 4]`, with the failure message `"prompt walked up the
screen"`. A sibling covers the same after a resize event (`painter.rs:2010`),
and a third pins that an *accurate* winsize is left alone (`:2039`). The
harness helper `repaint_from_stale_anchor_at` (`painter.rs:1958`) can set the
cached anchor and the terminal-reported cursor row **independently** — the
two quantities that drift apart in real terminals.

**Public API niubash can drive today (verified in the vendored 0.50.0
source):** `Reedline::run_edit_commands(&[EditCommand])` (`engine.rs:2013`),
`current_insertion_point() -> usize` (`engine.rs:921`), and
`current_buffer_contents() -> &str` (`engine.rs:926`); builders
`with_edit_mode`, `with_cross_line_cursor`, etc. The pinned 0.50.0 also has
the `W::Sink`/`W::Capture` writers and an in-crate `seam_engine()` helper
that forces the prompt anchor for tests (`engine.rs` `mod tests` at
`:2672`, `rl.painter.force_prompt_anchored_for_test(0)`) — but those are
`cfg(test) pub(crate)`: usable only *inside* reedline's own suite, not from
niubash.

### atuin — the complete PTY-screen harness in Rust, on niubash's own drivers

Atuin's e2e suite (`crates/atuin/tests/README.md:1-19` — "Interactive shell
tests against a rendered PTY screen", `#![cfg(unix)]`, rstest-parametrized
over `tests/shells/{bash-blesh,bash-preexec,fish-default,fish-vi,zsh-emacs,zsh-vi}.toml`)
is built on **portable-pty — the same ConPTY driver niubash's
`tests/interactive` already uses** — plus the `vt100` crate as an in-process
terminal emulator:

- `PtyState { parser: vt100::Parser, … }` (`tests/common/pty.rs:14-27`): a
  reader thread parses every PTY byte into a real screen grid
  (`:120-141`), with a condvar so tests wake only when the screen changed.
- **The harness answers terminal queries from the emulator's own grid**:
  `answer_queries` (`pty.rs:29-66`) intercepts `\x1b[6n` and replies
  `\x1b[{row+1};{col+1}R` computed from `state.parser.screen().cursor_position()`,
  with the comment "Process output in order so cursor reports reflect the
  position at the query" (`:29`) and a dedicated split-point unit test
  `cursor_reports_follow_output_order` across `#[values(0, 1, 5, 7, 9, 12, 16)]`
  byte splits (`:271-286`). (Niubash's driver answers CPR from a
  byte-counting approximation — `tests/interactive/driver.rs:94-125, 196-206` —
  which counts escape bytes as text; see the adoption plan.)
- **Prompt detection is cursor-anchored**: `wait_for_prompt` waits until *the
  row the cursor is on* equals the sentinel prompt —
  `screen.rows(...).nth(screen.cursor_position().0)` trimmed `== PROMPT`
  (`pty.rs:205-212`) — followed by a termios check that raw mode is back on
  (`:213-217`). The prompt is `E2E_PROMPT> ` with no right prompt
  (`tests/common/shell.rs:10`; `README.md:17`).
- **Typing is render-synchronous, not sleep-based**: `send_str` types ONE
  character at a time and waits until the cursor moved or the cursor row's
  content changed (`pty.rs:179-197`) — "Wait for each character to be
  rendered before sending the next one". This is the anti-glue/anti-loss
  discipline done with screen state instead of timers.
- **Geometry is fixed and sized for the feature under test**: `(rows=50,
  cols=120)` with the comment "Leave room for the prompt above inline_height
  (40)" (`pty.rs:76`). `resize()` resizes the emulator and the PTY together
  (`:151-166`) and e2e tests exercise it (`e2e_pty.rs` covers "resize").
- **Every wait is bounded and dumps the screen on failure**:
  `wait_for_terminal` asserts `!closed && before deadline` with the full
  `screen.contents()` in the panic message (`pty.rs:228-246`).
- **Echo/output disambiguation**: `wait_for_line` matches a *whole trimmed
  line* so echoed input can't satisfy an output assertion (`:257-261`);
  tests also wait for "the line editor to actually resume (cursor back on the
  prompt, raw mode)" after actions (`e2e_pty.rs:125`).

---

## 5. pyte and screen-fixture survey (verified, not assumed)

- **fish, zsh, atuin use no pyte anywhere** (grep over all three trees: zero
  hits). fish's screen layer is tmux `capture-pane`; atuin's is the `vt100`
  Rust crate; zsh's is zpty + normalization.
- **A real shell does use pyte**: oils-for-unix/oils,
  `spec/stateful/bind.py:12-13` imports `pyte` and `pexpect`; the
  `bind -x`/READLINE_POINT test builds `pyte.Screen(num_columns, num_lines)`
  + `pyte.Stream` (`:201-202`), feeds the pexpect-captured bytes through
  `stream.feed`, reads `screen.display`, and `screen.reset()`s between feeds
  (`:204-215`), then asserts the expected command line inside the *rendered*
  screen. Its `TestRunner` pins PTY geometry
  (`pexpect.spawn(..., dimensions=(num_lines, num_columns), echo=False)`,
  `spec/stateful/harness.py:127-131`) and syncs `LINES`/`COLUMNS` env vars to
  the same numbers (`TerminalDimensionEnvVars`, `harness.py:72-97`), with a
  `num_retries` retry knob for stateful tests (`harness.py:106-110, 170-173`).
- **Niubash itself already runs pyte** in the journey gate
  (`scripts/journey/golden-journey.py:216-223` builds a `pyte.HistoryScreen`;
  `Session.text()`/`transcript()` render from it at `:264-278`). Verified
  gap: **nothing in the journey reads `screen.cursor`** (zero references) —
  the cursor class is currently unasserted in every niubash harness.
- **Does anyone snapshot rendered screens as fixtures?** In the shells
  studied: no. fish's screen checks are *inline* `#CHECK:` rows against a
  live `capture-pane`; atuin's are inline predicates against the live grid.
  The fixture-snapshot precedent lives one layer down, in the
  terminal-emulator class: alacritty's ref tests
  (`alacritty_terminal/tests/ref.rs:87-122`) replay a recorded byte log
  (`alacritty.recording`) against per-test fixtures (`size.json`,
  `grid.json` — a serialized `Grid<Cell>` — `config.json`) and fail with a
  per-cell diff; its catalog includes `vttest_cursor_movement_1`,
  `saved_cursor`, and `newline_with_cursor_beyond_scroll_region` — cursor
  behavior as *named, versioned fixtures*.

**Reading for niubash:** the shell ecosystem asserts rendered screens
*inline* (predicate or row-list) and keeps *raw transcripts* as artifacts —
which is exactly what the journey gate already does — while golden *fixtures*
are reserved for terminal emulators. Adopt inline cursor assertions + keep
transcripts; do not introduce golden screen fixtures.

---

## 6. The cursor-assertion pattern that would have caught the owner's bug

The bug: at the themed multi-line prompt, after typing/accepting a path, the
cursor sits *in the middle* of the rendered line instead of after the last
character (invisible-width bytes counted, or a stale prompt anchor). Every
current niubash harness is blind to it: the journey matches text rows and
never reads `screen.cursor`; the Rust harness answers `\x1b[6n` from a
byte-counting approximation and never asserts on it.

The pattern, as each studied project expresses it:

| Project | The assertion |
| --- | --- |
| fish | `#{copy_cursor_y} #{copy_cursor_line}` equals the expected row and content (`tmux-multiline-prompt.fish:36-37`) |
| zsh | `CURSOR:` (and `BUFFER` split at cursor) printed by the `zle-finish` widget (`Test/comptest:80-87`) |
| readline | invariant: idle cursor is at `(_rl_vis_botlin, VIS_LLEN(_rl_vis_botlin) - wrap_offset)` — what `_rl_update_final` homes to (`display.c:3431-3452`) and `PROMPT_ENDING_INDEX` guards (`:124-129`) |
| reedline | `anchors == vec![35; 4]` — the prompt never walks when the terminal-reported cursor row disagrees with the believed height (`painter.rs:1996`) |
| atuin | `screen.rows(...).nth(screen.cursor_position().0).trim() == PROMPT` — the prompt is *where the cursor is* (`pty.rs:205-212`) |

**Concretely, for niubash:** after any input action at the themed prompt,

```python
row = index of last non-empty row of screen.display
col = display width of screen.display[row]   # wcwidth, escapes excluded
assert (screen.cursor.y, screen.cursor.x) == (row, col)
```

i.e. `(row, col) == (last_row, len(last_line))` — the owner's bug fails it
the first time the cursor lands mid-path. Two implementation caveats to
verify against the emulator at write time, both known to this family: (1)
**pending-wrap** — an emulator may report `x == width` when the cursor sits
at the right edge awaiting the next character (VT100 DECAWM); accept
`col in {width, width-1}` for the right-edge case or compare `>=` for the
column; (2) **wcwidth** — the column must count *display cells* of the
rendered row (pyte's `screen.display` strings are already escape-free, so
`wcswidth` on the row is the right measure; this is the same thing fish's
`escape_code_length` unit test pins at `screen.rs:2087-2122`).

---

## 7. Adoption plan for niubash

### (a) Run today — reedline-native, pure Rust, no new machinery

1. **Editor cursor-semantics tests over reedline 0.50's public API**
   (`crates/niubash-runtime/tests/reedline_editing.rs`; `reedline` is
   already a direct dependency of that crate): construct
   `Reedline::create().with_edit_mode(...)`, drive
   `run_edit_commands(&[EditCommand::InsertChar('c'), …,
   EditCommand::MoveToLineStart, …])`, then assert
   `current_insertion_point()` and `current_buffer_contents()`. This pins
   the *in-buffer half* of the cursor class (offset semantics of every
   keybinding niubash wires: Home/End/arrows/word-motions/accept-line) with
   zero I/O — the same surface zsh exposes via its `zle-finish` widget
   (`Test/comptest:79-89`) and reedline's own engine tests use 72 times.
2. **Upstream reedline suite at the exact pin.** Script
   `scripts/run-reedline-upstream.sh`: copy the vendored
   `reedline-0.50.0` source out of the cargo registry cache into
   `target/reedline-upstream/`, `cargo test` there (dev-deps download as
   usual). 848 upstream tests of the same editor family become a regression
   signal we can run before/after any reedline bump — including the
   painter/cursor tests (`painter.rs` has 34 in the pin). Never edit that
   tree; it is a thermometer, not a steering wheel.

### (b) The golden-screen harness (multi-line prompt + cursor row/col)

**Rust tier — upgrade `tests/interactive/driver.rs` (the `-i` e2e class):**

- Add the `vt100` crate as a dev-dependency and parse the session byte
  stream into a real screen inside `TerminalResponder::observe`, replacing
  the byte-counting row/col approximation (`driver.rs:94-125`) — the exact
  move atuin made (`pty.rs:14-27, 120-141`); pure parsing, Windows-safe,
  same portable-pty underneath.
- Answer CPR *from the emulator grid* (`driver.rs:196-206` today answers
  from the approximation) — atuin's `answer_queries` (`pty.rs:29-66`) is the
  reference implementation, including byte-order and split-chunk handling
  (unit-test it across split points like `pty.rs:271-286`).
- New assertion methods on `NiuSession`: `cursor() -> (u16, u16)`,
  `expect_cursor_at(row, col)`, `expect_cursor_at_line_end()`
  (implements §6), `wait_for_screen(pred)` with bounded deadline +
  screen-dump-on-timeout (`pty.rs:228-246` shape), and
  `wait_for_prompt_cursor_anchored()` (the row *at the cursor* is the
  prompt — `pty.rs:205-212`).
- First tests, aimed exactly at the owner's report: default themed prompt
  idle → `expect_cursor_at_line_end()`; type a long path + TAB-complete +
  accept → `expect_cursor_at_line_end()`; two-line PS1 → cursor row is the
  second prompt row; resize 120×36 → 90×28 → prompt intact and cursor on
  the prompt row (fish `terminal.py:27-38` + atuin `resize()` precedent).

**Python tier — extend the journey harness (the golden-journey class):**

- `Session` already owns a pyte `HistoryScreen`; add `cursor()` and the
  §6 `assert_cursor_at_line_end()` helper (read `self.screen.cursor` —
  pyte exposes `.x`/`.y`; handle the pending-wrap `x == cols` edge per §6).
- Adopt atuin's two timing disciplines where the current settle/anchor
  layers don't already cover them: per-character render-synchronous send for
  sensitive steps (`pty.rs:179-197` — no new sleeps), and always include
  cursor `(x, y)` in the `delivery` ledger lines and timeout screen dumps so
  a red cursor assertion is diagnosable from `verdict.json` alone.

### (c) Wiring into the golden journey gate — a new assertion class `cursor`

Per the gate's own rules (`docs/journey-gate.md` "How to add a step" +
standing rule), add a `cursor` assertion class to the verdict vocabulary:

- `verdict.check_cursor(name, session, expectation)` — expectation is one of
  `{at_line_end, at_row(row), on_prompt_row}`; every outcome lands in
  `verdict.json` (step, assertion kind, expected vs actual `(x, y)`, the
  rendered last row) and `verdict.txt`.
- New steps **J7** (cursor lands at end of input at the themed multiline
  prompt, including after TAB-completion accept — the owner's exact bug) and
  **J8** (terminal resize mid-session; prompt intact, cursor on prompt row),
  following the existing `verdict.step/check/capture` shape with a capture
  at each observed moment.
- Honesty rule unchanged: if the cursor class is red today, register a
  KNOWN-FAIL with the owning ticket (e.g. `wt74-cursor-position`) rather
  than weaken the assertion — "a match still makes the gate RED … the
  verdict names the ticket" (`docs/journey-gate.md`).

### (d) Effort ordering

| # | Item | Class | Effort | Catches |
| --- | --- | --- | --- | --- |
| 1 | reedline 0.50 cursor-semantics tests (`current_insertion_point`) | unit | hours | keybinding/buffer-offset regressions |
| 2 | `scripts/run-reedline-upstream.sh` (upstream suite at pin) | upstream gate | hours | editor regressions flowing in via the pin |
| 3 | vt100 screen in `tests/interactive` + `expect_cursor_at_line_end()` + CPR-from-grid | e2e (Rust) | 1–2 days | **the owner's cursor class, offline, in `cargo test`** |
| 4 | journey `cursor` assertion class + J7/J8 + ledger/verdict fields | release gate | 2–3 days | cursor bugs on the *real themed* prompt, blocking release |
| 5 | (optional, later) prompt-layout unit tests mirroring fish's `line_starts`/`last_line_width` if niubash grows its own prompt pre-layout code | unit | as-needed | prompt width math drift |

Rationale: 1–2 are pure additions with no flake surface (no PTY), and they
make everything above them debuggable; 3 gives the offline, per-PR net;
4 puts the class on the release blocker where the owner's directive put this
whole category. No product code changes in any of these — harness and
assertions only.

---

## 8. Journey manifest additions (machine-readable)

`docs/journey-spec.md` does not exist on `master` or on wt73's branch as of
this writing, so these are **standalone**, in a deliberately schema-plain
form wt73 can adopt or re-key. They express the two new steps of §(c) plus
the new assertion class.

```yaml
# --- NEW ASSERTION CLASS ---
assertion_classes:
  - id: cursor
    driver: conpty-pyte            # scripts/journey harness (pyte screen + screen.cursor)
    kinds:
      - id: cursor-position
        expect:                     # either concrete coords or anchors
          row: last_nonempty_row    # anchor | int (0-based from viewport top)
          col: display_width_of_row # anchor | int (display cells, wcwidth)
        wrap_tolerance: right_edge  # pending-wrap: col may equal width
      - id: cursor-on-prompt-row
        expect: { row_content: PROMPT }   # atuin wait_for_prompt shape

# --- NEW STEPS ---
steps:
  - id: J7
    title: "cursor lands at end of input at the themed multiline prompt"
    class: cursor
    sandboxed_home: true
    actions:
      - send_line: "cd ~"
      - type_chars_with_completion: "cd De"   # TAB completes; long path renders
      - press: TAB
      - press: ENTER
    assertions:
      - kind: cursor-position
        expect: { row: last_nonempty_row, col: display_width_of_row }
      - kind: screen-not-contains
        value: ["command not found", "syntax error"]
    captures: ["J7-after-accept"]
  - id: J8
    title: "resize mid-session; prompt intact and cursor on the prompt row"
    class: cursor
    actions:
      - resize: { cols: 90, rows: 28 }
      - send_line: "echo J8_ALIVE"
    assertions:
      - kind: cursor-on-prompt-row
      - kind: screen-contains
        value: "J8_ALIVE"
      - kind: screen-not-contains
        value: ["syntax error"]
    captures: ["J8-after-resize", "J8-after-echo"]

known_fails_candidates:
  - id: wt74-cursor-position
    covers: "cursor mid-line at themed prompt (owner report class)"
    scope: ["J7"]
```

The Rust-tier equivalents (§b) ride the existing `tests/interactive` target
and need no manifest entries.

---

## 9. What was NOT found (honest negatives)

- readline has no automated rendering tests (manual `examples/rltest.c`
  only) — the model is code + comments, enforced downstream.
- No studied shell stores golden rendered screens as fixtures (inline
  assertions + raw transcripts instead); fixture snapshots are an emulator
  project's pattern (alacritty).
- zsh has no per-test timeout (process isolation only); fish's
  screen-position tests are flaky enough that fish itself disables the
  flakiest one on CI (`tmux-multiline-prompt.fish:4`) — so "real shells"
  manage this flake class by admission and polling, not by eliminating it.
  Niubash's existing settle/anchor/ledger machinery in the journey gate is
  already *more* defensive than anything studied here; the missing piece is
  the cursor axis, not the timing machinery.
