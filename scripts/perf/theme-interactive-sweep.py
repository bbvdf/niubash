#!/usr/bin/env python3
"""ALL-THEMES INTERACTIVE HANG SWEEP — every theme, ConPTY, hard timeouts.

Owner iron directive (2026-10-05, after interactive coverage was found at
3/166 themes while bash-it's codeword/gitline hang the live session,
niu#182): interactive theme testing must cover EVERY theme, through a real
ConPTY, with hard timeouts, as a permanent automated reality — not a
hand-run sample. This harness is that reality and is designed to become the
third release gate (`--gate`: nonzero exit on any HANG / CRASH /
ERROR-STORM / RENDER-BROKEN theme cell; wire-up into release.yml is a
separate change).

FULL MATRIX, no sampling. Per theme (from BOTH trusted sources — every
oh-my-bash theme and every bash-it theme):

  histcontrol ∈ {unset, autosave, auto, noauto}   # niu#182 trigger is
                                                   # HISTCONTROL-conditional
  histscale   ∈ {0, 10, 2000, 20000} HISTFILE entries  # size-dependent?
  shape       ∈ {fresh, aged}                     # aged = second session on
                                                   # the SAME home a previous
                                                   # session just used (its
                                                   # saved history stays)
  + resrc     — the theme sourced twice in one session (re-source behavior;
                run per histcontrol at histscale 2000)

=> 36 interactive cells per theme + controls (bare shell and each framework
with NO theme, over the full histcontrol x histscale grid) so the winpty/
ConPTY transport floor and the frameworks' own per-prompt cost are measured
references, not confounders. A fresh+aged pair shares one sandbox home and
runs back-to-back in one worker task; every other cell is independent.

Interactive steps per session, each with its own hard bound (default 10s):

  boot          spawn -> rc bootstrap sentinels (records HISTCONTROL as the
                rc sees it pre- and post-framework-load, histappend state)
  first_prompt  first prompt sentinel rendered on the session
  t1-type       type `echo T1` -> echo of the typed text comes back
  t1-enter      ENTER -> `T1` output + next prompt
  t2-type       type `echo T2` -> echo
  t2-enter      ENTER -> `T2` output + next prompt
  (resrc)       `. <theme file>` -> next prompt, then one more round trip
  exit          `exit` -> process exits within the exit grace

A typed send whose first byte is swallowed (the niu#167 class) is RECORDED
as masked-first-key, then recovered with a kill-line so the session signal
stays clean — reported, never silently masked.

Verdict per cell, priority order:
  CRASH          the process died before the planned steps completed
                 (P0-class: separate note, not a hang)
  HANG           any step exceeded its hard bound (stall step + last output
                 captured); no further steps are attempted
  ERROR-STORM    >= --error-storm-threshold error-marker lines in the stream
  RENDER-BROKEN  cursor out of the screen, blank screen with a live
                 process, or raw escape-sequence leak fragments rendered
  OK-SLOW        every step completed but some step took > --slow-ms
  OK-FAST        everything completed fast and clean

Driver contract (same discipline as scripts/perf/asset-timing.py):
  * sandboxed HOME — USERPROFILE/HOME/LOCALAPPDATA/TEMP/TMP all point into
    a per-cell sandbox; an empty ~/.niubashrc suppresses the first-run
    wizard; HISTFILE is the pre-seeded sandbox file; nothing touches the
    real profile.
  * network off-affordance: DISABLE_AUTO_UPDATE/OSH_UPDATE_CHECK,
    GIT_TERMINAL_PROMPT=0, git config pointed at sandbox files. The driver
    itself performs no network I/O.
  * bash-it loads through the NATIVE loader (`source $BASH_IT/bash_it.sh`)
    in a per-worker writable clone with an EMPTY enabled/ dir — exactly the
    path bash_it.sh gives a real theme, including lib/history.bash's
    per-prompt _bash-it-history-auto-load hook (the niu#182 mechanism).
  * every session is killed on every exit path (ConPTY terminate +
    taskkill /T /F belt-and-braces, as asset-timing.py).
  * machine-readable output: cells.jsonl (one record per cell) + summary
    JSON (per-theme aggregates, verdict-change/conditional analysis,
    static theme-feature inventory: no-PROMPT_COMMAND themes, per-prompt
    subshells, network touches, CJK, minified files, history-reload calls).
  * resumable: cells already present in cells.jsonl are skipped
    (--no-resume / --rerun-cells to force); DRIVER-ERROR cells are retried.

Requires pywinpty and pyte (same deps as scripts/journey). Windows only
(ConPTY), like the journey gate. Run artifacts belong on a roomy drive
(--run-dir), NOT the near-full repo drive.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path

import pyte
from winpty import PtyProcess

REPO = Path(__file__).resolve().parents[2]

SENT_BOOT = "@@NIU-SWEEP-BOOT@@"
SENT_PROMPT = "@@NIU-SWEEP-PROMPT@@"

HISTCONTROLS = ("unset", "autosave", "auto", "noauto")
HISTSCALES = (0, 10, 2000, 20000)
SHAPES = ("fresh", "aged")
RESRC_SCALE = 2000

ERROR_MARKERS = (
    "command not found",
    "syntax error",
    "no such file or directory",
    "unbound variable",
    "bad substitution",
    "not a valid identifier",
    "cannot execute",
    "permission denied",
    "segmentation fault",
    "niu:",
)
ESCAPE_LEAK_RE = re.compile(r"\[(?:0m|\?25[hl]|\?2004[hl]|\?1000[hl]|\d+;\d+[HJK]|\d+[GK])")
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b[78]|\x1b[=>]")


def _canon(s: str) -> str:
    """ANSI-stripped, whitespace-collapsed text. Echo witnesses are matched
    in this space: readline repaints interleave color escapes into the raw
    stream and the emulated screen hard-wraps long input lines, so neither
    raw nor screen contains the typed text contiguously."""
    return re.sub(r"\s+", "", ANSI_RE.sub("", s))

VERDICT_ORDER = ("OK-FAST", "OK-SLOW", "RENDER-BROKEN", "ERROR-STORM", "HANG", "CRASH")
BAD_VERDICTS = {"HANG", "CRASH", "ERROR-STORM", "RENDER-BROKEN"}


# ---------------------------------------------------------------------------
# Inventory
# ---------------------------------------------------------------------------

class Theme:
    __slots__ = ("framework", "name", "source_file", "companion_files",
                 "theme_level_files", "features")

    def __init__(self, framework: str, name: str, source_file: Path,
                 companion_files: list[Path]):
        self.framework = framework  # "omb" | "bashit" | "bare"
        self.name = name
        self.source_file = source_file
        # companion_files = shared libs every bash-it theme loads anyway
        # (base/githelpers/p4helpers) — they must NOT drive per-theme
        # classification or all 84 bash-it themes would look identical.
        self.companion_files = companion_files
        # theme_level_files = the theme file + its own-dir *.base.bash —
        # what makes THIS theme different from the next.
        self.theme_level_files = [source_file] + [
            p for p in source_file.parent.glob("*.base.bash")
            if p != source_file]
        self.features: dict = {}

    @property
    def key(self) -> str:
        return f"{self.framework}/{self.name}"


def discover_omb(root: Path) -> list[Theme]:
    themes = []
    for d in sorted(p for p in (root / "themes").iterdir() if p.is_dir()):
        f = d / f"{d.name}.theme.sh"
        if not f.is_file():
            f = d / f"{d.name}.theme.bash"  # omb `random` ships .theme.bash
        if f.is_file():
            themes.append(Theme("omb", d.name, f, []))
    return themes


def discover_bashit(root: Path) -> list[Theme]:
    themes_dir = root / "themes"
    shared = [themes_dir / n for n in
              ("base.theme.bash", "githelpers.theme.bash", "p4helpers.theme.bash")]
    themes = []
    for d in sorted(p for p in themes_dir.iterdir() if p.is_dir()):
        f = d / f"{d.name}.theme.bash"
        if not f.is_file():
            continue
        companions = [p for p in shared if p.is_file()]
        companions += [p for p in sorted(d.glob("*.base.bash"))]
        themes.append(Theme("bashit", d.name, f, companions))
    return themes


# ---------------------------------------------------------------------------
# Static theme-feature inventory (edge cases the verdict JSON enumerates)
# ---------------------------------------------------------------------------

CJK_RE = re.compile(r"[\u2e80-\u9fff\uf900-\ufaff\uff00-\uffef]")
NETWORK_RE = re.compile(r"\b(curl|wget|ping|nslookup|finger|ftp|ssh)\b")
SUBSHELL_RE = re.compile(r"\$\(|`")
PROMPT_CMD_RE = re.compile(r"PROMPT_COMMAND|safe_append_prompt_command")
HISTORY_NEEDLES = ("_save-and-reload-history", "_bash-it-history-auto-")


def history_call_evidence(text: str) -> list[str]:
    """Lines that CALL the history-reload machinery (not lines that define
    it — base.theme.bash's `function _save-and-reload-history() {` must not
    classify every bash-it theme). Also captures the
    `safe_append_prompt_command '_save-and-reload-history 1'` argument
    form (codeword). The captured lines ARE the theme's per-prompt
    pattern for the findings report."""
    hits = []
    for line in text.splitlines():
        s = line.strip()
        if not s or (s.startswith("function ") and "(" in s):
            continue
        if any(n in s for n in HISTORY_NEEDLES) or re.match(r"history\s+-[acrn]\b", s):
            hits.append(s[:100])
    return hits
CLOCK_RE = re.compile(r"%[THMS]|date\s*\+|SECONDS|EPOCHREALTIME")
GIT_RE = re.compile(r"__git_ps1|git\s+(rev-parse|status|branch|describe|log)|parse_git")


def scan_features(theme: Theme) -> dict:
    def read_all(paths: list[Path]) -> str:
        parts = []
        for f in paths:
            try:
                parts.append(f.read_text(encoding="utf-8", errors="replace"))
            except OSError:
                continue
        return "\n".join(parts)

    # Classification inputs come from THEME-LEVEL files only; the shared
    # libs are scanned for information only (they load for every theme).
    joined = read_all([theme.source_file] + theme.theme_level_files[1:])
    shared = read_all(theme.companion_files)
    line_count = max(1, joined.count("\n") + len(theme.theme_level_files))
    hist_calls = history_call_evidence(joined)
    hist_args = sorted({m for c in hist_calls
                        for m in re.findall(r"_save-and-reload-history\s+(\S+)", c)})
    return {
        "size_bytes": theme.source_file.stat().st_size if theme.source_file.is_file() else 0,
        "files_scanned": [p.name for p in theme.theme_level_files],
        "shared_files_loaded": [p.name for p in theme.companion_files],
        "appends_prompt_command": bool(PROMPT_CMD_RE.search(joined)),
        "history_reload_calls": bool(hist_calls),
        "history_call_evidence": hist_calls[:6],
        "history_reload_args": hist_args,
        "history_always_autosave": "1" in hist_args,
        "history_call_names": sorted(set(re.findall(
            r"(?m)^\s*_\S+", joined)))[:8],
        "subshell_count": len(SUBSHELL_RE.findall(joined)),
        "per_prompt_subshell_risk": bool(SUBSHELL_RE.search(joined)),
        "sources_other_files": bool(re.search(
            r"(?:^|[\s;])(?:source|\.)\s+[\"']?[\~/$.]", joined, re.M)),
        "network_touch": sorted(set(NETWORK_RE.findall(joined))),
        "has_cjk": bool(CJK_RE.search(joined)),
        "clock_redraw_risk": bool(CLOCK_RE.search(joined)),
        "git_per_prompt": bool(GIT_RE.search(joined)),
        "minified": bool(joined) and (len(joined) / line_count) > 200,
        "shared_base_defines_history_reload": "_save-and-reload-history" in shared,
    }


def classify_family(theme: Theme) -> str:
    f = theme.features
    if f["history_reload_calls"]:
        return "history-reload (niu#182 family)"
    if f["network_touch"]:
        return "network-touching"
    if f["clock_redraw_risk"]:
        return "clock-redraw"
    if f["subshell_count"] >= 5:
        return "heavy-subshell"
    return "none"


# ---------------------------------------------------------------------------
# Sandbox + rc construction
# ---------------------------------------------------------------------------

def sh_quote(s: str) -> str:
    return "'" + s.replace("'", "'\\''") + "'"


def histcontrol_line(hc: str) -> str:
    if hc == "unset":
        return "unset HISTCONTROL"
    return f"export HISTCONTROL={sh_quote(hc)}"


def build_rc(framework: str, theme: Theme | None, control_kind: str,
             hc: str, omb: Path | None, bashit: Path | None,
             home: Path, histfile: Path) -> str:
    """One probe rc. control_kind: 'bare' | 'fw' | 'theme'.

    HISTFILE is set INSIDE the rc, the way bash-it's own docs tell users to
    configure it. Deliberately NOT via the environment: on niu 1.3.4
    (b4b87f1) an env-imported HISTFILE makes the history builtin fail under
    ConPTY (rc-time `history -a` silently aborts the rest of the rc;
    prompt-time `history -a` errors `filename not specified` even though
    $HISTFILE prints as set), which silently disarms the niu#182
    history-reload path. rc-set HISTFILE keeps every cell's histscale
    dimension operative. See docs/audit/findings/wt91-allthemes.md."""
    lines = [
        "unset BASH_ENV",
        "export TERM='xterm-256color'",
        "export DISABLE_AUTO_UPDATE=true",
        "export OSH_UPDATE_CHECK=false",
        "export GIT_TERMINAL_PROMPT=0",
        f"HISTFILE={sh_quote(histfile.as_posix())}",
        histcontrol_line(hc),
        (f"printf '{SENT_BOOT} phase=pre hc=%s ha=%s hf=%s\\n' "
         "\"${HISTCONTROL-<unset>}\" \"$(shopt -q histappend && echo 1 || echo 0)\" "
         "\"${HISTFILE-<unset>}\""),
    ]
    if control_kind != "bare":
        if framework == "omb":
            assert omb is not None
            lines += [
                f"OSH={sh_quote(omb.as_posix())}",
                f"OSH_CUSTOM={sh_quote((home / 'custom').as_posix())}",
                f"OSH_CACHE_DIR={sh_quote((home / 'osh-cache').as_posix())}",
                f"OSH_THEME={sh_quote(theme.name if theme else '')}",
                "source \"$OSH/oh-my-bash.sh\"",
            ]
        else:
            assert bashit is not None
            lines += [
                f"BASH_IT={sh_quote(bashit.as_posix())}",
                f"BASH_IT_THEME={sh_quote(theme.name if theme else '')}",
                "source \"$BASH_IT/bash_it.sh\"",
            ]
    lines += [
        "__sweep_src_rc=$?",
        (f"printf '{SENT_BOOT} phase=post hc=%s rc=%s hf=%s\\n' "
         "\"${HISTCONTROL-<unset>}\" \"$__sweep_src_rc\" \"${HISTFILE-<unset>}\""),
        "__sweep_decl=$(declare -p PROMPT_COMMAND 2>/dev/null)",
        "case \"$__sweep_decl\" in",
        f"  'declare -a'*) PROMPT_COMMAND+=(\"printf '%s\\\\n' '{SENT_PROMPT}'\") ;;",
        f"  *) PROMPT_COMMAND=\"${{PROMPT_COMMAND:+${{PROMPT_COMMAND}}; }}printf '%s\\\\n' '{SENT_PROMPT}'\" ;;",
        "esac",
    ]
    return "\n".join(lines) + "\n"


def sandbox_env(base: dict, home: Path) -> dict:
    env = dict(base)
    for k in ("BASH_ENV", "OSH", "OSH_THEME", "BASH_IT", "BASH_IT_THEME",
              "OSH_CUSTOM", "OSH_CACHE_DIR", "NIU_PLUGIN_SPEC", "HISTCONTROL",
              "HISTSIZE", "HISTFILESIZE", "PROMPT_COMMAND", "PS1", "INPUTRC",
              "HISTFILE", "OSH_UPDATE_CHECK", "DISABLE_AUTO_UPDATE"):
        env.pop(k, None)
    (home / "tmp").mkdir(parents=True, exist_ok=True)
    (home / "local-appdata").mkdir(parents=True, exist_ok=True)
    # An empty primary rc suppresses the first-run setup wizard without
    # contributing anything else; the probe rc arrives via --rcfile.
    (home / ".niubashrc").write_text("", encoding="utf-8")
    env.update({
        "HOME": str(home),
        "USERPROFILE": str(home),
        "LOCALAPPDATA": str(home / "local-appdata"),
        "TEMP": str(home / "tmp"),
        "TMP": str(home / "tmp"),
        "HISTFILE": str(home / "history"),  # engine env import is broken;
        # the operative HISTFILE is set inside the rc (see build_rc)
        "GIT_CONFIG_GLOBAL": str(home / "gitconfig"),
        "GIT_CONFIG_SYSTEM": os.devnull,
        "GIT_TERMINAL_PROMPT": "0",
        "TERM": "xterm-256color",
        "INPUTRC": os.devnull,
    })
    (home / "gitconfig").write_text(
        "[user]\n\tname = sweep\n\temail = sweep@localhost\n"
        "[init]\n\tdefaultBranch = main\n",
        encoding="utf-8",
    )
    return env


def make_histfile(template_dir: Path, scale: int) -> Path:
    template_dir.mkdir(parents=True, exist_ok=True)
    f = template_dir / f"histfile-{scale}"
    if not f.is_file():
        f.write_text(
            "".join(f"echo seeded-history-line-{i}\n" for i in range(scale)),
            encoding="utf-8", newline="\n")
    return f


def make_fixture_repo(rundir: Path) -> Path:
    """cwd for every session: a small committed git repo so git themes
    exercise their git segment deterministically and offline."""
    fx = rundir / "fixture-repo"
    fx.mkdir(parents=True, exist_ok=True)
    git = shutil.which("git")
    if git is None:
        print("sweep: git not on PATH — themes will show a non-repo prompt",
              file=sys.stderr)
        return fx
    env = dict(os.environ)
    env["GIT_CONFIG_GLOBAL"] = str(rundir / "fixture-gitconfig")
    env["GIT_CONFIG_SYSTEM"] = os.devnull
    (fx / "notes.txt").write_text("sweep fixture\n", encoding="utf-8")
    try:
        subprocess.run([git, "init", "-q"], cwd=fx, env=env, check=True, timeout=30)
        subprocess.run([git, "add", "-A"], cwd=fx, env=env, check=True, timeout=30)
        subprocess.run([git, "-c", "user.name=sweep", "-c", "user.email=sweep@localhost",
                        "commit", "-qm", "fixture"], cwd=fx, env=env, check=True,
                       timeout=30)
    except (subprocess.SubprocessError, OSError) as exc:
        print(f"sweep: fixture git init failed ({exc}); proceeding without a repo",
              file=sys.stderr)
    return fx


def prepare_bashit_sandbox(bashit: Path, rundir: Path, worker: int) -> Path:
    """Writable per-worker clone; enabled/ emptied so the theme under test
    is the only variable (lib/*.bash still load — that is the real user
    path and includes the niu#182 history hook)."""
    dst = rundir / f"bash-it-sandbox-w{worker}"
    for _ in range(3):
        if not dst.exists():
            break
        shutil.rmtree(dst, ignore_errors=True)
        time.sleep(0.5)
    # dirs_exist_ok: a killed previous run can leave an unremovable
    # (file-locked) tree behind — refresh in place rather than crash.
    shutil.copytree(bashit, dst, ignore=shutil.ignore_patterns(".git"),
                    dirs_exist_ok=True)
    enabled = dst / "enabled"
    if enabled.exists():
        shutil.rmtree(enabled, ignore_errors=True)
    enabled.mkdir(exist_ok=True)
    return dst


# ---------------------------------------------------------------------------
# ConPTY session
# ---------------------------------------------------------------------------

class ConptySession:
    """pyte-screened ConPTY session with a raw-stream tap (sentinel counts
    are taken from RAW bytes: a theme that wipes/redraws the screen can hide
    a sentinel from the emulated display but not from the stream)."""

    def __init__(self, argv: list[str], cwd: Path, env: dict, cols: int, rows: int):
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.Stream(self.screen)
        self.lock = threading.Lock()
        self.display = list(self.screen.display)
        self.last_change = time.perf_counter()
        self.boot_lines: list[str] = []
        self.prompt_count = 0
        self.raw_len = 0
        self.error_events: list[str] = []
        self.escape_leak_count = 0
        self.proc = PtyProcess.spawn(argv, cwd=str(cwd), env=env, dimensions=(rows, cols))
        self.spawn_at = time.perf_counter()
        self.alive = True
        self._raw = bytearray()
        self._reader = threading.Thread(target=self._pump, daemon=True)
        self._reader.start()

    def _pump(self):
        while self.alive:
            try:
                data = self.proc.read(256)
            except Exception:
                break
            if not data:
                continue
            with self.lock:
                self._raw.extend(data.encode("utf-8", "ignore")
                                 if isinstance(data, str) else data)
                self.raw_len = len(self._raw)
                self.stream.feed(data)
                cur = list(self.screen.display)
                if cur != self.display:
                    self.display = cur
                    self.last_change = time.perf_counter()
                    # Escape-leak check runs on the RENDERED screen only:
                    # raw bytes legitimately contain consumed CSI sequences;
                    # a leak is fragments pyte had to render as text.
                    self.escape_leak_count = sum(
                        len(ESCAPE_LEAK_RE.findall(ln)) for ln in cur)
                text = data if isinstance(data, str) else data.decode("utf-8", "ignore")
                for line in text.splitlines():
                    if SENT_BOOT in line:
                        self.boot_lines.append(line[line.index(SENT_BOOT):][:120])
                self.prompt_count += text.count(SENT_PROMPT)
                low = text.lower()
                for marker in ERROR_MARKERS:
                    at = low.find(marker)
                    if at >= 0:
                        self.error_events.append(text[max(0, at - 40):at + 80].strip())

    # -- observation -------------------------------------------------------

    def raw_since(self, pos: int) -> str:
        with self.lock:
            return self._raw[pos:].decode("utf-8", "replace")

    def raw_pos(self) -> int:
        with self.lock:
            return self.raw_len

    def raw_tail(self, limit: int = 4096) -> str:
        with self.lock:
            return self._raw[-limit:].decode("utf-8", "replace")

    def screen_text(self) -> str:
        with self.lock:
            return "\n".join(self.display)

    def snapshot(self):
        with self.lock:
            return (list(self.display), self.last_change, time.perf_counter(),
                    self.prompt_count)

    def cursor(self):
        with self.lock:
            return self.screen.cursor.x, self.screen.cursor.y

    # -- input --------------------------------------------------------------

    def write(self, data: str):
        try:
            self.proc.write(data)
        except (OSError, ValueError):
            pass

    def close(self):
        self.alive = False
        try:
            self.proc.terminate()
        except Exception:
            pass
        # Belt and braces: a ConPTY terminate does not always reap the child
        # (orphaned shell sessions were observed on Win10 19044).
        try:
            if self.proc.isalive():
                subprocess.run(["taskkill", "/PID", str(self.proc.pid), "/T", "/F"],
                               capture_output=True, timeout=15)
        except (OSError, subprocess.SubprocessError, AttributeError):
            pass


def wait_quiet(sess: ConptySession, settle_s: float, max_extension_s: float) -> float:
    """Wait until the screen has been quiet for settle_s, bounded by
    max_extension_s (clock-repaint themes tick every second; the extension
    bounds the wait instead of never settling)."""
    deadline = time.monotonic() + max_extension_s
    while time.monotonic() < deadline:
        _, last_change, now, _ = sess.snapshot()
        if (now - last_change) >= settle_s:
            return now - last_change
        time.sleep(0.02)
    return -1.0


def last_nonempty_row(rows: list[str]) -> str:
    for ln in reversed(rows):
        if ln.strip():
            return ln.strip()
    return ""


def wait_prompt_returned(sess: ConptySession, input_row: str,
                         timeout_s: float) -> tuple[bool, float]:
    """Fallback witness for a lost prompt sentinel: a framework or theme
    that REBUILDS PROMPT_COMMAND on (re-)source drops the appended sentinel
    even though the prompt renders fine. Prompt-returned = the bottom row
    is no longer the typed input line and the screen moved."""
    start = time.perf_counter()
    deadline = start + timeout_s
    while time.perf_counter() < deadline:
        display, last_change, now, _ = sess.snapshot()
        if last_nonempty_row(display) != input_row and \
                (now - last_change) >= 0.2:
            return True, (time.perf_counter() - start) * 1000.0
        if not sess.proc.isalive():
            return False, (time.perf_counter() - start) * 1000.0
        time.sleep(0.02)
    return False, (time.perf_counter() - start) * 1000.0


def wait_prompt_count(sess: ConptySession, want: int, timeout_s: float,
                      settle_s: float) -> tuple[bool, float]:
    """Wait for the raw prompt-sentinel count to reach `want`, then let the
    render settle. Returns (reached, elapsed_ms)."""
    start = time.perf_counter()
    deadline = start + timeout_s
    while time.perf_counter() < deadline:
        _, _, _, count = sess.snapshot()
        if count >= want:
            wait_quiet(sess, settle_s, 3.0)
            return True, (time.perf_counter() - start) * 1000.0
        if not sess.proc.isalive():
            return False, (time.perf_counter() - start) * 1000.0
        time.sleep(0.01)
    return False, (time.perf_counter() - start) * 1000.0


def _canon_keepnl(s: str) -> str:
    """ANSI-stripped, space/tab-collapsed text that KEEPS line breaks —
    lets an output witness require its own line (the typed echo already
    contains the marker text without a leading newline)."""
    return re.sub(r"[ \t]+", "", ANSI_RE.sub("", s))


def wait_prompt_advance(sess: ConptySession, want: int, marker: str,
                        input_row: str, since_pos: int, timeout_s: float,
                        settle_s: float) -> tuple[str | None, float]:
    """One bounded wait for 'the prompt advanced' with three witnesses,
    first hit wins (niu aborts a whole PROMPT_COMMAND chain on a failing
    element, so the sentinel legitimately never arrives for some themes):
      'sentinel'        raw prompt-sentinel count reached `want`
      'output-fallback' the command's output marker came back on its own
                        line among bytes arriving after ENTER
      'row-fallback'    the bottom row stopped being the typed input line
    Returns (witness, elapsed_ms)."""
    target = _canon(marker)
    start = time.perf_counter()
    deadline = start + timeout_s
    while time.perf_counter() < deadline:
        _, last_change, now, count = sess.snapshot()
        if count >= want:
            wait_quiet(sess, settle_s, 3.0)
            return "sentinel", (time.perf_counter() - start) * 1000.0
        if target and f"\n{target}" in _canon_keepnl(sess.raw_since(since_pos)):
            return "output-fallback", (now - start) * 1000.0
        if input_row and last_nonempty_row(sess.snapshot()[0]) != input_row \
                and (now - last_change) >= 0.2:
            return "row-fallback", (now - start) * 1000.0
        if not sess.proc.isalive():
            return None, (time.perf_counter() - start) * 1000.0
        time.sleep(0.01)
    return None, (time.perf_counter() - start) * 1000.0


def wait_raw_contains(sess: ConptySession, pos: int, needle: str,
                      timeout_s: float) -> tuple[bool, float]:
    start = time.perf_counter()
    deadline = start + timeout_s
    while time.perf_counter() < deadline:
        if needle in sess.raw_since(pos):
            return True, (time.perf_counter() - start) * 1000.0
        if not sess.proc.isalive():
            return False, (time.perf_counter() - start) * 1000.0
        time.sleep(0.01)
    return False, (time.perf_counter() - start) * 1000.0


def wait_echo(sess: ConptySession, pos: int, text: str,
              timeout_s: float) -> tuple[bool, float]:
    """Wait for the typed text to come back. The EMULATED SCREEN is the
    primary witness: readline re-echoes with per-keystroke ANSI repaints
    (echo -> echo\\x1b[0m T -> echo\\x1b[0m T1), so the raw stream usually
    never contains the contiguous text. Both witnesses are matched after
    ANSI-stripping and whitespace collapse (long lines hard-wrap)."""
    target = _canon(text)
    start = time.perf_counter()
    deadline = start + timeout_s
    while time.perf_counter() < deadline:
        if target in _canon(sess.screen_text()) or \
                target in _canon(sess.raw_tail(8192)):
            return True, (time.perf_counter() - start) * 1000.0
        if not sess.proc.isalive():
            return False, (time.perf_counter() - start) * 1000.0
        time.sleep(0.01)
    return False, (time.perf_counter() - start) * 1000.0


# ---------------------------------------------------------------------------
# One cell = one interactive session walking the step journey
# ---------------------------------------------------------------------------

def run_cell_session(niu: Path, rc_path: Path, cwd: Path, env: dict,
                     theme: Theme | None, shape: str,
                     args) -> dict:
    cols, rows = args.cols, args.rows
    step_timeout = args.step_timeout
    settle_s = args.settle_ms / 1000.0
    steps: list[dict] = []

    def record(step, status, ms, detection="", detail=""):
        steps.append({"step": step, "status": status, "ms": round(ms, 1),
                      "detection": detection, "detail": detail[:300]})

    sess = None
    crash = None
    exit_clean = False
    hang_step = None
    first_key_masked = False
    t_start = time.perf_counter()

    def finish():
        nonlocal sess, exit_clean
        if sess is None:
            return
        if sess.proc.isalive() and hang_step is None and crash is None:
            sess.write("exit\r")
            grace = time.monotonic() + args.exit_timeout
            while sess.proc.isalive() and time.monotonic() < grace:
                time.sleep(0.05)
            exit_clean = not sess.proc.isalive()
        if sess.proc.isalive():
            sess.close()
            time.sleep(0.1)
            if sess.proc.isalive():
                try:
                    subprocess.run(["taskkill", "/PID", str(sess.proc.pid), "/T", "/F"],
                                   capture_output=True, timeout=15)
                except (OSError, subprocess.SubprocessError):
                    pass
                sess.alive = False

    def typed_send(label: str, text: str) -> None:
        """Type `text`, verify its echo, ENTER, wait for the next prompt.
        One masking-tolerant retry: a swallowed first key is recorded, then
        recovered with a kill-line so the SESSION signal stays clean."""
        nonlocal first_key_masked, hang_step, crash
        if hang_step is not None:
            record(label + "-type", "skipped", 0.0, detail="session already stalled")
            record(label + "-enter", "skipped", 0.0, detail="session already stalled")
            return
        want = sess.snapshot()[3] + 1
        pos = sess.raw_pos()
        time.sleep(args.type_delay_ms / 1000.0)
        sess.write(text)
        ok, ms = wait_echo(sess, pos, text, step_timeout)
        if not ok:
            canon_screen = _canon(sess.screen_text())
            canon_tail = _canon(sess.raw_tail(8192))
            if _canon(text[1:]) in canon_screen or _canon(text[1:]) in canon_tail:
                # first key swallowed by a repaint race
                first_key_masked = True
                record(label + "-type", "masked-first-key", ms,
                       detail=f"echo shows '{text[1:]}' — first byte swallowed")
                sess.write("\x15")  # kill line, recover the session
                time.sleep(0.1)
                pos = sess.raw_pos()
                sess.write(text)
                ok, ms2 = wait_echo(sess, pos, text, step_timeout)
                ms += ms2
        if ok:
            record(label + "-type", "ok", ms, detection="echo")
        else:
            hang_step = label + "-type"
            record(label + "-type", "timeout", ms,
                   detail=f"no echo of typed text in {step_timeout}s; "
                          f"alive={sess.proc.isalive()}")
            record(label + "-enter", "skipped", 0.0, detail="type stalled")
            return
        # ENTER -> next prompt (three witnesses, one bound)
        marker = text.split()[-1]
        input_row = last_nonempty_row(sess.snapshot()[0])
        pos = sess.raw_pos()
        sess.write("\r")
        witness, ms = wait_prompt_advance(sess, want, marker, input_row,
                                          pos, step_timeout, settle_s)
        if witness == "sentinel":
            record(label + "-enter", "ok", ms, detection="sentinel")
            return
        if witness == "output-fallback":
            record(label + "-enter", "ok", ms, detection="output-fallback",
                   detail="prompt sentinel missing (PROMPT_COMMAND overwritten?)")
            return
        if witness == "row-fallback":
            record(label + "-enter", "ok", ms, detection="prompt-row-fallback",
                   detail="prompt sentinel absent after ENTER — "
                          "PROMPT_COMMAND chain failed/rebuilt")
            return
        if not sess.proc.isalive():
            crash = f"process died during {label}-enter"
            record(label + "-enter", "error", ms, detail=crash)
            return
        hang_step = label + "-enter"
        record(label + "-enter", "timeout", ms,
               detail=f"no next prompt in {step_timeout}s; "
                      f"alive={sess.proc.isalive()}")

    try:
        try:
            sess = ConptySession([str(niu), "--rcfile", str(rc_path), "-i"],
                                 cwd, env, cols, rows)
        except (OSError, ValueError) as exc:
            crash = f"conpty spawn failed: {exc}"
            record("boot", "error", 0.0, detail=crash)

        # -- boot: rc bootstrap sentinels -----------------------------------
        if hang_step is None and crash is None:
            ok, ms = wait_raw_contains(sess, 0, f"{SENT_BOOT} phase=post",
                                       step_timeout)
            if ok:
                record("boot", "ok", ms, detection="sentinel")
            else:
                hang_step = "boot"
                record("boot", "timeout", ms,
                       detail=f"rc bootstrap sentinel not seen in {step_timeout}s; "
                              f"alive={sess.proc.isalive()}")

        # -- first prompt ----------------------------------------------------
        if hang_step is None and crash is None:
            boot_row = last_nonempty_row(sess.snapshot()[0])
            ok, ms = wait_prompt_count(sess, 1, step_timeout, settle_s)
            if not ok and sess.proc.isalive():
                # Fallback: a theme whose PROMPT_COMMAND aborts (expansion
                # error) can take the sentinel down with it — niu stops the
                # whole PROMPT_COMMAND chain on a failing element while GNU
                # bash runs the remaining elements. If the screen moved past
                # the rc bootstrap row, a prompt rendered; the journey
                # continues and later steps carry the evidence. A true hang
                # shows no screen movement at all.
                ok2, ms2 = wait_prompt_returned(sess, boot_row, 5.0)
                if ok2:
                    ok = True
                    changed_at = sess.snapshot()[1]
                    ms = (changed_at - sess.spawn_at) * 1000.0
                    record("first_prompt", "ok", ms,
                           detection="prompt-row-fallback",
                           detail="prompt sentinel absent — theme "
                                  "PROMPT_COMMAND failed or was rebuilt; "
                                  "a prompt row rendered")
            if ok:
                if not steps or steps[-1]["step"] != "first_prompt":
                    record("first_prompt", "ok", ms, detection="sentinel")
            else:
                hang_step = "first_prompt"
                record("first_prompt", "timeout", ms,
                       detail=f"no prompt sentinel in {step_timeout}s; "
                              f"alive={sess.proc.isalive()}")

        # -- journey ---------------------------------------------------------
        typed_send("t1", "echo T1")
        typed_send("t2", "echo T2")

        if hang_step is None and crash is None and shape == "resrc" \
                and theme is not None:
            src_line = ". " + sh_quote(theme.source_file.as_posix())
            pos = sess.raw_pos()
            time.sleep(args.type_delay_ms / 1000.0)
            sess.write(src_line)
            ok, ms = wait_echo(sess, pos, src_line, step_timeout)
            if ok:
                record("resrc-type", "ok", ms, detection="echo")
                want = sess.snapshot()[3] + 1
                input_row = last_nonempty_row(sess.snapshot()[0])
                sess.write("\r")
                # Re-sourcing a theme can rebuild PROMPT_COMMAND (bash-it
                # safe_append, omb theme replacement) and drop the sentinel
                # while the prompt still renders fine — one bound, three
                # witnesses like every other ENTER.
                witness, ms = wait_prompt_advance(sess, want, "", input_row,
                                                  sess.raw_pos(), step_timeout,
                                                  settle_s)
                if witness == "sentinel":
                    record("resrc-enter", "ok", ms, detection="sentinel")
                elif witness is not None:
                    record("resrc-enter", "ok", ms,
                           detection="prompt-returned-fallback",
                           detail="prompt sentinel lost on re-source "
                                  "(PROMPT_COMMAND rebuilt)")
                if witness is not None:
                    typed_send("t3", "echo T3")
                else:
                    hang_step = "resrc-enter"
                    record("resrc-enter", "timeout", ms,
                           detail=f"no prompt after re-source in {step_timeout}s; "
                                  f"alive={sess.proc.isalive()}")
            else:
                hang_step = "resrc-type"
                record("resrc-type", "timeout", ms,
                       detail=f"no echo of re-source line in {step_timeout}s; "
                              f"alive={sess.proc.isalive()}")

    finally:
        finish()

    # -- verdict ----------------------------------------------------------
    if sess is None:
        return {
            "steps": steps, "verdict": "CRASH", "stall_step": None, "crash": crash,
            "exit_clean": False, "boot_hc_lines": [],
            "error_count": 0, "error_samples": [], "escape_leak_count": 0,
            "first_key_masked": False, "prompt_sentinels_seen": 0,
            "render_notes": [], "session_wall_s": round(time.perf_counter() - t_start, 2),
        }

    display, _, _, prompt_total = sess.snapshot()
    cur = sess.cursor()
    blank_screen = not any(ln.strip() for ln in display)
    render_notes = []
    if not (0 <= cur[0] <= cols and 0 <= cur[1] <= rows):
        render_notes.append(f"cursor out of bounds: {cur}")
    if blank_screen and hang_step is None and crash is None:
        render_notes.append("screen blank with live session")
    if sess.escape_leak_count >= 5:
        render_notes.append(f"escape leak fragments: {sess.escape_leak_count}")

    error_count = len(sess.error_events)
    wall = round(time.perf_counter() - t_start, 2)
    rec = {
        "steps": steps,
        "stall_step": hang_step,
        "crash": crash,
        "exit_clean": exit_clean,
        "boot_hc_lines": sess.boot_lines[:4],
        "error_count": error_count,
        "error_samples": sess.error_events[:6],
        "escape_leak_count": sess.escape_leak_count,
        "first_key_masked": first_key_masked,
        "prompt_sentinels_seen": prompt_total,
        "render_notes": render_notes,
        "session_wall_s": wall,
    }

    if crash:
        rec["verdict"] = "CRASH"
    elif hang_step is not None:
        rec["verdict"] = "HANG"
    elif error_count >= args.error_storm_threshold:
        rec["verdict"] = "ERROR-STORM"
    elif render_notes:
        rec["verdict"] = "RENDER-BROKEN"
    else:
        ok_ms = [s["ms"] for s in steps if s["status"] == "ok"]
        rec["worst_ok_step_ms"] = round(max(ok_ms), 1) if ok_ms else None
        if any(s["ms"] > args.slow_ms for s in steps if s["status"] == "ok"):
            rec["verdict"] = "OK-SLOW"
        else:
            rec["verdict"] = "OK-FAST"

    # Capture evidence for anything not OK-FAST (bounded, for the findings).
    if rec["verdict"] != "OK-FAST":
        rec["raw_tail"] = sess.raw_tail(args.tail_bytes)
        rec["screen_tail"] = [ln.rstrip() for ln in display if ln.strip()][-8:]
    return rec


# ---------------------------------------------------------------------------
# Cell/group driver
# ---------------------------------------------------------------------------

def sanitize(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]", "_", name)


def parse_pair_shapes(shapes: list[str]) -> list[str]:
    """Pair tasks run the requested shapes (fresh+aged default); a pair
    reduced to one shape degrades to a single session on the same home."""
    return [s for s in ("fresh", "aged") if s in shapes]


def cell_key(framework: str, name: str, shape: str, hc: str, hs: int) -> str:
    return f"{framework}/{name}/{shape}/hc={hc};hs={hs}"


def enumerate_tasks(themes: list[Theme], controls: bool, histcontrols,
                    histscales, kinds) -> list[dict]:
    """Task unit: a fresh+aged PAIR over one home (sequential, same worker),
    plus independent resrc singles and control singles."""
    tasks = []
    for t in themes:
        for hs in histscales:
            for hc in histcontrols:
                if "pair" in kinds:
                    tasks.append({"kind": "pair", "theme": t, "hc": hc, "hs": hs})
                if "resrc" in kinds:
                    tasks.append({"kind": "resrc", "theme": t, "hc": hc, "hs": hs})
    if controls:
        for fw, name in (("bare", "(bare-control)"),
                         ("omb", "(omb-notheme-control)"),
                         ("bashit", "(bashit-notheme-control)")):
            t = Theme(fw, name, Path("-"), [])
            for hs in histscales:
                for hc in histcontrols:
                    tasks.append({"kind": "single", "theme": t, "hc": hc, "hs": hs})
    return tasks


def task_keys(task: dict, shapes: list[str]) -> list[str]:
    t = task["theme"]
    if task["kind"] == "pair":
        return [cell_key(t.framework, t.name, s, task["hc"], task["hs"])
                for s in parse_pair_shapes(shapes)]
    shape = "resrc" if task["kind"] == "resrc" else "fresh"
    return [cell_key(t.framework, t.name, shape, task["hc"], task["hs"])]


def run_task(task: dict, args, niu: Path, rundir: Path, fixture: Path,
             omb: Path | None, bashit_rw: Path | None,
             hist_templates: dict, worker: int) -> list[dict]:
    theme = task["theme"]
    hc, hs, kind = task["hc"], task["hs"], task["kind"]
    is_control = theme.name.startswith("(")
    control_kind = "bare" if theme.framework == "bare" else \
        ("fw" if is_control else "theme")

    if kind == "resrc":
        home = rundir / "homes" / (f"{theme.framework}--{sanitize(theme.name)}"
                                   f"--resrc--hc-{hc}--hs-{hs}")
    elif kind == "single":
        home = rundir / "homes" / (f"control--{theme.framework}--hc-{hc}--hs-{hs}")
    else:
        home = rundir / "homes" / (f"{theme.framework}--{sanitize(theme.name)}"
                                   f"--hc-{hc}--hs-{hs}")
    home.mkdir(parents=True, exist_ok=True)

    histfile = home / "history"
    rc_path = home / "sweep.rc"
    rc_path.write_text(
        build_rc(theme.framework, None if is_control else theme, control_kind,
                 hc, omb if theme.framework == "omb" else None,
                 bashit_rw if theme.framework == "bashit" else None, home,
                 histfile),
        encoding="utf-8", newline="\n")
    env = sandbox_env(os.environ, home)

    if kind == "pair":
        shapes = parse_pair_shapes(args.shapes)
    else:
        shapes = ["resrc" if kind == "resrc" else "fresh"]
    records = []
    for shape in shapes:
        hist_before = None
        if not is_control and shape in ("fresh", "resrc"):
            # Both session kinds start from the seeded scale: a re-source
            # session must face the same HISTFILE scale as a fresh one.
            shutil.copy2(hist_templates[hs], histfile)
            hist_before = hist_templates[hs].read_text(encoding="utf-8").count("\n")
        rec = run_cell_session(niu, rc_path, fixture, env,
                               None if is_control else theme, shape, args)
        hist_after = None
        if histfile.is_file():
            try:
                hist_after = histfile.read_text(encoding="utf-8",
                                                errors="replace").count("\n")
            except OSError:
                pass
        out = {
            "key": cell_key(theme.framework, theme.name, shape, hc, hs),
            "framework": theme.framework,
            "theme": theme.name,
            "control": is_control,
            "shape": shape,
            "histcontrol_requested": hc,
            "histscale": hs,
            "histfile_lines_before": hist_before,
            "histfile_lines_after": hist_after,
            "worker": worker,
        }
        out.update(rec)
        records.append(out)

    if not args.keep_homes:
        shutil.rmtree(home, ignore_errors=True)
    return records


# ---------------------------------------------------------------------------
# Summary / aggregation
# ---------------------------------------------------------------------------

def summarize(records: list[dict], meta: dict, themes: list[Theme]) -> dict:
    per_theme = {}
    controls = []
    for c in records:
        if c.get("control"):
            controls.append(c)
            continue
        t = per_theme.setdefault(c["framework"] + "|" + c["theme"], {
            "theme": c["theme"], "framework": c["framework"],
            "cells_total": 0, "verdicts": {}, "worst_ok_step_ms": None,
            "hang_cells": [], "crash_cells": [], "storm_cells": [],
            "render_cells": [], "slow_cells": 0, "masked_first_key_cells": 0,
            "unclean_exits": 0,
        })
        t["cells_total"] += 1
        v = c.get("verdict", "DRIVER-ERROR")
        t["verdicts"][v] = t["verdicts"].get(v, 0) + 1
        if v in ("OK-FAST", "OK-SLOW"):
            w = c.get("worst_ok_step_ms")
            if w is not None and (t["worst_ok_step_ms"] is None
                                  or w > t["worst_ok_step_ms"]):
                t["worst_ok_step_ms"] = w
        if v == "OK-SLOW":
            t["slow_cells"] += 1
        elif v == "HANG":
            t["hang_cells"].append({"shape": c["shape"], "hc": c["histcontrol_requested"],
                                    "hs": c["histscale"], "stall_step": c.get("stall_step"),
                                    "session_wall_s": c.get("session_wall_s")})
        elif v == "CRASH":
            t["crash_cells"].append({"shape": c["shape"], "hc": c["histcontrol_requested"],
                                     "hs": c["histscale"], "detail": c.get("crash")})
        elif v == "ERROR-STORM":
            t["storm_cells"].append({"shape": c["shape"], "hc": c["histcontrol_requested"],
                                     "hs": c["histscale"],
                                     "error_count": c.get("error_count"),
                                     "samples": c.get("error_samples", [])[:3]})
        elif v == "RENDER-BROKEN":
            t["render_cells"].append({"shape": c["shape"], "hc": c["histcontrol_requested"],
                                      "hs": c["histscale"],
                                      "notes": c.get("render_notes")})
        if c.get("first_key_masked"):
            t["masked_first_key_cells"] += 1
        if not c.get("exit_clean", True):
            t["unclean_exits"] += 1

    def cell_dims(c):
        return {"hc": c["histcontrol_requested"], "hs": c["histscale"],
                "shape": c["shape"]}

    for t in per_theme.values():
        bad = t["hang_cells"] + t["crash_cells"] + t["storm_cells"] + t["render_cells"]
        ok_cells = [c for c in records
                    if not c.get("control")
                    and c["framework"] == t["framework"] and c["theme"] == t["theme"]
                    and c.get("verdict") in ("OK-FAST", "OK-SLOW")]
        t["verdict_changed_between_cells"] = bool(bad) and bool(ok_cells)
        for dim in ("hc", "hs", "shape"):
            hang_vals = sorted({b[dim] for b in bad}, key=str)
            ok_vals = sorted({cell_dims(c)[dim] for c in ok_cells}, key=str)
            t[f"bad_only_in_{dim}"] = [v for v in hang_vals if v not in ok_vals]

    theme_map = {t.key: t for t in themes}
    for t in per_theme.values():
        th = theme_map.get(t["framework"] + "/" + t["theme"])
        if th is not None:
            t["features"] = th.features
            t["family"] = classify_family(th)

    verdict_totals = {}
    for c in records:
        v = c.get("verdict", "DRIVER-ERROR")
        verdict_totals[v] = verdict_totals.get(v, 0) + 1

    ctrl_summary = {}
    for c in controls:
        k = f"{c['framework']}/hc={c['histcontrol_requested']};hs={c['histscale']}"
        v = c.get("verdict", "DRIVER-ERROR")
        e = ctrl_summary.setdefault(k, {"cells": 0, "verdicts": {},
                                        "worst_ok_step_ms": None})
        e["cells"] += 1
        e["verdicts"][v] = e["verdicts"].get(v, 0) + 1
        w = c.get("worst_ok_step_ms")
        if w is not None and (e["worst_ok_step_ms"] is None or w > e["worst_ok_step_ms"]):
            e["worst_ok_step_ms"] = w

    def worst_key(t):
        cur = max(t["verdicts"], key=t["verdicts"].get)
        rank = VERDICT_ORDER.index(cur) if cur in VERDICT_ORDER else 0
        return (rank, -(t["worst_ok_step_ms"] or 0), t["framework"], t["theme"])

    return {
        "meta": meta,
        "cell_totals": {"total": len(records), "by_verdict": verdict_totals},
        "controls": ctrl_summary,
        "themes": sorted(per_theme.values(), key=worst_key),
    }


def print_summary(summary: dict) -> None:
    print("\n=== all-themes interactive sweep summary ===")
    ct = summary["cell_totals"]
    print(f"cells: {ct['total']}  " +
          " ".join(f"{k}={v}" for k, v in sorted(ct["by_verdict"].items())))
    print(f"\n{'theme':34s} {'fw':6s} {'cells':>5s} {'verdicts':34s} "
          f"{'worst-ok-ms':>11s}  family")
    for t in summary["themes"]:
        vs = " ".join(f"{k}:{v}" for k, v in sorted(t["verdicts"].items()))
        print(f"{t['theme']:34s} {t['framework']:6s} {t['cells_total']:5d} "
              f"{vs:34s} {str(t['worst_ok_step_ms']):>11s}  {t.get('family', '')}")
    changed = [t for t in summary["themes"] if t["verdict_changed_between_cells"]]
    if changed:
        print(f"\nVERDICT CHANGES BETWEEN CELLS ({len(changed)} themes) — "
              "the conditionals are the product bugs:")
        for t in changed:
            print(f"  {t['framework']}/{t['theme']}: bad only in "
                  f"hc={t['bad_only_in_hc']} hs={t['bad_only_in_hs']} "
                  f"shape={t['bad_only_in_shape']}")
    bad_themes = [t for t in summary["themes"] if set(t["verdicts"]) & BAD_VERDICTS]
    print(f"\nthemes with bad cells: {len(bad_themes)}"
          + (" — GATE WOULD FAIL" if bad_themes else " — GATE PASS"))
    print("\ncontrols (transport/framework floor):")
    for k in sorted(summary["controls"]):
        e = summary["controls"][k]
        vs = " ".join(f"{k2}:{v2}" for k2, v2 in sorted(e["verdicts"].items()))
        print(f"  {k:44s} {vs:26s} worst-ok={e['worst_ok_step_ms']}ms")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def niu_version(niu: Path) -> str:
    try:
        r = subprocess.run([str(niu), "--version"], capture_output=True, text=True,
                           timeout=20)
        return (r.stdout or r.stderr).strip().splitlines()[0]
    except (subprocess.SubprocessError, OSError):
        return "unknown"


def run(args) -> int:
    niu = args.niu.resolve()
    if not niu.is_file():
        print(f"sweep: niu binary not found: {niu}", file=sys.stderr)
        return 2

    omb = args.omb.resolve() if args.omb else None
    bashit = args.bashit.resolve() if args.bashit else None
    themes: list[Theme] = []
    if args.frameworks in ("omb", "both") and omb:
        themes += discover_omb(omb)
    if args.frameworks in ("bashit", "both") and bashit:
        themes += discover_bashit(bashit)
    if args.theme_filter:
        filters = args.theme_filter
        themes = [t for t in themes if any(f in t.key for f in filters)]
    for t in themes:
        t.features = scan_features(t)
    if args.list:
        for t in themes:
            f = t.features
            print(f"{t.key:30s} prompt_cmd={int(f['appends_prompt_command'])} "
                  f"hist_reload={int(f['history_reload_calls'])} "
                  f"subshells={f['subshell_count']} net={f['network_touch']} "
                  f"cjk={int(f['has_cjk'])} minified={int(f['minified'])} "
                  f"clock={int(f['clock_redraw_risk'])} git={int(f['git_per_prompt'])}")
        print(f"TOTAL {len(themes)}")
        return 0
    if not themes:
        print("sweep: empty theme inventory (pass --omb/--bash-it)", file=sys.stderr)
        return 2

    rundir = (args.run_dir or Path(
        "E:/niubash-sweep/" + datetime.now().strftime("%Y%m%d-%H%M%S"))).resolve()
    rundir.mkdir(parents=True, exist_ok=True)
    cells_path = rundir / "cells.jsonl"
    done: dict[str, dict] = {}
    if args.resume and cells_path.is_file():
        for line in cells_path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                try:
                    rec = json.loads(line)
                    done[rec["key"]] = rec
                except (json.JSONDecodeError, KeyError):
                    continue

    tasks = enumerate_tasks(themes, args.controls, args.histcontrols,
                            args.histscales, args.kinds)
    todo = []
    for task in tasks:
        keys = task_keys(task, args.shapes)
        stale = [k for k in keys
                 if done.get(k) is None
                 or done[k].get("verdict") == "DRIVER-ERROR"]
        if stale or args.rerun_cells:
            todo.append(task)
    n_cells = sum(len(task_keys(t, args.shapes)) for t in tasks)
    print(f"sweep: {len(themes)} themes, {n_cells} matrix cells "
          f"({sum(len(task_keys(t, args.shapes)) for t in todo)} to run, {len(done)} resumed), "
          f"workers={args.workers}, step_timeout={args.step_timeout}s, niu={niu}",
          flush=True)

    fixture = make_fixture_repo(rundir)
    hist_templates = {hs: make_histfile(rundir / "cache", hs) for hs in HISTSCALES}
    workers = max(1, args.workers)
    worker_sandboxes = [
        (prepare_bashit_sandbox(bashit, rundir, w) if bashit else None)
        for w in range(workers)
    ]

    out_f = open(cells_path, "a", encoding="utf-8", newline="\n")
    lock = threading.Lock()
    t_all = time.perf_counter()
    results: list[dict] = list(done.values())
    done_count = [0]

    def work(item):
        idx, task, w = item
        recs = run_task(task, args, niu, rundir, fixture, omb,
                        worker_sandboxes[w], hist_templates, w)
        with lock:
            for rec in recs:
                out_f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                out_f.flush()
            done_count[0] += len(recs)
            for rec in recs:
                print(f"[{done_count[0]}/{n_cells}] {rec['key']:58s} "
                      f"{rec.get('verdict'):12s} stall={rec.get('stall_step')} "
                      f"worst_ok={rec.get('worst_ok_step_ms')}ms", flush=True)
        return recs

    try:
        with ThreadPoolExecutor(max_workers=workers) as pool:
            for recs in pool.map(work, [(i, t, i % workers)
                                        for i, t in enumerate(todo)]):
                results.extend(recs)
    finally:
        out_f.close()

    elapsed = round(time.perf_counter() - t_all, 1)
    meta = {
        "schema": "niubash:theme-interactive-sweep@1.0.0",
        "niu": str(niu),
        "niu_version": niu_version(niu),
        "omb": str(omb) if omb else None,
        "bashit": str(bashit) if bashit else None,
        "themes": len(themes),
        "matrix": {"histcontrols": args.histcontrols, "histscales": args.histscales,
                   "shapes": args.shapes, "kinds": args.kinds,
                   "resrc_scale": RESRC_SCALE},
        "step_timeout_s": args.step_timeout,
        "slow_ms": args.slow_ms,
        "error_storm_threshold": args.error_storm_threshold,
        "workers": workers,
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "wall_seconds": elapsed,
    }
    summary = summarize(results, meta, themes)
    summary_path = args.summary or (rundir / "summary.json")
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, ensure_ascii=False, indent=1),
                            encoding="utf-8", newline="\n")
    print_summary(summary)
    print(f"\ncells: {cells_path}\nsummary: {summary_path}\nwall: {elapsed}s")

    for sb in worker_sandboxes:
        if sb is not None:
            shutil.rmtree(sb, ignore_errors=True)

    if args.gate:
        bad = [t for t in summary["themes"] if set(t["verdicts"]) & BAD_VERDICTS]
        if bad:
            print(f"GATE: FAIL — {len(bad)} theme(s) with HANG/CRASH/"
                  "ERROR-STORM/RENDER-BROKEN cells")
            return 1
        print("GATE: PASS")
    return 0


def parse_args(argv) -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--niu", type=Path, default=REPO / "target" / "release" / "niu.exe")
    p.add_argument("--omb", type=Path,
                   default=Path(os.environ.get(
                       "NIU_SWEEP_OMB",
                       r"D:\repo\rubash\target\ecosweep-corpus\oh-my-bash-lf")))
    p.add_argument("--bashit", type=Path,
                   default=Path(os.environ.get(
                       "NIU_SWEEP_BASHIT", r"D:\repo\rubash\target\bit-real")))
    p.add_argument("--frameworks", choices=("both", "omb", "bashit"), default="both")
    p.add_argument("--histcontrols", default=",".join(HISTCONTROLS),
                   help="comma list subset of: " + ",".join(HISTCONTROLS))
    p.add_argument("--histscales", default=",".join(str(s) for s in HISTSCALES),
                   help="comma list of HISTFILE entry counts")
    p.add_argument("--shapes", default="fresh,aged",
                   help="pair shapes to run: fresh,aged (full-matrix default)")
    p.add_argument("--kinds", default="pair,resrc",
                   help="task kinds: pair (fresh+aged grid), resrc (re-source)")
    p.add_argument("--theme-filter", action="append", default=None,
                   help="substring(s) on fw/theme; repeatable, OR-matched")
    p.add_argument("--run-dir", type=Path, default=None,
                   help="artifact dir (default E:/niubash-sweep/<ts>)")
    p.add_argument("--summary", type=Path, default=None)
    p.add_argument("--workers", type=int, default=8)
    p.add_argument("--step-timeout", type=float, default=10.0,
                   help="hard bound per interactive step (owner spec: 10s)")
    p.add_argument("--exit-timeout", type=float, default=10.0)
    p.add_argument("--slow-ms", type=float, default=2000.0,
                   help="any completed step above this => OK-SLOW (owner spec: 2s)")
    p.add_argument("--error-storm-threshold", type=int, default=10)
    p.add_argument("--settle-ms", type=int, default=250,
                   help="screen-quiet window confirming a rendered prompt")
    p.add_argument("--type-delay-ms", type=int, default=200,
                   help="idle gap before typing (lets the prompt settle; "
                        "the #167 first-key race is reported, not masked)")
    p.add_argument("--cols", type=int, default=120)
    p.add_argument("--rows", type=int, default=36)
    p.add_argument("--tail-bytes", type=int, default=4096)
    p.add_argument("--controls", action=argparse.BooleanOptionalAction, default=True,
                   help="include bare + no-theme framework control cells")
    p.add_argument("--resume", action=argparse.BooleanOptionalAction, default=True)
    p.add_argument("--rerun-cells", action="store_true")
    p.add_argument("--keep-homes", action="store_true")
    p.add_argument("--gate", action="store_true",
                   help="exit 1 on any HANG/CRASH/ERROR-STORM/RENDER-BROKEN "
                        "(release-gate semantics; wire-up is a separate change)")
    p.add_argument("--list", action="store_true")
    args = p.parse_args(argv)
    args.histcontrols = [h.strip() for h in args.histcontrols.split(",") if h.strip()]
    args.histscales = [int(s) for s in args.histscales.split(",") if s.strip()]
    args.shapes = [s.strip() for s in args.shapes.split(",") if s.strip()]
    args.kinds = [k.strip() for k in args.kinds.split(",") if k.strip()]
    bad = (set(args.histcontrols) - set(HISTCONTROLS)) | \
          (set(args.shapes) - set(SHAPES)) | (set(args.kinds) - {"pair", "resrc"})
    if bad:
        p.error(f"unknown matrix values: {sorted(bad)}")
    return args


def main() -> int:
    return run(parse_args(sys.argv[1:]))


if __name__ == "__main__":
    sys.exit(main())
