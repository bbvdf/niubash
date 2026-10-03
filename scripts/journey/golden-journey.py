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
import json
import os
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
]


def now_utc() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


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

    def __init__(self, argv, cwd, env, cols=COLS, rows=ROWS, raw_log=None):
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

    def answer(self, keys):
        """Settle out the menu draw, then press; digit and Enter split."""
        time.sleep(SETTLE_SECONDS)
        if keys.endswith(ENTER) and len(keys) > 1:
            self.proc.write(keys[:-1])
            time.sleep(ENTER_GAP_SECONDS)
            self.proc.write(ENTER)
        else:
            self.proc.write(keys)

    def send_line(self, line):
        """Type a whole command at the REPL prompt, then Enter."""
        time.sleep(SETTLE_SECONDS)
        self.proc.write(line)
        time.sleep(ENTER_GAP_SECONDS)
        self.proc.write(ENTER)

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


# ── The journey steps ───────────────────────────────────────────────────────
def journey(exe: Path, root: Path, verdict: Verdict) -> str:
    home = (root / "home").resolve()
    home.mkdir(parents=True, exist_ok=True)
    env = build_env(home, exe)

    # ── J1 + J2 share one session: on a fresh install `niu` IS the wizard.
    step = verdict.step(
        "J1", "fresh-install first contact: v-banner + wizard")
    session = None
    j1_ok = True
    try:
        session = Session([str(exe)], home, env,
                           raw_log=verdict.transcripts / "J1J2.raw.ansi")
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

    def block_rest(reason):
        for later in ("J2", "J3", "J4", "J5", "J6"):
            verdict.step(later, f"{later} (blocked: {reason})").finish(
                status="blocked")

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
        session.proc.write(ENTER)

        # The wizard prints its finish screen with the undo receipts, then
        # the about-tour takes over (clears the screen, waits for a key).
        try:
            session.wait_for("Undo this run", timeout=120)
            step.check("finish screen listed undo receipts", True)
        except TimeoutError as err:
            step.check("finish screen listed undo receipts", False, str(err))
        try:
            session.wait_for("press any key to continue", timeout=60)
            session.proc.write(" ")
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

    if step.record["status"] == "fail":
        block_rest("J2 failed")
        return verdict.seal()

    # ── J3: activation — source the rc in a live session, zero syntax errors.
    step = verdict.step(
        "J3", "activation: source ~/.niubashrc, zero syntax errors")
    s3 = Session([str(exe)], home, env,
                 raw_log=verdict.transcripts / "J3.raw.ansi")
    try:
        step.check("live session prompt renders",
                   prompt_alive(s3, "J3_ALIVE"))
        s3.send_line("source ~/.niubashrc")
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
                    raw_log=verdict.transcripts / f"J4-terminal-{i}.raw.ansi")
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

    # ── J6: the owner's exact flow — trust + activate bash-completion.
    step = verdict.step(
        "J6", "trust + activate bash-completion, zero syntax errors")
    s6 = Session([str(exe)], home, env,
                 raw_log=verdict.transcripts / "J6.raw.ansi")
    try:
        step.check("live session prompt renders",
                   prompt_alive(s6, "J6_ALIVE"))
        s6.send_line("niu plugin trust bash-completion")
        try:
            s6.wait_for("is now trusted", timeout=90)
            step.check("`niu plugin trust bash-completion` reported trusted",
                       True)
        except TimeoutError as err:
            step.check("`niu plugin trust bash-completion` reported trusted",
                       False, str(err))
        s6.send_line("source ~/.niubashrc")
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
                  raw_log=verdict.transcripts / "J6-fresh.raw.ansi")
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
    args = parser.parse_args()

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
                    "started_utc": now_utc(), "pid": os.getpid()},
                   indent=2) + "\n",
        encoding="utf-8", newline="\n")

    try:
        result = journey(exe, sandbox, verdict)
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
