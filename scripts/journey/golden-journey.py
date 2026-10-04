#!/usr/bin/env python3
"""The Golden User Journey gate — the owner's 2026-10-03/04 transcripts as
an automated release blocker.

Owner directive (2026-10-04, verbatim intent): "你现在的测试逻辑完完全全
不是按照用户使用来的!交互式你根本不测!…必须改革测试方式" — tests must
follow REAL user journeys, interactive included, not synthetic shapes. This
script walks the exact journey a real user walked, over a real ConPTY
(pywinpty + pyte, the scripts/smoke-wizard-journey.py pattern), in a
sandboxed HOME (USERPROFILE and HOME both overridden — USERPROFILE wins in
this product, a known pitfall, so both point at the sandbox):

  J1 fresh-install first contact
     no ~/.niubash anything -> run `niu` -> the v-banner + wizard appear
     (welcome block, environment panel, "No external themes installed yet").
  J2 wizard full run
     collection "full" -> Apply -> the real `git clone`s progress on the
     terminal -> trust-now question -> "Trust now" (现在信任) -> theme
     gallery lists >60 themes -> pick `powerline-multiline` -> rc written,
     spec written, undo receipts listed on the finish screen -> the
     about-tour keypress -> the live REPL prompt.
  J3 activation
     source ~/.niubashrc in a live session -> ZERO syntax errors anywhere.
  J4 new-terminal x3
     each fresh `niu` session: no "Cloning into" (no re-downloads), no
     "not declared" nags, at most the documented per-source awaiting-trust
     notices, prompt renders.
  J5 daily battery (in the live session left open by J4)
     `ls | wc -l`, `echo hi | cat -n`, `cd ~ && pwd`,
     `[[ a != b ]] && echo ok` — each must produce output, not errors,
     with the prompt still alive after each command.
  J6 trust + activate bash-completion (the owner's exact flow)
     `niu plugin trust bash-completion` -> `source ~/.niubashrc` -> a
     fresh terminal -> ZERO syntax errors (bash_completion included).

Wave-1 persistence lane (wt79/jw1-persistence, journey-spec P3 + P8-S4 —
the two live P0s #168/#167 plus F4/F5 aged state). Runs after J5, before
J6 (the theme-bearing source must still be installed when the persistence
assertions walk; J6 then runs on the state P8-S4 restored):

  P3-S1 reopen sandbox session (aged state)
     a fresh terminal after J5: prompt renders; the picked theme still
     owns the rc (`OSH_THEME`/`BASH_IT_THEME` line intact); the spec
     still declares the wizard's sources; the session reads back a
     marker file a PREVIOUS session wrote (the wake-flag capability:
     a session detects "a previous session mutated state"), proving the
     sandbox HOME wiring survives process boundaries.
  P3-S2 rc byte-stability + theme identity across terminals (niu#168)
     snapshot rc bytes after the wizard; `source ~/.niubashrc` in a live
     session; two fresh terminals; snapshot after each. (a) bytes
     identical after every open; (b) the picked theme's variable still
     present, unchanged, on the same framework; (c) the string
     `selection materialized` (the #168 rewrite tell) never appears
     during an unchanged-spec source/startup; (d) terminal 2's first
     prompt row equals terminal 1's modulo clock digits. Expected-red
     until niu#168 lands -> registered KNOWN-FAIL
     `wt73-168-theme-rebound` (labeling, never waiving).
  P3-S3 first-key integrity as product behavior (niu#167 anti-masking)
     after the themed prompt idles >=1.2s (>=1 clock repaint), send
     `echo NIU167KEY` with the driver's Ctrl-U wake DISABLED for this
     one probe: the full word must execute, never `cho: command not
     found`. One labeled probe per session. When it passes the journey
     FLIPS the global wake off (`WAKE_ENABLED = False`): every later
     send runs wake-free, so the gate stops masking #167.
  P3-S4 aged state: damaged tree + bootstrap-failures ledger (F5)
     seed between sessions (the `seed` capability): rename a file
     inside the trusted oh-my-bash tree (the guarded loader must no-op
     silently) and hand-write a spec entry for a declared-but-missing
     origin plus its `bootstrap-failures.toml` memo. The next terminal
     reaches a prompt within 10s with at most the documented one-line
     notices (deferred / awaiting-trust); the explicit verbs repair
     (`niu plugin sync` retries the missing origin readably, `niu
     plugin restore` rebuilds the tree); after healing the spec, the
     final terminal is silent again.
  P8-S4 remove the source providing the ACTIVE theme (F4)
     `niu plugin source remove oh-my-bash` while its theme is applied:
     spec declaration drops, registry record and tree drop, a fresh
     terminal falls back to the floor prompt with no resurrection and
     no syntax-error storm; then re-add + trust + re-enable restores
     the theme in a fresh terminal, and the rc carries exactly one
     oh-my-bash block (no orphan blocks) at steady state.

Driver capabilities landed with this lane (journey-steps.json
`driver_capabilities`, owner W1):

  wake-flag  `send_line(..., wake=False)` sends a line WITHOUT the
             sacrificial Ctrl-U byte, and never retries it (a retry
             would mask exactly the first-key behavior the probe
             exists to observe). Plus the previous-session marker
             file (`~/.journey-wake.flag`): a step's session writes
             it through the shell; the NEXT session's step reads it
             back — a session detects "a previous session mutated
             state" without any driver-internal channel.
  seed       the `SandboxSeed` helper: byte snapshots of the rc,
             spec `[[sources]]` stanza add/drop, a product-format
             `bootstrap-failures.toml` memo write, and tree-file
             damage — all under the sandbox home, between sessions.

`--phase` selects the group: `full` (default: J1-J5 + P3-S1..S4 +
P8-S4 + J6), `gate` (the J1-J6 legacy gate, unchanged), `persist`
(J1-J5 + P3 + P8-S4), `P3` (J1-J5 + P3-S1..S4), `P8` (J1-J5 +
P8-S4).

Unlike scripts/smoke-wizard-journey.py (offline, mirror-seeded, one
question path), this is the ONLINE journey: the clones come from the real
canonical origins, exactly as the user's terminal did. CI and local runs
need network + git on PATH; `ls`/`cat` for the battery come from the real
PATH (WinuxCmd on a user machine, Git for Windows on a runner).

Known failures are told, not hidden: each syntax-error assertion checks
the registered KNOWN-FAIL list. A known-fail is still RED (exit 1) — the
gate's job is to tell the truth — but the verdict names the owning ticket
so the red is expected-red, not a mystery.

Exit codes: 0 every assertion holds, 1 any fail/known-fail, 2 skip
(missing python deps / not Windows / no niu.exe / no git).

Usage: python scripts/journey/golden-journey.py <niu.exe> [--artifacts DIR]
"""

import argparse
import difflib
import json
import os
import random
import re
import shutil
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

if os.name != "nt":  # pragma: no cover - ConPTY is Windows-only
    print("SKIP: the golden journey drives ConPTY (Windows only)")
    sys.exit(2)

try:
    import pyte
    from winpty import PtyProcess
except ImportError as err:  # pragma: no cover - environment guard
    print(f"SKIP: python deps missing ({err}); needs pywinpty + pyte")
    sys.exit(2)

# ── Timing discipline (scripts/smoke-wizard-journey.py pattern) ─────────────
# Menus drop input queued while they draw, so every answer settles before
# pressing; the digit and the Enter go as separate writes (one burst can
# lose the trailing Enter on the ConPTY bridge).
SETTLE_SECONDS = 0.6
ENTER_GAP_SECONDS = 0.2
NAV_GAP_SECONDS = 0.12
ENTER = "\r"
DOWN = "\x1b[B"

# ── Input-delivery hardening (release run 37149660449, journey J6) ──────────
# On a slow runner the J6 keystrokes (`niu plugin trust bash-completion`,
# typed right after the prompt came up) never reached the ConPTY: the
# screen still showed the startup banner + awaiting-trust nags with NO
# echo of the command, and the 90s wait timed out. Same family as wt61:
# interactive screens drain console input queued while they draw — a slow
# runner widens every such window. Delivery is therefore VERIFIED, not
# assumed (bounded everywhere; the send always proceeds when a bound
# expires, and every retry is recorded as a delivery event):
#   settle — before every send, wait for a quiet screen (two polls with
#            no new bytes and no viewport change) or a prompt-ish last
#            non-empty row. Bounded by QUIESCE_TIMEOUT_SECONDS.
#   wake   — REPL lines are preceded by a kill-line (Ctrl-U): the themed
#            prompt's clock repaint eats the FIRST input byte after
#            idle, so the kill-line goes first as the sacrifice (and
#            clears stale input before a retry's retype).
#   echo   — after a REPL line, wait for the typed text to appear NEW in
#            the rendered viewport (the editor syntax-highlights input,
#            so raw bytes never hold the text contiguously; and
#            pre-existing screen text does not count); after a menu key,
#            wait for ANY viewport change (menus in raw mode give no
#            echo). Bounded by ECHO_TIMEOUT_SECONDS.
#   retry  — when the signal never came, resend ONCE. This retries
#            DELIVERY only: the expected-output waits (wait_for) never
#            retry, so a real product failure still fails the gate.
#   anchor — send_line RETURNS only after the command completed: a new
#            prompt-ish row below the typed line (see the ANCHOR_*
#            constants and await_output_anchor below). Sequencing, not
#            delivery: it closes the J6 gluing window where send N+1
#            landed while send N was still executing.
QUIESCE_POLL_SECONDS = 0.15
QUIESCE_TIMEOUT_SECONDS = 15.0
ECHO_TIMEOUT_SECONDS = 10.0
ENTER_ACK_SECONDS = 3.0
WAKE_GAP_SECONDS = 0.15
# ── Output-anchored sequencing (release run 37153503706, J6) ─────────────────
# `echo J6_ALIVE` and the NEXT line executed as ONE glued command
# (`❯ echo J6_ALIVEniu plugin trust bash-completion`): the second send
# landed while the first was still being processed, the wake Ctrl-U was
# the clock-repaint's eaten byte, and the text appended to the unsubmitted
# input line. The wt67 settle cannot close that window — the typed input
# line ITSELF starts with the prompt glyph, so PROMPTISH_LAST_ROW matches
# it during execution and `wait_quiescent` reports "prompt" before the
# prompt has returned. So between two consecutive REPL sends, send_line
# returns only when the command it typed has COMPLETED: a NEW prompt-ish
# last row that is neither the typed input line nor any row carrying the
# command text (anchor condition in await_output_anchor). Bounded like
# everything here; the known-long journey commands carry their own bound
# (trust 90s, source 60s) and an expiry is ledgered, never silent.
ANCHOR_POLL_SECONDS = 0.1
ANCHOR_TIMEOUT_SECONDS = 30.0
# Stress harness (hidden --stress-delay-ms): a randomized pause before
# every send's settle check, simulating runner slowness — the shape that
# broke two release runs. 0 disables (normal mode).
STRESS_DELAY_MS = 0
# Readline kill-line: the REPL editor's first input byte after the themed
# prompt has been idle is EATEN (observed 2026-10-03, run drv-run1: `echo`
# renders as `❯ cho`, `niu` as `❯ iu`, `source` as `❯ ource` — the prompt's
# per-second clock repaint consumes one queued key). Ctrl-U is the
# sacrificial wake byte: eaten -> the line behind it lands intact; not
# eaten -> it clears the (empty or stale) input line and the line still
# lands intact. Verified against the release build: Ctrl-U + line executes
# clean; double Ctrl-U + line (the retry shape) also executes clean.
KILL_LINE = "\x15"
# The wake-flag capability (journey-spec W1, P3-S3 / niu#167): the gate's
# sends normally carry the sacrificial Ctrl-U above, which MASKS the
# product's first-key-eat behavior. P3-S3 runs one labeled no-wake probe;
# when the product passes it (the 1.3.3 typeahead guard), the probe flips
# this flag and every LATER send in the run goes wake-free too — the gate
# stops masking. When the probe fails, the flag stays True so one live
# product bug does not drown the rest of the journey in eaten bytes.
WAKE_ENABLED = True
# A REPL prompt is the classic "last non-empty row ends in a prompt
# glyph" (powerline tails, the default user@host:cwd# $ #) or the
# agnoster shape where the glyph leads the row (➜  dirname). An early
# exit here is only a heuristic miss on a lookalike help line — the
# echo/change confirmation behind it still decides whether the key was
# delivered.
PROMPTISH_LAST_ROW = re.compile(
    r"(?:.*[$#%>❯➜▶►»❮]\s*$|\s*[❯➜▶►»➤⮞❮])")

COLS, ROWS = 120, 36

# Network clones are the real thing; give them real time.
CLONE_TIMEOUT = 900
STARTUP_TIMEOUT = 120

# Registered known-fails. A match keeps the gate RED (the gate must tell
# the truth) but labels the verdict with the owning ticket so the failure
# is actionable rather than mysterious.
KNOWN_FAILS = [
    {
        "id": "wt56-bash-completion-syntax",
        "ticket": "lane wt56 (alias family)",
        "pattern": r"bash_completion[^\n]*line\s+\d+",
        "note": "syntax error while sourcing bash_completion (the owner hit "
                "bash_completion:1376 on 2026-10-03, from the alias family; "
                "gate-observed 2026-10-03: 'syntax error in conditional "
                "expression: unexpected token ../' at line 1376)",
    },
    {
        "id": "bash-preexec-recipe-entry",
        "ticket": "recipe seed: entry file vs upstream layout",
        "pattern": r"bash-preexec[^:\n]*: entry file 'bash-preexec' not found",
        "note": "the 'full' collection's bash-preexec recipe names entry "
                "'bash-preexec' but upstream rcaloras/bash-preexec ships "
                "'bash-preexec.sh' — the apply reports '1 entries failed'",
    },
    {
        "id": "wt73-168-theme-rebound",
        "ticket": "niu#168 (wt72 fix lane): theme rebound across source/new "
                  "terminals",
        "pattern": r"theme re-?bound \(#168\)|selection materialized",
        "note": "the OPEN #168 P0: sync's 'selection materialized' rewrite "
                "clobbers the user's picked theme across source/new "
                "terminals. Evidence shapes this pattern labels: rc bytes "
                "not identical across session opens, the picked theme's "
                "variable (OSH_THEME/BASH_IT_THEME) re-pointed or flipped "
                "to the other framework, or the literal rewrite tell "
                "printed during an unchanged-spec source/startup. "
                "Expected-red until wt72 lands; labeling, never waiving "
                "(journey-spec P3-S2).",
    },
]


def now_utc() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def stress_pause():
    """The hidden --stress-delay-ms harness: a randomized pause before a
    send's settle check, stretching exactly the timing window a slow
    runner stretches. No-op in normal mode (STRESS_DELAY_MS == 0)."""
    if STRESS_DELAY_MS > 0:
        time.sleep(random.uniform(0, STRESS_DELAY_MS) / 1000.0)


def render_history_line(line, columns: int) -> str:
    """One pyte history row to text. Newer pyte stores rows as
    char-maps (`StaticDefaultDict[int, Char]`), older as mappings too —
    either way `Char.data` is the glyph and the default is a space."""
    if isinstance(line, str):  # future pyte handing us rendered text
        return line.rstrip()
    chars = []
    for column in range(columns):
        char = line.get(column)
        data = getattr(char, "data", " ") or " "
        chars.append(data)
    return "".join(chars).rstrip()


def make_history_screen(cols: int, rows: int) -> "pyte.HistoryScreen":
    """pyte's HistoryScreen kwarg moved between versions (`top=` -> 
    `history=`); accept either so the gate runs on the pyte a machine
    actually has."""
    try:
        return pyte.HistoryScreen(cols, rows, history=600)
    except TypeError:  # older pyte: top/bottom deques
        return pyte.HistoryScreen(cols, rows, top=600)


# ── The ConPTY session (the proven Session pattern, plus full transcripts) ──
class Session:
    """A niu.exe under ConPTY with a pyte-parsed screen and a running
    capture of every raw chunk. The RAW stream is the truth: a screen
    clear (the about-tour's \\x1b[2J) erases unscrolled lines from pyte's
    view forever, so waits fall back to the raw stream and the artifacts
    keep the raw bytes alongside the rendered transcripts."""

    def __init__(self, argv, cwd, env, cols=COLS, rows=ROWS, raw_log=None,
                 label="session", delivery_log=None):
        self.label = label
        self.delivery_log = delivery_log if delivery_log is not None else []
        self.dead = False  # the child exited and took the pty with it
        self.proc = PtyProcess.spawn(argv, cwd=str(cwd), env=env,
                                     dimensions=(rows, cols))
        # HistoryScreen keeps scrolled-off lines so the artifacts hold FULL
        # captures, not just the final viewport.
        self.screen = make_history_screen(cols, rows)
        self.stream = pyte.Stream(self.screen)
        self.raw_chunks = []
        self._lock = threading.Lock()
        self._raw_file = open(raw_log, "wb") if raw_log else None
        self._reader = threading.Thread(target=self._pump, daemon=True)
        self._reader.start()

    def _pump(self):
        while self.proc.isalive():
            try:
                data = self.proc.read()
            except Exception:
                break
            if data:
                with self._lock:
                    self.raw_chunks.append(data)
                    if self._raw_file:
                        self._raw_file.write(data.encode("utf-8",
                                                         errors="replace"))
                self.stream.feed(data)
        self.dead = True

    def _write(self, data) -> bool:
        """One guarded ConPTY write. A child that dies mid-journey closes
        the pty out from under the driver (observed 2026-10-04 run 4:
        niu.exe children vanishing while sibling lanes ran their own
        cleanup — a by-name taskkill looks identical to a product crash
        here); the journey must then record an honest red and STILL seal
        a verdict, never crash unsealed."""
        if self.dead:
            return False
        try:
            self.proc.write(data)
            return True
        except (EOFError, OSError) as err:
            self.dead = True
            self._delivery_event(
                "pty-write", str(data)[:20], 1,
                f"pty closed (child exited): {err}", "child-exited")
            return False

    def text(self) -> str:
        """The current viewport (what a user sees right now)."""
        return "\n".join(self.screen.display)

    def raw_text(self) -> str:
        with self._lock:
            return "".join(self.raw_chunks)

    def transcript(self) -> str:
        """Scrollback plus viewport — the full story of the session."""
        history = [render_history_line(line, self.screen.columns)
                   for line in self.screen.history.top]
        return "\n".join(history + self.screen.display)

    def raw_stripped(self) -> str:
        """The raw byte stream with ANSI escape sequences removed — every
        line the terminal ever received, even ones a later screen clear
        erased from pyte's model."""
        raw = self.raw_text()
        raw = re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", raw)
        raw = raw.replace("\r\n", "\n").replace("\r", "")
        return raw

    def wait_for(self, *needles, timeout=60, abort_on=()):
        """Wait until a needle shows on the viewport or in the raw stream
        (screen clears hide text from pyte, never from the bytes).
        `abort_on` needles (viewport only) fail fast: they mean the flow
        moved past what we were waiting for."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.dead:
                raise TimeoutError(
                    f"pty closed (child exited) while waiting for {needles};"
                    f" screen:\n{self.text()}")
            body = self.text()
            for needle in needles:
                if needle in body or needle in self.raw_text():
                    return needle
            for gone in abort_on:
                if gone in body:
                    raise TimeoutError(
                        f"flow moved on (saw {gone!r}) while waiting for "
                        f"{needles}; screen:\n{body}")
            time.sleep(0.05)
        raise TimeoutError(
            f"timed out ({timeout}s) waiting for {needles}; screen:\n{self.text()}"
        )

    def _raw_pulse(self) -> int:
        """How many chunks the terminal has emitted so far — a cheap
        'bytes are still arriving' signal for settle-before-send."""
        with self._lock:
            return len(self.raw_chunks)

    def last_nonempty_row(self) -> str:
        for row in reversed(self.screen.display):
            if row.strip():
                return row.rstrip()
        return ""

    def wait_quiescent(self, timeout=QUIESCE_TIMEOUT_SECONDS) -> str:
        """Settle-before-send. Returns 'quiescent' once two polls saw no
        viewport change and no new bytes, 'prompt' when the last
        non-empty row already looks like a REPL prompt (a themed prompt
        with a clock/spinner never goes fully quiet), or 'timeout' when
        the bound expired. Bounded and best-effort: the caller sends
        regardless, and the send's own echo/change confirmation is what
        decides whether delivery needs a retry.

        KNOWN HOLE this method cannot close on its own: the typed input
        line itself starts with the prompt glyph, so 'prompt' can fire
        while a command is still executing (release run 37153503706 —
        the J6 gluing). send_line closes it with await_output_anchor
        after its Enter; first-sends (no predecessor command) have no
        gluing to prevent."""
        stress_pause()
        deadline = time.time() + timeout
        prev_screen = None
        prev_pulse = None
        while time.time() < deadline:
            screen = self.text()
            pulse = self._raw_pulse()
            # prev_pulse must be nonzero: a session that has never emitted
            # a byte is not settled, it is not started yet — typing into
            # that window is exactly the startup-drain loss (37149660449).
            if (prev_pulse and prev_screen is not None
                    and screen == prev_screen and pulse == prev_pulse):
                return "quiescent"
            if PROMPTISH_LAST_ROW.match(self.last_nonempty_row()):
                return "prompt"
            prev_screen, prev_pulse = screen, pulse
            time.sleep(QUIESCE_POLL_SECONDS)
        return "timeout"

    def _delivery_event(self, kind, keys, attempt, reason, action):
        """Record a delivery retry (or a give-up) for the verdict, so a
        future flake is diagnosable from the artifacts alone."""
        self.delivery_log.append({
            "utc": now_utc(),
            "session": self.label,
            "kind": kind,
            "keys": keys,
            "attempt": attempt,
            "reason": reason,
            "action": action,
        })

    def _await_echo(self, line, before, timeout=ECHO_TIMEOUT_SECONDS) -> bool:
        """Wait for the typed text to appear in the RENDERED viewport.
        The viewport is the only reliable surface: the editor
        syntax-highlights the input line, so the raw bytes interleave SGR
        codes INSIDE the word (`\\x1b[36mecho\\x1b[0m DUM...`) and a raw
        substring test can never match; pyte's rendered row holds the
        glyphs contiguously. `before` is the pre-send viewport — a match
        that was already on screen is not an echo (the awaiting-trust
        nag spells `niu plugin trust bash-completion`; drv-run1's
        raw-stream false confirm hid the eaten first byte)."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            body = self.text()
            if line in body and line not in before:
                return True
            time.sleep(0.1)
        return False

    def _await_change(self, before, timeout=ECHO_TIMEOUT_SECONDS) -> bool:
        """Wait for ANY viewport change — the delivery signal for menu
        keys, which read raw and never echo: a delivered answer always
        advances the screen (next question, gallery, clones)."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.text() != before:
                return True
            time.sleep(0.1)
        return False

    def await_output_anchor(self, line, input_row,
                            timeout=ANCHOR_TIMEOUT_SECONDS) -> bool:
        """Output-anchored sequencing: wait for the POST-EXECUTION prompt
        of a command send_line just submitted — a NEW prompt-ish last
        non-empty row that is neither the typed input line (`input_row`)
        nor any row carrying the command text (`line`). The input line
        itself starts with the prompt glyph, so PROMPTISH_LAST_ROW
        matches it DURING execution — that is exactly the J6 gluing
        (release run 37153503706: `echo J6_ALIVEniu plugin trust
        bash-completion` ran as ONE command because the next send landed
        before this command's prompt returned). What a human waits for —
        echo scrolled up, output printed, fresh prompt at the bottom —
        is what this waits for. The command-text guard also survives the
        theme clock's repaints (the repainted input line still carries
        the text, so it keeps being excluded until execution replaces
        it). Bounded: on expiry the caller proceeds anyway (the step's
        own expected-output wait rules) but the expiry is ledgered as an
        'anchor-wait' delivery event, never silent. Every anchor
        outcome is ledgered (with its elapsed time) — one line per REPL
        send is the sequencing trace a future CI flake needs."""
        started = time.time()
        deadline = started + timeout
        while time.time() < deadline:
            row = self.last_nonempty_row()
            if (row and row != input_row and line not in row
                    and PROMPTISH_LAST_ROW.match(row)):
                elapsed = time.time() - started
                self._delivery_event(
                    "anchor-wait", line, 1,
                    f"post-execution prompt for {line!r} anchored after "
                    f"{elapsed:.1f}s — safe to send the next line",
                    "anchored")
                return True
            time.sleep(ANCHOR_POLL_SECONDS)
        self._delivery_event(
            "anchor-wait", line, 1,
            f"no post-execution prompt within {timeout}s of {line!r} — "
            "proceeding; the step's own expected-output wait will rule",
            "anchor-timeout")
        return False

    def answer(self, keys):
        """Settle out the menu draw, then press; digit and Enter split.

        Delivery-verified like send_line, but menus give no echo, so the
        signal is that the screen CHANGED after the press; one resend
        when nothing moved. The gallery's DOWN walk deliberately does
        NOT go through this per key — a repaint can lag a delivered
        arrow, a resent arrow can overshoot the row the walk just
        verified, and the walk already self-heals by polling the
        highlight."""
        for attempt in (1, 2):
            settle = self.wait_quiescent()
            before = self.text()
            if keys.endswith(ENTER) and len(keys) > 1:
                if not self._write(keys[:-1]):
                    return
                time.sleep(ENTER_GAP_SECONDS)
                self._write(ENTER)
            else:
                if not self._write(keys):
                    return
            if self._await_change(before):
                return
            if attempt == 1:
                self._delivery_event(
                    "answer", keys, 2,
                    f"screen unchanged {ECHO_TIMEOUT_SECONDS}s after the "
                    f"press (settle={settle}) — resending once", "resend")
            else:
                self._delivery_event(
                    "answer", keys, 2,
                    "screen still unchanged after the resend — proceeding; "
                    "the step's own wait will rule",
                    "undelivered")

    def send_line(self, line, anchor_timeout=ANCHOR_TIMEOUT_SECONDS,
                  wake=None):
        """Type a whole command at the REPL prompt, then Enter — and do
        not return until the command has COMPLETED.

        Delivery-verified (release run 37149660449, J6: keystrokes typed
        while startup notices were still landing never reached the
        ConPTY — no echo, 90s timeout; and drv-run1: the themed prompt's
        clock repaint eats the FIRST input byte after idle). Protocol,
        per attempt: kill-line first (sacrificial wake byte + clear of
        any stale input), then the text, then wait for the text to come
        back as NEW bytes; a failed confirm resends ONCE — the retry's
        kill-line also wipes a late-landing first attempt, so a resend
        can no longer concatenate two half-lines. The Enter gets its own
        short confirm (its scroll is immediate). Delivery retries only —
        the expected-output wait that follows never retries.

        wake (the W1 wake-flag capability, journey-spec P3-S3 / niu#167):
        `wake=False` sends the line WITHOUT the sacrificial Ctrl-U and
        WITHOUT the one resend — the #167 probe must observe the
        product's raw first-key behavior (a retry, with or without a
        wake byte, would mask exactly what this send exists to test),
        and without the kill-line a resend could concatenate with a
        half-delivered first line. `wake=None` (default) follows the
        global WAKE_ENABLED, which P3-S3 flips off for the rest of the
        run when the product passes the probe.

        Output-anchored (release run 37153503706, J6): after the Enter,
        await_output_anchor holds this send until a NEW prompt-ish row
        sits below the typed line — so the NEXT send in this session can
        never land while this command is still executing (the gluing).
        `anchor_timeout` carries the known-long commands' own bounds
        (trust 90s, source 60s); the default covers everything else."""
        use_wake = WAKE_ENABLED if wake is None else bool(wake)
        settle = self.wait_quiescent()
        for attempt in ((1,) if not use_wake else (1, 2)):
            before = self.text()
            if use_wake:
                self._write(KILL_LINE)
                time.sleep(WAKE_GAP_SECONDS)
            if not self._write(line):
                return
            if self._await_echo(line, before):
                break
            if not use_wake:
                self._delivery_event(
                    "send_line", line, 1,
                    f"no echo of the typed text within "
                    f"{ECHO_TIMEOUT_SECONDS}s (wake disabled, "
                    f"settle={settle}) — no retry by design (a no-wake "
                    "send must not mask the first-key behavior it "
                    "probes)", "no-wake-no-retry")
                break
            if attempt == 1:
                self.wait_quiescent(timeout=5.0)  # do not retype mid-drain
                self._delivery_event(
                    "send_line", line, 2,
                    f"no echo of the typed text within "
                    f"{ECHO_TIMEOUT_SECONDS}s (settle={settle}) — "
                    "resending once", "resend")
            else:
                self._delivery_event(
                    "send_line", line, 2,
                    "echo never appeared after the resend — proceeding; "
                    "the step's own wait will rule",
                    "undelivered")
        time.sleep(ENTER_GAP_SECONDS)
        before = self.text()
        # The input line is the bottom-most content while editing; this
        # snapshot is what the anchor must see REPLACED by a new prompt.
        input_row = self.last_nonempty_row()
        self._write(ENTER)
        if not self._await_change(before, timeout=ENTER_ACK_SECONDS):
            self._delivery_event(
                "send_line-enter", line, 2,
                f"screen did not advance within {ENTER_ACK_SECONDS}s of "
                "Enter — resending Enter once", "resend")
            self._write(ENTER)
        self.await_output_anchor(line, input_row, timeout=anchor_timeout)

    def close(self):
        try:
            self.proc.terminate(force=True)
        except Exception:
            pass
        if self._raw_file:
            try:
                self._raw_file.close()
            except Exception:
                pass


# ── Verdict recording ───────────────────────────────────────────────────────
class Verdict:
    """Per-step assertions + full screen transcripts -> verdict.{json,txt}."""

    def __init__(self, artifacts: Path):
        self.artifacts = artifacts
        self.transcripts = artifacts / "transcripts"
        self.transcripts.mkdir(parents=True, exist_ok=True)
        self.steps = []
        # Input-delivery retries from every Session, merged into the
        # verdict so a future ConPTY flake is diagnosable from the
        # artifacts alone (raw-stream format itself is unchanged).
        self.delivery_events = []

    def step(self, step_id, title):
        return StepRecorder(self, step_id, title)

    def seal(self) -> str:
        worst = "pass"
        for step in self.steps:
            if step["status"] == "fail":
                worst = "fail"
                break
            if step["status"] == "known-fail":
                worst = "known-fail"
        summary = {
            "gate": "golden-user-journey",
            "finished_utc": now_utc(),
            "result": worst,
            "steps": self.steps,
            # Input-delivery retry ledger: keys that had to be resent (or
            # never confirmed) on the ConPTY bridge. Delivery retries are
            # never a waiver — if the product failed, the step assertions
            # above still rule — but they separate "the keystroke never
            # landed" (a runner/window problem) from "the product said
            # so" (a real red).
            "delivery": self.delivery_events,
        }
        (self.artifacts / "verdict.json").write_text(
            json.dumps(summary, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8", newline="\n")
        lines = [f"GOLDEN JOURNEY VERDICT: {worst.upper()}", ""]
        for step in self.steps:
            mark = {"pass": "PASS", "fail": "FAIL",
                    "known-fail": "KNOWN-FAIL",
                    "blocked": "BLOCKED", "skip": "SKIP"}[step["status"]]
            lines.append(f"[{mark}] {step['id']} {step['title']}")
            for assertion in step["assertions"]:
                flag = "ok " if assertion["ok"] else "BAD"
                detail = f" — {assertion['detail']}" if assertion.get("detail") else ""
                lines.append(f"    {flag}  {assertion['name']}{detail}")
            for note in step.get("notes", []):
                lines.append(f"    ·    {note}")
        if self.delivery_events:
            lines.append("")
            lines.append("INPUT DELIVERY EVENTS (ConPTY keystroke retries +")
            lines.append("anchor waits — delivery/sequencing only; "
                         "expected-output waits never retry):")
            for event in self.delivery_events:
                lines.append(
                    f"    [{event['action']}] {event['session']} "
                    f"{event['kind']} attempt {event['attempt']} "
                    f"{event['keys']!r} — {event['reason']}")
        (self.artifacts / "verdict.txt").write_text(
            "\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        return worst

    def capture(self, name: str, session: Session):
        (self.transcripts / f"{name}.txt").write_text(
            f"=== {name} @ {now_utc()} ===\n{session.transcript()}\n",
            encoding="utf-8", newline="\n")


class StepRecorder:
    def __init__(self, verdict: Verdict, step_id: str, title: str):
        self.verdict = verdict
        self.record = {
            "id": step_id,
            "title": title,
            "started": now_utc(),
            "status": "pass",
            "assertions": [],
            "notes": [],
        }
        self._failed = False
        self._known_failed = False

    def check(self, name: str, ok, detail: str = "", known=False) -> bool:
        """Record one assertion. `known=True` marks a registered
        known-fail: still RED (recorded ok=False) but labeled — and it
        does not BLOCK later journey steps the way an unknown fail does."""
        ok = bool(ok)
        self.record["assertions"].append(
            {"name": name, "ok": ok, "detail": detail, "known": known})
        if not ok:
            if known:
                self._known_failed = True
            else:
                self._failed = True
        return ok

    def note(self, text: str):
        self.record["notes"].append(text)

    def finish(self, status: str = None):
        if status:
            self.record["status"] = status
        elif self._failed:
            self.record["status"] = "fail"
        elif self._known_failed:
            # Red (the gate must tell the truth) but not blocking: the
            # journey continues past a labeled known-fail.
            self.record["status"] = "known-fail"
        else:
            self.record["status"] = "pass"
        self.record["finished"] = now_utc()
        self.verdict.steps.append(self.record)


def match_known_fail(text: str):
    """The first registered known-fail whose pattern matches, if any."""
    for known in KNOWN_FAILS:
        if re.search(known["pattern"], text, re.IGNORECASE):
            return known
    return None


def syntax_error_lines(body: str):
    return [line.strip() for line in body.splitlines()
            if "syntax error" in line.lower()]


def all_syntax_errors(session: Session):
    """Syntax-error lines from the rendered transcript AND the raw stream
    (a screen clear can erase an error from pyte's model before anyone
    reads it)."""
    seen = []
    for line in syntax_error_lines(session.transcript()) + \
            syntax_error_lines(session.raw_stripped()):
        if line not in seen:
            seen.append(line)
    return seen


def verdict_for_syntax_errors(recorder: StepRecorder, name: str, errors):
    """Record one syntax-error assertion: pass, known-fail, or fail."""
    if not errors:
        recorder.check(name, True)
    elif (known := match_known_fail("\n".join(errors))):
        recorder.check(name, False,
                       f"KNOWN-FAIL {known['id']} ({known['ticket']}): "
                       f"{errors[0]} — {known['note']}")
        recorder.note(f"expected-red until {known['ticket']} lands")
    else:
        recorder.check(name, False,
                       f"{len(errors)} syntax error line(s): {errors[:3]}")


def check_with_known_fail(recorder: StepRecorder, name: str, evidence: str):
    """Record an assertion whose absence-check FAILED: `evidence` is the
    offending text that was found (a failing install report, a syntax
    error, ...). This never records a pass — the caller records the pass
    when nothing was found. A registered known-fail turns the red into a
    labeled one; an unregistered one stays a plain fail."""
    lines = [line for line in evidence.splitlines() if line.strip()]
    known = match_known_fail(evidence)
    if known is not None:
        recorder.check(name, False,
                       f"KNOWN-FAIL {known['id']} ({known['ticket']}) — "
                       f"{known['note']}", known=True)
        recorder.note(f"expected-red until {known['ticket']} lands")
    else:
        recorder.check(name, False,
                       f"{len(lines)} offending line(s), e.g. "
                       f"{lines[0].strip()[:160] if lines else evidence[:160]}")


# ── The journey environment ─────────────────────────────────────────────────
def build_env(home: Path, exe: Path) -> dict:
    """The real user's terminal environment with state redirected into the
    sandbox: USERPROFILE and HOME both point there (USERPROFILE wins in
    this product — set both, leave no ambiguity; both must be ABSOLUTE —
    a relative home doubles inside niu's path joins), APPDATA/LOCALAPPDATA/
    TEMP/TMP too. PATH is the real user PATH with one journey fix: the
    tested exe's directory goes FIRST so `niu` resolves to the binary
    under test (a developer machine may carry an older niu on PATH, and
    the rc bootstrap line `niu plugin sync --bootstrap` must run THIS
    build). git (clones) and ls/cat (battery) come from the real PATH,
    exactly like the user's machine."""
    env = dict(os.environ)
    system_root = env.get("SystemRoot", r"C:\Windows")
    env.update({
        "SystemRoot": system_root,
        "COMSPEC": system_root + r"\System32\cmd.exe",
        "HOME": str(home),
        "USERPROFILE": str(home),
        "LOCALAPPDATA": str(home / "AppData" / "Local"),
        "APPDATA": str(home / "AppData" / "Roaming"),
        "TEMP": str(home / "tmp"),
        "TMP": str(home / "tmp"),
        "TERM": "xterm",
        # Deterministic, assertable wizard language (the strings this gate
        # waits for are the English ones).
        "NIU_LANG": "en",
        # The update hint is environment noise, not part of the journey.
        "NIU_NO_UPDATE_CHECK": "1",
    })
    # Ambient overrides from a developer shell must not leak into the user
    # journey: niu state locations must resolve purely from HOME.
    for key in ("NIU_PLUGIN_SOURCES_ROOT", "NIU_PLUGIN_SPEC", "NIU_MIRRORS",
                "NIU_PLUGIN_BOOTSTRAP", "NIU_REPL_STARTUP", "BASH_ENV",
                "WINUXSH_ROOT", "NIU_THEME", "OSH_THEME", "BASH_IT_THEME"):
        env.pop(key, None)
    env["PATH"] = os.pathsep.join(
        [str(exe.parent)] + [ensure_tools_on_path(env.get("PATH", ""))])
    (home / "tmp").mkdir(parents=True, exist_ok=True)
    (home / "AppData" / "Local").mkdir(parents=True, exist_ok=True)
    (home / "AppData" / "Roaming").mkdir(parents=True, exist_ok=True)
    return env


def ensure_tools_on_path(path: str) -> str:
    """The journey needs `git` (clones) and `ls`/`cat` (battery) on PATH.
    On a runner only `Git\cmd` may be present; add the Git for Windows
    layout pieces that exist. A user machine has WinuxCmd and needs none
    of this."""
    extras = []
    if shutil.which("git") is None:
        for guess in (r"C:\Program Files\Git\cmd",
                      r"C:\Program Files\Git\bin"):
            if Path(guess).joinpath("git.exe").is_file():
                extras.append(guess)
                break
    if shutil.which("ls") is None:
        usr_bin = r"C:\Program Files\Git\usr\bin"
        if Path(usr_bin).joinpath("ls.exe").is_file():
            extras.append(usr_bin)
    return path + os.pathsep.join([""] + extras) if extras else path


def prompt_alive(session: Session, marker: str, timeout=STARTUP_TIMEOUT):
    """The prompt is alive: `echo <marker>` comes back as output."""
    if session.dead:
        return False
    session.send_line(f"echo {marker}")
    try:
        session.wait_for(marker, timeout=timeout)
        time.sleep(0.3)
        return True
    except TimeoutError:
        return False


def drain_notices(session: Session, seconds: float = 1.0):
    """Let late stderr (startup sync notices) land before reading."""
    time.sleep(seconds)


# ── W1 driver capabilities (journey-spec §4: wake-flag, seed) ────────────────
PROMPT_BLOCK_ROWS = 4


def prompt_block(session: Session):
    """The prompt BLOCK as the user sees it: the last non-empty viewport
    rows (a themed prompt paints several — powerline-multiline carries
    the clock on its top segment row). Returns the rows newest-last, or
    None before the first prompt has rendered."""
    if not (session._raw_pulse()
            and PROMPTISH_LAST_ROW.match(session.last_nonempty_row())):
        return None
    rows = [row for row in session.text().splitlines() if row.strip()]
    return rows[-PROMPT_BLOCK_ROWS:]


def await_first_prompt_row(session: Session, timeout=STARTUP_TIMEOUT):
    """The terminal's FIRST prompt block, captured before anything is
    typed: poll until bytes have arrived and the last non-empty row looks
    like a prompt, let trailing startup notices land, then read it.
    Returns the digit-stripped fingerprint (the clock must not count) of
    the block joined with '|', or None on timeout."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        block = prompt_block(session)
        if block is not None:
            time.sleep(1.0)  # trailing startup notices, then re-read
            block = prompt_block(session)
            if block is not None:
                return "|".join(normalize_prompt_row(row)
                                for row in block)
        time.sleep(0.05)
    return None


def time_to_first_prompt(session: Session, timeout, started=None):
    """Seconds from `started` (default: now) to the first prompt-ish row —
    the P3-S4 bounded-startup measure (prompt within 10s). None on
    timeout."""
    start = started if started is not None else time.time()
    deadline = start + timeout
    while time.time() < deadline:
        if (session._raw_pulse()
                and PROMPTISH_LAST_ROW.match(session.last_nonempty_row())):
            return time.time() - start
        time.sleep(0.05)
    return None


def normalize_prompt_row(row):
    """A prompt row modulo its clock: the themed prompt repaints a
    per-second clock, so P3-S2(d) compares rows with every digit run
    stripped."""
    return re.sub(r"\d", "", row or "")


def await_bare_line(session: Session, text, timeout=30):
    """True when `text` arrives as an OUTPUT line (viewport or
    ANSI-stripped raw stream) within the bound. The typed input line also
    carries the text — behind the prompt glyph — so the match is a line
    that STARTS with the text: `echo X`'s output row and `cat`'s output
    both lead with the payload, while the input echo leads with the
    prompt glyph (the distinction the #167 probe lives on). Prefix (not
    full-line) matching on purpose: stray console noise (observed:
    tasklist's 'No Instance(s) Available.' from niu's own startup probes)
    can land on the same physical row right after the payload with no
    newline in between."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        for line in session.text().splitlines():
            if line.strip().startswith(text):
                return True
        for line in session.raw_stripped().splitlines():
            if line.strip().startswith(text):
                return True
        time.sleep(0.1)
    return False


def sync_row_keys(body: str, action: str):
    """Distinct source ids of `niu plugin sync: <action> <id> ...` rows in
    a mixed transcript+raw body. The same row appears twice (the pyte
    transcript AND the raw stream both carry it) and stray console noise
    can glue onto a physical line, so row identity is (action, id) —
    never the physical line count."""
    return {
        match.group(1)
        for match in re.finditer(
            rf"niu plugin sync: {re.escape(action)}\s+(\S+)", body)
    }


def first_diff_lines(old: bytes, new: bytes, limit=8):
    """The first changed lines between two rc snapshots — the verdict
    detail for a P3-S2 byte-stability red."""
    old_text = old.decode("utf-8", errors="replace").splitlines()
    new_text = new.decode("utf-8", errors="replace").splitlines()
    diffs = [line for line in difflib.unified_diff(
        old_text, new_text, lineterm="", n=0)
        if line[:1] in "+-" and line[:3] not in ("+++", "---")]
    return diffs[:limit]


def check_theme_stability(recorder: "StepRecorder", name: str, ok: bool,
                          evidence_shape: str, detail: str = "") -> bool:
    """Record one P3-S2 assertion (the #168 assertion class: rc
    byte-stability, theme identity, no-rewrite-tell, prompt identity).
    A red carries the evidence shape so the registered KNOWN-FAIL
    `wt73-168-theme-rebound` can label it expected-red (labeling, never
    waiving) — the raw diff stays in the detail and the artifacts."""
    ok = bool(ok)
    if ok:
        recorder.check(name, True)
        return True
    evidence = f"theme rebound (#168): {evidence_shape}"
    if detail:
        evidence += f" — {detail}"
    known = match_known_fail(evidence)
    if known is not None:
        recorder.check(
            name, False,
            f"KNOWN-FAIL {known['id']} ({known['ticket']}): {detail or evidence}",
            known=True)
        recorder.note(f"expected-red until {known['ticket']} lands")
    else:
        recorder.check(name, False, detail or evidence)
    return False


class SandboxSeed:
    """The `seed` capability (journey-spec §4 `conpty+seed`, owner lane
    W1): aged-state writes under the sandbox home BETWEEN sessions, so a
    phase starts from state a PRIOR session produced (or damaged) — never
    from fresh-wizard state. Every path stays inside the sandbox; the
    product reads these files with its normal startup/sync machinery.

    The previous-session marker (`~/.journey-wake.flag`) also lives here:
    a step's session writes it THROUGH THE SHELL and the next session's
    step reads it back the same way — a session detects "a previous
    session mutated state" with no driver-internal channel, and the
    sandbox HOME wiring is proven end-to-end across process boundaries
    (the relative-home doubling pitfall)."""

    def __init__(self, home: Path):
        self.home = home

    # ── paths ──
    @property
    def rc_path(self) -> Path:
        return self.home / ".niubashrc"

    @property
    def spec_path(self) -> Path:
        return self.home / ".niubash" / "plugins.toml"

    @property
    def sources_root(self) -> Path:
        return self.home / ".niubash" / "sources"

    @property
    def ledger_path(self) -> Path:
        """The startup install-failure memo (`bootstrap-failures.toml`,
        changelog 1.3.1 F5) — under the sources root, the product's own
        `bootstrap_failure_path()` location."""
        return self.sources_root / "bootstrap-failures.toml"

    def marker_path(self) -> Path:
        return self.home / ".journey-wake.flag"

    # ── snapshots ──
    def rc_bytes(self) -> bytes:
        try:
            return self.rc_path.read_bytes()
        except OSError:
            return b""

    def spec_text(self) -> str:
        try:
            return self.spec_path.read_text(encoding="utf-8")
        except OSError:
            return ""

    # ── spec hand-editing (the documented workflow: edit + sync) ──
    def add_spec_source(self, stanza_body: str):
        """Append one `[[sources]]` stanza (body lines without the
        header) to the spec — the same shape `niu plugin add` writes."""
        text = self.spec_text()
        if text and not text.endswith("\n"):
            text += "\n"
        text += "\n[[sources]]\n" + stanza_body.rstrip("\n") + "\n"
        self.spec_path.parent.mkdir(parents=True, exist_ok=True)
        self.spec_path.write_text(text, encoding="utf-8", newline="\n")

    def drop_spec_source(self, id_or_target: str) -> bool:
        """Remove every `[[sources]]` stanza whose text names
        `id_or_target` (its `id =` or `target =` line). Returns True when
        a stanza was dropped."""
        text = self.spec_text()
        stanzas = text.split("[[sources]]")
        kept = [stanzas[0]]
        dropped = False
        for stanza in stanzas[1:]:
            if id_or_target in stanza:
                dropped = True
                continue
            kept.append("[[sources]]" + stanza)
        if dropped:
            self.spec_path.write_text("".join(kept), encoding="utf-8",
                                      newline="\n")
        return dropped

    # ── the F5 ledger, in the product's exact format ──
    def seed_bootstrap_failure(self, target: str, error: str,
                               ref_name=None):
        """Write (or merge into) `bootstrap-failures.toml` exactly the way
        `sync.rs write_bootstrap_failures` formats a startup failure for
        (target, ref) — the shape `bootstrap_failure_recorded` matches
        against the resolved origin of a spec entry."""
        entry = f"\n[[failure]]\ntarget = \"{target}\"\n"
        if ref_name:
            entry += f"ref = \"{ref_name}\"\n"
        entry += (f"error = \"{error}\"\n"
                  f"at = \"{datetime.now(timezone.utc).isoformat(timespec='seconds')}\"\n")
        path = self.ledger_path
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.is_file():
            path.write_text(path.read_text(encoding="utf-8") + entry,
                            encoding="utf-8", newline="\n")
        else:
            path.write_text(
                "# Startup install-failure memo (niu plugin sync --bootstrap).\n"
                "# Cleared by an explicit `niu plugin sync` / `niu plugin add` retry.\n"
                "schema = \"niubash:plugin-bootstrap-failures@1\"\n" + entry,
                encoding="utf-8", newline="\n")

    # ── tree damage (the `tree missing` / guarded-loader contract) ──
    def damage_source_file(self, source_id: str, relative: str,
                           suffix=".journey-bak"):
        """Rename one file inside an installed source tree (aged-state
        damage a real user's disk can produce). Returns (old, new) paths;
        raises when the file is not there to damage."""
        old = self.sources_root / source_id / relative
        new = old.with_name(old.name + suffix)
        if not old.is_file():
            raise FileNotFoundError(f"cannot damage: {old} is missing")
        old.rename(new)
        return old, new


# ── Wave-1 persistence steps (journey-spec P3 + P8-S4, lane wt79/W1) ─────────
#
# Each step is the user's own moves over the SAME sandbox the J-blocks
# aged: real fresh terminals, real `niu plugin` verbs, and the seed
# capability writing between sessions. `state` threads the ordered facts
# (rc byte snapshots keyed by milestone, the picked theme line, the
# first themed prompt row) from the wizard block into the assertions.

P3_MARKER_FILE = ".journey-wake.flag"
P3_MARKER_VALUE = "wake-p3s1"


def p3_s1_reopen(exe, home, env, verdict, seed, state):
    """P3-S1: reopen the sandbox session — theme persists, spec persists,
    and the session detects a previous session's state mutation through
    the wake-flag marker file."""
    step = verdict.step(
        "P3-S1", "reopen sandbox session: theme persists, spec persists")
    session = None
    try:
        session = Session([str(exe)], home, env,
                          raw_log=verdict.transcripts / "P3-S1-reopen.raw.ansi",
                          label="P3-S1", delivery_log=verdict.delivery_events)
        row = await_first_prompt_row(session)
        step.check("fresh terminal: prompt renders", row is not None,
                   "no prompt-ish row ever rendered"
                   if row is None else f"first prompt row: {row.strip()[:100]}")
        rc_bytes = seed.rc_bytes()
        rc_text = rc_bytes.decode("utf-8", errors="replace")
        theme_line = state.get("theme_line")
        step.check("theme persists in the rc (the wizard's pick intact)",
                   bool(theme_line) and theme_line in rc_text,
                   f"expected {theme_line!r} in the rc"
                   if theme_line else "the wizard snapshot carried no theme "
                   "line (J2 carried the failure)")
        spec_text = seed.spec_text()
        step.check("spec persists (oh-my-bash declared, theme declared)",
                   "oh-my-bash" in spec_text
                   and "powerline-multiline" in spec_text,
                   f"spec exists: {seed.spec_path.is_file()}")
        # The wake-flag capability: THIS session writes the marker through
        # the shell; the NEXT session (P3-S2's) reads it back — a session
        # detects "a previous session mutated state" across the process
        # boundary, proving the sandbox HOME wiring end-to-end.
        session.send_line(
            f"printf '{P3_MARKER_VALUE}' > ~/{P3_MARKER_FILE}"
            f" && cat ~/{P3_MARKER_FILE}")
        step.check(
            "session wrote the wake-flag marker through the shell",
            await_bare_line(session, P3_MARKER_VALUE, timeout=30))
        verdict.capture("P3-S1-reopen", session)
        state["rc_bytes"]["after-reopen"] = rc_bytes
        state["raw_texts"]["P3-S1"] = session.raw_stripped()
    except Exception as err:  # noqa: BLE001 - a stalled walk is the fail
        step.check("P3-S1 walk completed", False, f"{err}")
        if session is not None:
            verdict.capture("P3-S1-stalled", session)
    finally:
        if session is not None:
            session.close()
    step.finish()


def p3_s2_rc_stability(exe, home, env, verdict, seed, state):
    """P3-S2: rc byte-stability + theme identity across terminals — the
    niu#168 killer. Expected-red until niu#168 lands; registered
    KNOWN-FAIL wt73-168-theme-rebound (labeling, never waiving)."""
    step = verdict.step(
        "P3-S2", "rc byte-stability + theme identity across terminals "
        "(niu#168)")
    theme_line = state.get("theme_line") or ""
    theme_var = state.get("theme_var") or "OSH_THEME"
    other_var = "BASH_IT_THEME" if theme_var == "OSH_THEME" else "OSH_THEME"
    baseline = state["rc_bytes"].get("after-wizard", b"")
    raws = state["raw_texts"]
    rows = {}
    try:
        # Live session: read back the previous session's marker, then the
        # unchanged-spec source.
        live = Session([str(exe)], home, env,
                       raw_log=verdict.transcripts / "P3-S2-source.raw.ansi",
                       label="P3-S2-live",
                       delivery_log=verdict.delivery_events)
        try:
            live.send_line(f"cat ~/{P3_MARKER_FILE}")
            step.check(
                "live session detects the previous session's wake-flag "
                "marker",
                await_bare_line(live, P3_MARKER_VALUE, timeout=30))
            live.send_line("source ~/.niubashrc", anchor_timeout=60)
            live.send_line("echo P3S2_SRC_DONE")
            try:
                live.wait_for("P3S2_SRC_DONE", timeout=60)
                step.check("source ~/.niubashrc completed", True)
            except TimeoutError as err:
                step.check("source ~/.niubashrc completed", False, str(err))
            drain_notices(live)
            verdict.capture("P3-S2-after-source", live)
            state["rc_bytes"]["after-source"] = seed.rc_bytes()
            raws["P3-S2-source"] = live.raw_stripped()
        finally:
            live.close()

        # Two fresh terminals (the #168 shape: the rebound shows up when
        # a NEW session's startup sync rewrites the rc).
        for i in (1, 2):
            term = Session(
                [str(exe)], home, env,
                raw_log=verdict.transcripts / f"P3-S2-terminal-{i}.raw.ansi",
                label=f"P3-S2-term{i}",
                delivery_log=verdict.delivery_events)
            try:
                rows[i] = await_first_prompt_row(term)
                drain_notices(term)
                verdict.capture(f"P3-S2-terminal-{i}", term)
                state["rc_bytes"][f"after-term{i}"] = seed.rc_bytes()
                raws[f"P3-S2-term{i}"] = term.raw_stripped()
            finally:
                term.close()

        # (a) rc bytes identical after every source/terminal.
        for milestone in ("after-reopen", "after-source", "after-term1",
                          "after-term2"):
            snap = state["rc_bytes"].get(milestone)
            stable = bool(snap) and snap == baseline
            diff = "" if stable else " | ".join(
                first_diff_lines(baseline, snap or b""))
            check_theme_stability(
                step, f"rc bytes identical after {milestone}",
                stable, f"rc bytes diverged at {milestone}",
                diff or ("snapshot missing — the earlier step carrying it "
                         "failed" if snap is None else
                         "no line-level diff (mode/length change)"))

        # (b) the picked theme's variable still present, unchanged, on
        # the SAME framework (the rebound re-points or flips it).
        for milestone in ("after-source", "after-term1", "after-term2"):
            text = (state["rc_bytes"].get(milestone) or b"").decode(
                "utf-8", errors="replace")
            present = bool(theme_line) and theme_line in text
            flipped = re.search(
                rf"^{other_var}='powerline-multiline'",
                text, re.M) is not None
            check_theme_stability(
                step,
                f"{theme_var} still 'powerline-multiline' after "
                f"{milestone} (same framework)",
                present and not flipped,
                "the picked theme's variable was re-pointed or flipped to "
                "the other framework across session opens"
                if flipped or not present else "",
                f"{theme_line!r} present: {present}; "
                f"{other_var}='powerline-multiline' appeared: {flipped}")

        # (c) the #168 rewrite tell never appears during an
        # unchanged-spec source/startup.
        for label in sorted(raws):
            if not label.startswith(("P3-S1", "P3-S2")):
                continue
            raw = raws[label]
            tell = next((line.strip()[:140] for line in raw.splitlines()
                         if "selection materialized" in line), "")
            check_theme_stability(
                step,
                f"'selection materialized' never printed during {label}",
                "selection materialized" not in raw,
                "the #168 rewrite tell appeared during an unchanged-spec "
                "source/startup", tell)

        # (d) terminal 2's first prompt row equals terminal 1's modulo
        # clock digits.
        row1, row2 = rows.get(1), rows.get(2)
        if row1 is not None:
            state["themed_prompt_row"] = normalize_prompt_row(row1)
        same = (row1 is not None and row2 is not None
                and normalize_prompt_row(row1) == normalize_prompt_row(row2))
        check_theme_stability(
            step, "terminal 2's first prompt row equals terminal 1's "
            "(modulo clock digits)",
            same, "the first prompt row differs across fresh terminals",
            f"t1={(row1 or '(none)').strip()[:100]!r} "
            f"t2={(row2 or '(none)').strip()[:100]!r}")
    except Exception as err:  # noqa: BLE001 - a stalled walk is the fail
        step.check("P3-S2 walk completed", False, f"{err}")
    step.finish()


def p3_s3_first_key(exe, home, env, verdict, seed, state):
    """P3-S3: first-key integrity as product behavior — the niu#167
    anti-masking probe. One labeled no-wake probe; on a pass the run's
    global wake is disabled so the gate stops masking."""
    global WAKE_ENABLED
    step = verdict.step(
        "P3-S3", "first-key integrity as product behavior (niu#167 "
        "anti-masking)")
    marker = "NIU167KEY"
    session = None
    try:
        session = Session(
            [str(exe)], home, env,
            raw_log=verdict.transcripts / "P3-S3-first-key.raw.ansi",
            label="P3-S3", delivery_log=verdict.delivery_events)
        step.check("themed prompt renders",
                   prompt_alive(session, "P3S3_ALIVE"))
        # Idle past at least one clock repaint (the themed clock ticks at
        # 1 Hz) — the exact window where the #167 eat consumed the first
        # byte of the next command.
        time.sleep(1.6)
        session.send_line(f"echo {marker}", wake=False)
        executed = await_bare_line(session, marker, timeout=30)
        body = session.transcript() + "\n" + session.raw_stripped()
        eaten_lines = [line.strip()[:120] for line in body.splitlines()
                       if re.search(r"\bcho: command not found\b", line)]
        marker_lines = [line.strip()[:120] for line in body.splitlines()
                        if marker in line][:3]
        step.check("first key survives the idle repaint (full word ran)",
                   executed,
                   f"the marker never printed as output; screen showed: "
                   f"{marker_lines}")
        step.check("no eaten-first-byte evidence in the probe session",
                   not eaten_lines,
                   f"{len(eaten_lines)} line(s), first: "
                   f"{eaten_lines[0] if eaten_lines else ''}")
        verdict.capture("P3-S3-first-key", session)
        if executed and not eaten_lines:
            WAKE_ENABLED = False
            step.note("first-key integrity holds — WAKE_ENABLED flipped "
                      "off: every later send in this run goes without the "
                      "Ctrl-U wake (the gate stops masking #167)")
        else:
            step.note("probe FAILED — WAKE_ENABLED stays True so the rest "
                      "of the run still delivers (the wake stays for "
                      "everything else); this red is a #167 regression")
    except Exception as err:  # noqa: BLE001
        step.check("P3-S3 walk completed", False, f"{err}")
        if session is not None:
            verdict.capture("P3-S3-stalled", session)
    finally:
        if session is not None:
            session.close()
    step.finish()


def p3_s4_aged_state(exe, home, env, verdict, seed, state):
    """P3-S4: aged state — a damaged trusted tree plus a seeded F5
    bootstrap-failures ledger. Bounded startup, documented one-line
    notices, guarded-loader silence, explicit-verb repair, silent after."""
    step = verdict.step(
        "P3-S4", "aged state: damaged tree + bootstrap-failures ledger "
        "(changelog 1.3.1 F5)")
    try:
        # ── seed between sessions (the `seed` capability) ──
        missing = seed.home / ".journey-missing-origin"
        target = missing.as_posix()
        seed.add_spec_source(f'target = "{target}"\n'
                             'id = "journey-missing-origin"')
        seed.seed_bootstrap_failure(
            target=target,
            error="seeded by the journey (the previous session's install "
                  "failed)")
        step.check("bootstrap-failures.toml ledger seeded for the "
                   "declared-but-missing origin",
                   seed.ledger_path.is_file()
                   and target in seed.ledger_path.read_text(encoding="utf-8"),
                   f"ledger: {seed.ledger_path}")
        old, new = seed.damage_source_file("oh-my-bash", "oh-my-bash.sh")
        step.check("trusted tree damaged (the guarded loader's entry file "
                   "renamed)",
                   new.is_file() and not old.exists(),
                   f"{old.name} -> {new.name}")

        # ── fresh terminal: bounded startup, documented notices ──
        started = time.time()
        term = Session(
            [str(exe)], home, env,
            raw_log=verdict.transcripts / "P3-S4-damaged.raw.ansi",
            label="P3-S4-damaged", delivery_log=verdict.delivery_events)
        try:
            secs = time_to_first_prompt(term, timeout=10.0, started=started)
            step.check("prompt within 10s (the failed install is deferred, "
                       "not retried at startup)",
                       secs is not None,
                       "no prompt within 10s"
                       if secs is None else f"{secs:.1f}s to first prompt")
            prompt_alive(term, "P3S4_ALIVE")
            drain_notices(term)
            body = term.transcript() + "\n" + term.raw_stripped()
            deferred = sync_row_keys(body, "deferred")
            failed = sync_row_keys(body, "failed")
            degraded = sync_row_keys(body, "degraded") | {
                line.strip()[:60] for line in body.splitlines()
                if "tree missing" in line}
            nags = sync_row_keys(body, "awaiting-trust")
            first = next(iter(sorted(deferred | failed | degraded)),
                         "(none)")
            step.check("at most the documented one-line notices (the "
                       "deferred memo line; no failed/degraded rows)",
                       len(deferred) == 1 and not failed and not degraded,
                       f"deferred={sorted(deferred)} failed={sorted(failed)} "
                       f"degraded={sorted(degraded)}; first: "
                       f"{first.strip()[:140]}")
            step.check("awaiting-trust notices stay documented (<=2)",
                       len(nags) <= 2,
                       f"{len(nags)} source(s): {sorted(nags)}")
            storm = {line.strip()[:120] for line in body.splitlines()
                     if "command not found" in line}
            step.check("guarded loader no-ops silently (no error storm)",
                       not storm,
                       f"{len(storm)} line(s), first: "
                       f"{sorted(storm)[0] if storm else ''}")
            verdict_for_syntax_errors(
                step, "zero syntax errors with the damaged tree",
                all_syntax_errors(term))
            verdict.capture("P3-S4-damaged-terminal", term)

            # ── explicit verbs retry and repair ──
            term.send_line("niu plugin sync", anchor_timeout=300)
            try:
                term.wait_for("journey-missing-origin", timeout=120)
                retry_body = term.raw_stripped()
                # The interactive verb's rows have no `niu plugin sync:`
                # prefix (that is the bootstrap form): `  failed <id>  …`.
                readable = any(
                    re.search(r"failed\s+journey-missing-origin\b", line)
                    for line in retry_body.splitlines())
                step.check("explicit sync retries the missing origin "
                           "(readable per-source failure)", readable,
                           "the origin was named but no failed row carried "
                           "it")
            except TimeoutError as err:
                step.check("explicit sync retries the missing origin "
                           "(readable per-source failure)", False, str(err))
            # The restore fetches the pinned commit from the origin; on a
            # degraded network this is the long pole (the five TLS-burned
            # release runs), so it carries the largest bound in the group
            # and fails fast on the transport error instead of timing out.
            term.send_line("niu plugin restore oh-my-bash",
                           anchor_timeout=900)
            try:
                needle = term.wait_for("Restored source 'oh-my-bash'",
                                       "git clone exited", timeout=600)
                step.check("`niu plugin restore oh-my-bash` rebuilt the "
                           "pinned tree",
                           needle == "Restored source 'oh-my-bash'",
                           f"restore returned: {needle}")
            except TimeoutError as err:
                step.check("`niu plugin restore oh-my-bash` rebuilt the "
                           "pinned tree", False, str(err))
            guarded = seed.sources_root / "oh-my-bash" / "oh-my-bash.sh"
            step.check("the damaged file is back after restore (tree "
                       "matches the lockfile pin)", guarded.is_file(),
                       f"{guarded}")
            # Heal the spec the documented way (hand-edit the declaration
            # away); the ledger entry — already cleared by the explicit
            # retry's clear-before-install — is inert either way.
            dropped = seed.drop_spec_source(".journey-missing-origin")
            step.check("healed: the missing origin's declaration dropped "
                       "from the spec", dropped)
            step.note("the spec heal is the documented hand-edit workflow "
                      "(plugins-guide: 'you can equally hand-edit the "
                      "spec and run niu plugin sync')")
        finally:
            term.close()

        # ── the terminal after repair is silent ──
        started = time.time()
        healed = Session(
            [str(exe)], home, env,
            raw_log=verdict.transcripts / "P3-S4-healed.raw.ansi",
            label="P3-S4-healed", delivery_log=verdict.delivery_events)
        try:
            secs = time_to_first_prompt(healed, timeout=10.0, started=started)
            step.check("terminal after repair reaches a prompt within 10s",
                       secs is not None,
                       "no prompt within 10s"
                       if secs is None else f"{secs:.1f}s to first prompt")
            prompt_alive(healed, "P3S4B_ALIVE")
            drain_notices(healed)
            body = healed.transcript() + "\n" + healed.raw_stripped()
            bad = [line for line in body.splitlines()
                   if re.search(r"niu plugin sync: (deferred|failed|"
                                "degraded)\b", line)]
            step.check("terminal after repair is silent (no deferred/"
                       "failed/degraded rows)", not bad,
                       f"{len(bad)} line(s), first: "
                       f"{bad[0].strip()[:140] if bad else ''}")
            nags = sync_row_keys(body, "awaiting-trust")
            step.check("only the documented awaiting-trust lines remain",
                       len(nags) <= 2,
                       f"{len(nags)} source(s): {sorted(nags)}")
            verdict_for_syntax_errors(
                step, "zero syntax errors after repair",
                all_syntax_errors(healed))
            verdict.capture("P3-S4-healed-terminal", healed)
        finally:
            healed.close()
    except Exception as err:  # noqa: BLE001
        step.check("P3-S4 walk completed", False, f"{err}")
    step.finish()


def p8_s4_remove_active_theme_source(exe, home, env, verdict, seed, state):
    """P8-S4: `niu plugin source remove oh-my-bash` while its theme is
    applied — the F4 resurrection guard (spec declaration drops, no
    resurrection at the next startup) plus the degradation contract
    (guarded loader no-ops, floor prompt, no syntax-error storm), then
    re-add + re-enable restores the theme with no orphan rc blocks."""
    step = verdict.step(
        "P8-S4", "remove the source providing the ACTIVE theme "
        "(changelog 1.3.1 F4)")
    theme_line = state.get("theme_line") or ""
    tree = seed.sources_root / "oh-my-bash"
    registry_path = seed.sources_root / "registry.toml"
    try:
        rc_text = seed.rc_bytes().decode("utf-8", errors="replace")
        ready = (bool(theme_line) and theme_line in rc_text
                 and "oh-my-bash" in seed.spec_text() and tree.is_dir())
        step.check("precondition: oh-my-bash installed with the picked "
                   "theme active", ready,
                   f"theme line present: {theme_line in rc_text}; declared: "
                   f"{'oh-my-bash' in seed.spec_text()}; tree: {tree.is_dir()}")

        remover = Session(
            [str(exe)], home, env,
            raw_log=verdict.transcripts / "P8-S4-remove.raw.ansi",
            label="P8-S4-remove", delivery_log=verdict.delivery_events)
        try:
            prompt_alive(remover, "P8S4_ALIVE")
            # Self-heal first: the removal contract only deletes a tree it
            # can fingerprint (sources.rs remove_source verifies the
            # adapter layout), so a tree still damaged from P3-S4 (whose
            # restore is network-bound) is repaired to the pin before the
            # removal — the documented repair verb, then the removal.
            entry_file = tree / "oh-my-bash.sh"
            if not entry_file.is_file():
                step.note("tree still damaged from P3-S4 — running the "
                          "documented repair (`niu plugin restore "
                          "oh-my-bash`) before the removal")
                remover.send_line("niu plugin restore oh-my-bash",
                                  anchor_timeout=900)
                try:
                    needle = remover.wait_for("Restored source 'oh-my-bash'",
                                              "git clone exited", timeout=600)
                    step.check("repair before removal (network-bound)",
                               needle == "Restored source 'oh-my-bash'",
                               f"restore returned: {needle}")
                except TimeoutError as err:
                    step.check("repair before removal (network-bound)",
                               False, str(err))
            remover.send_line("niu plugin source remove oh-my-bash",
                              anchor_timeout=120)
            try:
                remover.wait_for("Removed source 'oh-my-bash'", timeout=90)
                step.check("`niu plugin source remove oh-my-bash` reported "
                           "removal", True)
            except TimeoutError as err:
                step.check("`niu plugin source remove oh-my-bash` reported "
                           "removal", False, str(err))
        finally:
            remover.close()

        # F4's resurrection guard, on disk, immediately.
        step.check("F4: the spec declaration dropped",
                   "oh-my-bash" not in seed.spec_text(),
                   "plugins.toml still names oh-my-bash")
        registry_text = (registry_path.read_text(encoding="utf-8")
                         if registry_path.is_file() else "")
        step.check("F4: the registry record dropped",
                   "oh-my-bash" not in registry_text,
                   "registry.toml still records oh-my-bash")
        step.check("F4: the tree is gone", not tree.exists(),
                   f"{tree} still present")

        # Fresh terminal: floor prompt, no resurrection, no error storm.
        floor = Session(
            [str(exe)], home, env,
            raw_log=verdict.transcripts / "P8-S4-floor.raw.ansi",
            label="P8-S4-floor", delivery_log=verdict.delivery_events)
        try:
            row = await_first_prompt_row(floor)
            step.check("fresh terminal without the theme source: prompt "
                       "renders (floor)", row is not None,
                       "no prompt-ish row ever rendered")
            drain_notices(floor)
            body = floor.transcript() + "\n" + floor.raw_stripped()
            step.check("no resurrection: no 'Cloning into' at the fresh "
                       "terminal", "Cloning into" not in body,
                       next((line.strip()[:120] for line in body.splitlines()
                             if "Cloning into" in line), ""))
            step.check("no resurrection: the spec still lacks oh-my-bash",
                       "oh-my-bash" not in seed.spec_text())
            verdict_for_syntax_errors(
                step, "no syntax-error storm after removal",
                all_syntax_errors(floor))
            themed = state.get("themed_prompt_row")
            if themed is not None and row is not None:
                step.check("prompt fell back to the floor (no longer the "
                           "themed row)",
                           normalize_prompt_row(row) != themed,
                           f"floor row: {row.strip()[:100]!r}")
            rc_after = seed.rc_bytes().decode("utf-8", errors="replace")
            orphan = ">>> niu source oh-my-bash" in rc_after
            state["orphan_block_after_fresh_terminal"] = orphan
            step.note(
                "rc block status after the removal + one fresh terminal: "
                + ("an orphan oh-my-bash block is still in the rc (inert: "
                   "its guarded loader no-ops on the missing tree); the "
                   "steady-state no-orphan gate is asserted after the "
                   "re-add below" if orphan else
                   "the block was dropped — no orphan remained"))
            verdict.capture("P8-S4-floor-terminal", floor)
        finally:
            floor.close()

        # Re-add + trust + re-enable (the manifest's restore path).
        restorer = Session(
            [str(exe)], home, env,
            raw_log=verdict.transcripts / "P8-S4-restore.raw.ansi",
            label="P8-S4-restore", delivery_log=verdict.delivery_events)
        try:
            prompt_alive(restorer, "P8S4B_ALIVE")
            # The re-clone is the one network-bound leg of the group (the
            # TLS-reset family burned five release runs): one bounded
            # retry when the add fails CLEANLY (a reported clone failure,
            # not a wedge) — a second failure rules. Delivery/transport
            # retry only; the step's assertions still decide the verdict.
            installed = None
            add_detail = ""
            for add_attempt in (1, 2):
                restorer.send_line("niu plugin add oh-my-bash",
                                   anchor_timeout=900)
                try:
                    needle = restorer.wait_for(
                        "Installed source 'oh-my-bash'",
                        "could not install 'oh-my-bash'", timeout=600)
                    installed = needle == "Installed source 'oh-my-bash'"
                    add_detail = f"attempt {add_attempt}: {needle}"
                except TimeoutError as err:
                    installed = None
                    add_detail = f"attempt {add_attempt}: {err}"
                if installed or restorer.dead:
                    break
                if add_attempt == 1 and installed is False:
                    step.note("the re-add clone failed (TLS/transport "
                              "reset family) — retrying the add once; a "
                              "second failure rules")
                else:
                    break
            step.check("`niu plugin add oh-my-bash` re-installed the "
                       "source", installed is True, add_detail)
            restorer.send_line("niu plugin trust oh-my-bash",
                               anchor_timeout=90)
            try:
                restorer.wait_for("is now trusted", timeout=90)
                step.check("`niu plugin trust oh-my-bash` reported trusted",
                           True)
            except TimeoutError as err:
                step.check("`niu plugin trust oh-my-bash` reported trusted",
                           False, str(err))
            restorer.send_line("niu plugin enable oh-my-bash/"
                               "powerline-multiline", anchor_timeout=120)
            try:
                restorer.wait_for("Enabled", timeout=90)
                step.check("`niu plugin enable "
                           "oh-my-bash/powerline-multiline` re-applied the "
                           "theme", True)
            except TimeoutError as err:
                step.check("`niu plugin enable "
                           "oh-my-bash/powerline-multiline` re-applied the "
                           "theme", False, str(err))
        finally:
            restorer.close()

        # Steady state: the theme is back and no orphan blocks remain.
        final = Session(
            [str(exe)], home, env,
            raw_log=verdict.transcripts / "P8-S4-restored.raw.ansi",
            label="P8-S4-restored", delivery_log=verdict.delivery_events)
        try:
            row = await_first_prompt_row(final)
            step.check("fresh terminal after re-add: prompt renders",
                       row is not None,
                       "no prompt-ish row ever rendered")
            drain_notices(final)
            verdict.capture("P8-S4-restored-terminal", final)
            rc_text = seed.rc_bytes().decode("utf-8", errors="replace")
            blocks = rc_text.count(">>> niu source oh-my-bash")
            step.check("theme restored: the picked theme line is active in "
                       "the rc again",
                       bool(theme_line) and theme_line in rc_text,
                       f"expected {theme_line!r}")
            step.check("no orphan blocks: exactly one oh-my-bash managed "
                       "block", blocks == 1,
                       f"{blocks} oh-my-bash block(s) in the rc")
            step.check("the spec declares oh-my-bash again",
                       "oh-my-bash" in seed.spec_text())
            verdict_for_syntax_errors(
                step, "zero syntax errors after the restore",
                all_syntax_errors(final))
            themed = state.get("themed_prompt_row")
            if themed is not None and row is not None:
                step.check("the themed prompt renders again",
                           normalize_prompt_row(row) == themed,
                           f"row now: {row.strip()[:100]!r}")
        finally:
            final.close()
    except Exception as err:  # noqa: BLE001
        step.check("P8-S4 walk completed", False, f"{err}")
    step.finish()


# ── The journey steps ───────────────────────────────────────────────────────
# Phase selection (the W1 wiring): `gate` is the legacy J1-J6 run,
# `full` (default) adds the wave-1 persistence group after J5 / before
# J6, and the per-phase selections reuse the same J-prefix for their
# aged state (P-phases compose after the wizard, journey-spec §3).
PHASES = {
    "full": {"p": ["P3-S1", "P3-S2", "P3-S3", "P3-S4", "P8-S4"], "j6": True},
    "gate": {"p": [], "j6": True},
    "persist": {"p": ["P3-S1", "P3-S2", "P3-S3", "P3-S4", "P8-S4"],
                "j6": False},
    "P3": {"p": ["P3-S1", "P3-S2", "P3-S3", "P3-S4"], "j6": False},
    "P8": {"p": ["P8-S4"], "j6": False},
}


def journey(exe: Path, root: Path, verdict: Verdict,
            phase: str = "full") -> str:
    selection = PHASES[phase]
    home = (root / "home").resolve()
    home.mkdir(parents=True, exist_ok=True)
    env = build_env(home, exe)
    seed = SandboxSeed(home)
    state = {
        "rc_bytes": {},      # milestone -> rc bytes (P3-S2's equality grid)
        "raw_texts": {},     # label -> ANSI-stripped raw stream
        "theme_line": None,  # the exact rc theme line the wizard wrote
        "theme_var": None,   # OSH_THEME / BASH_IT_THEME
        "themed_prompt_row": None,
        "marker_file": P3_MARKER_FILE,
        "marker_value": P3_MARKER_VALUE,
    }

    def block_rest(reason):
        later = ["J2", "J3", "J4", "J5"]
        later += selection["p"]
        if selection["j6"]:
            later.append("J6")
        for later_step in later:
            verdict.step(later_step,
                         f"{later_step} (blocked: {reason})").finish(
                             status="blocked")

    # ── J1 + J2 share one session: on a fresh install `niu` IS the wizard.
    step = verdict.step(
        "J1", "fresh-install first contact: v-banner + wizard")
    session = None
    j1_ok = True
    try:
        session = Session([str(exe)], home, env,
                           raw_log=verdict.transcripts / "J1J2.raw.ansi",
                           label="J1J2",
                           delivery_log=verdict.delivery_events)
    except Exception as err:  # noqa: BLE001 - any spawn failure is a J1 fail
        step.check("niu spawned under ConPTY", False, f"{err}")
        j1_ok = False
    if j1_ok:
        try:
            session.wait_for("Welcome to Niubash", timeout=90)
            step.check("welcome v-banner appears", True)
        except TimeoutError as err:
            step.check("welcome v-banner appears", False, str(err))
            j1_ok = False
        body = session.text()
        step.check("banner carries the version (v{CARGO_PKG_VERSION})",
                   re.search(r"v\d+\.\d+\.\d+", body) is not None)
        for needle, name in (
            ("Environment", "environment panel appears"),
            ("No external themes installed yet",
             "empty-ecosystem note (no external themes yet)"),
        ):
            try:
                session.wait_for(needle, timeout=30)
                step.check(name, True)
            except TimeoutError as err:
                step.check(name, False, str(err))
                j1_ok = False
        verdict.capture("J1-fresh-install", session)
    step.finish()

    if not j1_ok:
        if session is not None:
            session.close()
        block_rest("J1 failed")
        return verdict.seal()

    # ── J2: the wizard full run, same session.
    step = verdict.step(
        "J2", "wizard full run: full -> trust now -> powerline-multiline")
    try:
        try:
            session.wait_for("Plugin collection?", timeout=60)
            session.answer("4\r")  # 4 = full (both frameworks + hooks)
            step.check("collection menu answered with 'full'", True)
        except TimeoutError as err:
            raise AssertionError(f"collection question never appeared: {err}")

        try:
            session.wait_for("niu-git", timeout=60)
            session.answer(ENTER)  # default Skip
            step.check("niu-git question answered with Skip", True)
        except TimeoutError as err:
            raise AssertionError(f"niu-git question never appeared: {err}")

        try:
            session.wait_for("Apply this configuration?", timeout=60)
            session.answer(ENTER)  # Apply is the highlighted default
            step.check("Apply gate confirmed", True)
        except TimeoutError as err:
            raise AssertionError(f"Apply gate never appeared: {err}")

        # The real clones: git's own "Cloning into ..." lands on the
        # terminal (oh-my-bash, bash-it, bash-completion, bash-preexec).
        try:
            session.wait_for("Cloning into", timeout=CLONE_TIMEOUT)
            step.check("clone progress visible on the terminal", True)
        except TimeoutError as err:
            raise AssertionError(f"no clone ever started: {err}")

        # Singular (one theme-bearing source) or plural (several); the
        # 'full' collection installs two theme-bearing frameworks. Abort
        # on the tour/REPL: an install that failed outright never asks
        # this question, and the wait must fail fast with the real reason.
        try:
            session.wait_for("to list its themes?", "to list their themes?",
                             timeout=CLONE_TIMEOUT,
                             abort_on=("press any key to continue",
                                       "Type `about` for a quick tour"))
            clones = session.raw_text().count("Cloning into")
            step.check("multiple sources cloned (>=2 clone progress lines)",
                       clones >= 2, f"counted {clones} 'Cloning into' lines")
            verdict.capture("J2-after-clones", session)
            # Every collection entry must have landed (a failed entry is a
            # real gap — the user sees the failure line in their terminal).
            # Settle first: the question draws while git stderr is still
            # draining, then read the RAW stream only — it is append-only
            # and byte-ordered, immune to menu redraws and screen clears.
            time.sleep(1.0)
            body = session.raw_stripped()
            (verdict.transcripts / "J2-apply-report.txt").write_text(
                body, encoding="utf-8", newline="\n")
            failed_lines = [line.strip() for line in body.splitlines()
                            if re.search(r"\bfailed\b", line)]
            if not failed_lines:
                step.check("every collection entry installed (no 'failed' "
                           "lines)", True)
            else:
                check_with_known_fail(
                    step, "every collection entry installed (no 'failed' "
                    "lines)", "\n".join(failed_lines))
            session.answer("2\r")  # 2 = Trust now (现在信任)
            step.check("trust-now chosen (现在信任)", True)
        except TimeoutError as err:
            raise AssertionError(f"trust question never appeared: {err}")

        # Theme gallery: >60 themes, then navigate to powerline-multiline.
        # The size proof is deterministic, not a screen scrape: after
        # navigating, powerline-multiline's own row number (it sorts deep
        # in the list) proves the gallery size; a mid-redraw scroll marker
        # can read "0 more" and must never gate on its own.
        try:
            session.wait_for("Pick a theme", timeout=120)
            verdict.capture("J2-gallery", session)
        except TimeoutError as err:
            raise AssertionError(f"theme gallery never appeared: {err}")

        def highlight_row(target: str):
            for line in session.text().splitlines():
                if "◆" in line and target in line:
                    return line
            return None

        picked = False
        # Settle before the first arrow: the gallery's own draw drains
        # queued keys (the wt61 lesson), and an arrow lost to that drain
        # is indistinguishable from a clamped one. Inside the walk the
        # existing dynamics stay: per-key resend is deliberately NOT
        # applied there (see answer()'s docstring) — the walk self-heals
        # by polling the highlight and re-verifying after the settle.
        session.wait_quiescent()
        for _ in range(600):
            if highlight_row("powerline-multiline") is not None:
                picked = True
                break
            session.proc.write(DOWN)
            time.sleep(NAV_GAP_SECONDS)
        # Redraw-race guard: rapid Downs can outrun the menu's repaint, so
        # settle and RE-VERIFY the highlight before confirming — an Enter
        # that races the repaint picks whatever row niu actually has.
        if picked:
            time.sleep(SETTLE_SECONDS)
            picked = highlight_row("powerline-multiline") is not None
        step.check("gallery navigation reached powerline-multiline", picked)
        row = highlight_row("powerline-multiline") or ""
        row_number = re.search(r"(\d+)\)", row)
        gallery_position = int(row_number.group(1)) if row_number else 0
        step.check("theme gallery lists >60 themes",
                   gallery_position > 60,
                   f"powerline-multiline sits at gallery position "
                   f"#{gallery_position}; any position past 60 proves the "
                   "gallery size")
        verdict.capture("J2-theme-highlighted", session)
        if not picked:
            raise AssertionError("powerline-multiline never highlighted")
        session.answer(ENTER)  # delivery-verified confirm

        # The wizard prints its finish screen with the undo receipts, then
        # the about-tour takes over (clears the screen, waits for a key).
        try:
            session.wait_for("Undo this run", timeout=120)
            step.check("finish screen listed undo receipts", True)
        except TimeoutError as err:
            step.check("finish screen listed undo receipts", False, str(err))
        try:
            session.wait_for("press any key to continue", timeout=60)
            session.answer(" ")  # delivery-verified tour keypress
        except TimeoutError:
            pass  # tour skipped (non-tty guard); the REPL marker still runs
        step.check("REPL alive after the wizard (first run continues)",
                   prompt_alive(session, "J2_ALIVE"))
        verdict.capture("J2-repl-alive", session)
    except AssertionError as err:
        step.check("wizard full run completed", False, str(err))
        verdict.capture("J2-stalled", session)
    finally:
        session.close()

    # Durable receipts (screen tails can drop at process exit — the files
    # are the truth, the smoke-journey discipline).
    rc_path = home / ".niubashrc"
    spec_path = home / ".niubash" / "plugins.toml"
    journal_path = home / ".niubash" / "setup-journal.toml"
    rc = rc_path.read_text(encoding="utf-8") if rc_path.is_file() else ""
    spec = spec_path.read_text(encoding="utf-8") if spec_path.is_file() else ""
    journal = (journal_path.read_text(encoding="utf-8")
               if journal_path.is_file() else "")
    finish_text = ""
    finish_file = verdict.transcripts / "J2-finish.txt"
    if finish_file.is_file():
        finish_text = finish_file.read_text(encoding="utf-8")
    step.check("~/.niubashrc written with the picked theme active",
               "OSH_THEME='powerline-multiline'" in rc
               or "BASH_IT_THEME='powerline-multiline'" in rc,
               f"rc exists: {rc_path.is_file()}")
    step.check("plugin spec written (oh-my-bash declared)",
               "oh-my-bash" in spec, f"spec exists: {spec_path.is_file()}")
    step.check("plugin spec declares bash-completion",
               "bash-completion" in spec)
    step.check("spec carries the picked theme",
               "powerline-multiline" in spec)
    step.check("setup journal records collection = 'full'",
               "collection = 'full'" in journal
               or 'collection = "full"' in journal)
    step.check("finish screen undo receipts captured",
               "Undo this run" in finish_text
               or "Undo this run" in session.raw_text())
    # The gallery assertion, made durable: the cloned oh-my-bash tree
    # itself holds the themes (>60).
    themes_dir = home / ".niubash" / "sources" / "oh-my-bash" / "themes"
    if themes_dir.is_dir():
        count = sum(1 for child in themes_dir.iterdir() if child.is_dir())
        step.check("cloned oh-my-bash carries >60 themes on disk",
                   count > 60, f"{count} theme directories")
    step.finish()

    # W1 baseline for P3-S2's byte-stability grid: the rc exactly as the
    # wizard left it, plus the picked theme's exact line and variable.
    state["rc_bytes"]["after-wizard"] = rc_path.read_bytes() \
        if rc_path.is_file() else b""
    picked = re.search(r"^((?:OSH|BASH_IT)_THEME)='powerline-multiline'$",
                       rc, re.M)
    if picked:
        state["theme_var"] = picked.group(1)
        state["theme_line"] = picked.group(0)

    if step.record["status"] == "fail":
        block_rest("J2 failed")
        return verdict.seal()

    # ── J3: activation — source the rc in a live session, zero syntax errors.
    step = verdict.step(
        "J3", "activation: source ~/.niubashrc, zero syntax errors")
    s3 = Session([str(exe)], home, env,
                 raw_log=verdict.transcripts / "J3.raw.ansi", label="J3",
                 delivery_log=verdict.delivery_events)
    try:
        step.check("live session prompt renders",
                   prompt_alive(s3, "J3_ALIVE"))
        # source loads the theme loaders; its own completion wait below
        # is 60s, so the anchor bound matches it.
        s3.send_line("source ~/.niubashrc", anchor_timeout=60)
        # The marker only prints after the source finished (theme loaders
        # included); it is the completion signal, not decoration.
        s3.send_line("echo J3_SRC_DONE")
        try:
            s3.wait_for("J3_SRC_DONE", timeout=60)
            step.check("source ~/.niubashrc completed", True)
        except TimeoutError as err:
            step.check("source ~/.niubashrc completed", False, str(err))
        drain_notices(s3)
        verdict.capture("J3-after-source", s3)
        verdict_for_syntax_errors(
            step, "zero syntax errors after source ~/.niubashrc",
            all_syntax_errors(s3))
    finally:
        s3.close()
    step.finish()

    # ── J4: new terminal x3 — no re-downloads, no not-declared nags.
    step = verdict.step(
        "J4", "new-terminal x3: no re-clones, no nags, prompt renders")
    live = None
    for i in (1, 2, 3):
        s = Session([str(exe)], home, env,
                    raw_log=verdict.transcripts / f"J4-terminal-{i}.raw.ansi",
                    label=f"J4-{i}", delivery_log=verdict.delivery_events)
        try:
            marker = f"J4_READY_{i}"
            if not prompt_alive(s, marker):
                step.check(f"terminal #{i}: prompt renders", False,
                           f"echo {marker} never came back")
                continue
            step.check(f"terminal #{i}: prompt renders", True)
            drain_notices(s)
            body = s.transcript() + "\n" + s.raw_stripped()
            step.check(f"terminal #{i}: no 'Cloning into' (no re-download)",
                       "Cloning into" not in body)
            step.check(f"terminal #{i}: no 'not declared' nag",
                       "not declared" not in body)
            nag_lines = [line for line in body.splitlines()
                         if "awaiting-trust" in line]
            # The documented nag: one line per still-untrusted declared
            # source ('full' leaves bash-completion and bash-preexec
            # untrusted until their own `niu plugin trust`).
            step.check(f"terminal #{i}: awaiting-trust notices stay "
                       "documented (one per untrusted source)",
                       len(nag_lines) <= 4,
                       f"{len(nag_lines)} line(s)"
                       + (f", first: {nag_lines[0].strip()[:100]}"
                          if nag_lines else ""))
            verdict.capture(f"J4-terminal-{i}", s)
            if i == 3:
                live = s
                s = None  # keep the third session alive for J5
        finally:
            if s is not None:
                s.close()
    step.finish()

    # ── J5: the daily battery, in the live session left open by J4.
    step = verdict.step(
        "J5", "daily battery: pipes, cd, test — output, not errors")
    if live is None:
        step.check("live session available from J4", False,
                   "J4 never produced a live session")
        step.finish(status="blocked")
    else:
        try:
            # `ls | wc -l` — a bare number line that was not on screen
            # before the command.
            before = live.text()
            live.send_line("ls | wc -l")
            got_count = False
            deadline = time.time() + 30
            while time.time() < deadline and not got_count:
                for line in live.text().splitlines():
                    if (re.fullmatch(r"\s*\d+\s*", line)
                            and line not in before.splitlines()):
                        got_count = True
                        break
                time.sleep(0.1)
            step.check("`ls | wc -l` printed a count", got_count)

            # `echo hi | cat -n` — the numbered line "1 hi".
            live.send_line("echo hi | cat -n")
            try:
                live.wait_for("1", timeout=30)
                time.sleep(0.5)
                got = any(re.search(r"1\s+hi", line)
                          for line in live.text().splitlines())
                step.check("`echo hi | cat -n` numbered the line", got)
            except TimeoutError as err:
                step.check("`echo hi | cat -n` numbered the line", False,
                           str(err))

            # `cd ~ && pwd` — a NEW line carrying the sandbox path (the
            # prompt already shows the cwd, so compare before/after).
            before = live.text()
            live.send_line("cd ~ && pwd")
            try:
                live.wait_for("journey-", timeout=30)
                time.sleep(0.5)
                got = any("journey-" in line and line not in before.splitlines()
                          and "pwd" not in line
                          for line in live.text().splitlines())
                step.check("`cd ~ && pwd` printed the sandbox home", got)
            except TimeoutError as err:
                step.check("`cd ~ && pwd` printed the sandbox home", False,
                           str(err))

            live.send_line("[[ a != b ]] && echo ok")
            try:
                live.wait_for("ok", timeout=30)
                time.sleep(0.3)
                got = any(re.search(r"^\s*ok\s*$", line)
                          for line in live.text().splitlines())
                step.check("`[[ a != b ]] && echo ok` printed ok", got)
            except TimeoutError as err:
                step.check("`[[ a != b ]] && echo ok` printed ok", False,
                           str(err))

            verdict.capture("J5-battery", live)
            # The picked theme must OWN the prompt: the default fallback
            # prompt is user@host:cwd# — if that shape prefixes the typed
            # battery commands, the theme never rendered (observed on
            # master 2026-10-03: bash-it powerline-multiline sourced
            # without a single syntax error yet PS1 stayed the default).
            battery_echoes = [line for line in live.transcript().splitlines()
                              if re.search(r"(wc -l|cat -n|&& pwd|echo ok)",
                                           line)]
            default_prompt = re.compile(r"^\S+@\S+:[^#]*#\s")
            themed = [line for line in battery_echoes
                      if not default_prompt.match(line)]
            step.check("theme prompt visible after each battery command",
                       bool(battery_echoes) and len(themed) == len(battery_echoes),
                       f"{len(themed)}/{len(battery_echoes)} typed command "
                       "lines carry a non-default prompt; first default-"
                       f"shaped line: {battery_echoes[0].strip()[:100] if battery_echoes else '(none)'}")
            verdict_for_syntax_errors(
                step, "zero syntax errors during the battery",
                all_syntax_errors(live))
            step.check("prompt still visible after the battery",
                       prompt_alive(live, "J5_STILL_HERE"))
            verdict.capture("J5-after", live)
        finally:
            live.close()
        step.finish()

    # ── Wave-1 persistence group (journey-spec P3 + P8-S4, lane wt79/W1):
    # the same sandbox the J-blocks aged, after J5 / before J6 — the
    # persistence assertions need the theme-bearing source still
    # installed, and J6 then runs on the state P8-S4 restored.
    p_steps = {
        "P3-S1": p3_s1_reopen,
        "P3-S2": p3_s2_rc_stability,
        "P3-S3": p3_s3_first_key,
        "P3-S4": p3_s4_aged_state,
        "P8-S4": p8_s4_remove_active_theme_source,
    }
    p_blocked = None
    for p_id in selection["p"]:
        if p_blocked is None and state.get("theme_line") is None:
            p_blocked = ("no wizard theme line (J2 failed) — the aged-state "
                         "assertions need the picked theme")
        if p_blocked is not None:
            verdict.step(p_id, f"{p_id} (blocked: {p_blocked})").finish(
                status="blocked")
            continue
        p_steps[p_id](exe, home, env, verdict, seed, state)

    if not selection["j6"]:
        return verdict.seal()

    # ── J6: the owner's exact flow — trust + activate bash-completion.
    step = verdict.step(
        "J6", "trust + activate bash-completion, zero syntax errors")
    s6 = Session([str(exe)], home, env,
                 raw_log=verdict.transcripts / "J6.raw.ansi", label="J6",
                 delivery_log=verdict.delivery_events)
    try:
        # The aliveness probe and the trust verb travel as ONE anchored
        # line: two back-to-back sends after session open were the release
        # glue twice (37153503706 / 37157587488) — prompt_alive's
        # wait_for + 0.3s sleep is not an output anchor, and on slow
        # runners the next line appended to the unsubmitted input. One
        # send cannot glue with itself.
        s6.send_line("echo J6_ALIVE && niu plugin trust bash-completion",
                     anchor_timeout=90)
        try:
            s6.wait_for("J6_ALIVE", timeout=STARTUP_TIMEOUT)
            step.check("live session prompt renders", True)
        except TimeoutError:
            step.check("live session prompt renders", False,
                       "echo J6_ALIVE did not come back")
        # The trust step's own expected-output wait is 90s; the anchor
        # bound matches it so a slow trust run expires the anchor, not
        # the sequencing (run 37153503706's 90s timeout was the glue).
        s6.send_line("niu plugin trust bash-completion", anchor_timeout=90)
        try:
            s6.wait_for("is now trusted", timeout=90)
            step.check("`niu plugin trust bash-completion` reported trusted",
                       True)
        except TimeoutError as err:
            step.check("`niu plugin trust bash-completion` reported trusted",
                       False, str(err))
        s6.send_line("source ~/.niubashrc", anchor_timeout=60)
        s6.send_line("echo J6_SRC_DONE")
        try:
            s6.wait_for("J6_SRC_DONE", timeout=60)
            step.check("source ~/.niubashrc completed after trust", True)
        except TimeoutError as err:
            step.check("source ~/.niubashrc completed after trust", False,
                       str(err))
        drain_notices(s6)
        verdict.capture("J6-after-trust-source", s6)
        verdict_for_syntax_errors(
            step, "zero syntax errors after trust + source",
            all_syntax_errors(s6))
    finally:
        s6.close()

    # Activation completes in the next terminal: the materialized loader
    # block runs when a fresh shell sources the rc. This is where the
    # owner's bash_completion:1376 surfaced.
    s6b = Session([str(exe)], home, env,
                  raw_log=verdict.transcripts / "J6-fresh.raw.ansi",
                  label="J6-fresh", delivery_log=verdict.delivery_events)
    try:
        step.check("fresh terminal after trust: prompt renders",
                   prompt_alive(s6b, "J6B_ALIVE"))
        drain_notices(s6b)
        verdict.capture("J6-fresh-terminal", s6b)
        verdict_for_syntax_errors(
            step, "fresh terminal sources bash_completion cleanly",
            all_syntax_errors(s6b))
        # The activated completion must actually be loaded, not just
        # absent errors: bash_completion defines _init_completion.
        s6b.send_line("type _init_completion >/dev/null 2>&1 && echo BC_LOADED")
        try:
            s6b.wait_for("BC_LOADED", timeout=30)
            step.check("bash_completion functions are loaded", True)
        except TimeoutError as err:
            step.check("bash_completion functions are loaded", False, str(err))
    finally:
        s6b.close()
    step.finish()

    return verdict.seal()


def main() -> int:
    # CI runners default to a charmap console; the journey transcript carries
    # CJK/emoji wizard text (the owner's own wording). Force UTF-8 stdio.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="The Golden User Journey gate (owner directive 2026-10-04)")
    parser.add_argument("niu", type=Path, help="path to the built niu.exe")
    parser.add_argument("--artifacts", type=Path, default=None,
                        help="where verdict + transcripts go "
                             "(default: target/journey-results/<timestamp>)")
    parser.add_argument("--keep-sandbox", type=Path, default=None,
                        help="create the sandbox under this directory "
                             "(kept on failure for diagnosis)")
    parser.add_argument(
        "--phase", choices=sorted(PHASES), default="full",
        help="journey group to run: full (default: J1-J5 + the wave-1 "
             "persistence group P3-S1..S4 + P8-S4 + J6), gate (the legacy "
             "J1-J6 block), persist (J1-J5 + P3 + P8-S4), P3, P8")
    # Hidden stress harness (owner-approved validation shape, not a user
    # knob): a randomized 0..N ms pause before every send's settle check,
    # simulating runner slowness — the shape that broke release runs
    # 37149660449 and 37153503706. Validated 3x with N=800.
    parser.add_argument("--stress-delay-ms", type=int, default=0,
                        help=argparse.SUPPRESS)
    args = parser.parse_args()

    global STRESS_DELAY_MS
    STRESS_DELAY_MS = max(0, args.stress_delay_ms)
    exe = args.niu.resolve()
    if not exe.is_file():
        print(f"SKIP: niu binary not found: {exe}")
        return 2
    if shutil.which("git") is None:
        print("SKIP: git not on PATH (the journey clones real origins)")
        return 2

    repo = Path(__file__).resolve().parent.parent.parent
    if args.artifacts is not None:
        artifacts = args.artifacts
    else:
        artifacts = repo / "target" / "journey-results" / datetime.now(
            ).strftime("%Y%m%d-%H%M%S")
    artifacts.mkdir(parents=True, exist_ok=True)
    verdict = Verdict(artifacts)

    if args.keep_sandbox is not None:
        args.keep_sandbox.mkdir(parents=True, exist_ok=True)
    sandbox_parent = (args.keep_sandbox or Path(tempfile.gettempdir())).resolve()
    # ABSOLUTE on purpose: a relative sandbox home doubles inside niu's
    # path joins (observed 2026-10-03: rc written to <home>/<home>/.niubashrc).
    sandbox = Path(tempfile.mkdtemp(prefix="journey-", dir=sandbox_parent)).resolve()

    (artifacts / "run.json").write_text(
        json.dumps({"niu": str(exe), "sandbox": str(sandbox),
                    "phase": args.phase, "started_utc": now_utc(),
                    "pid": os.getpid(), "stress_delay_ms": STRESS_DELAY_MS},
                   indent=2) + "\n",
        encoding="utf-8", newline="\n")

    try:
        result = journey(exe, sandbox, verdict, phase=args.phase)
    finally:
        # Sandbox hygiene: the sources trees are heavy clones; keep them
        # only when a red gate (or a crash) needs diagnosis.
        if gate_is_red(artifacts):
            (artifacts / "sandbox-kept.txt").write_text(
                f"{sandbox}\n", encoding="utf-8", newline="\n")
            print(f"NOTE: sandbox kept for diagnosis: {sandbox}")
        else:
            shutil.rmtree(sandbox, ignore_errors=True)

    print((artifacts / "verdict.txt").read_text(encoding="utf-8"))
    print(f"artifacts: {artifacts}")
    return 0 if result == "pass" else 1


def gate_is_red(artifacts: Path) -> bool:
    """True when the verdict says anything but pass (or could not be
    read — an unread verdict keeps the sandbox for diagnosis)."""
    data = artifacts / "verdict.json"
    if not data.is_file():
        return True
    try:
        return json.loads(data.read_text(encoding="utf-8"))["result"] != "pass"
    except Exception:  # noqa: BLE001
        return True


if __name__ == "__main__":
    sys.exit(main())
