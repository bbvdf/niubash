#!/usr/bin/env python3
"""Offline ConPTY probe: the one-run out-of-box wizard journey.

Owner ruling 2026-10-03 (装完即选主题): a fresh user who picks the
'recommended' plugin collection gets the theme pick in the SAME `niu
setup` run — the wizard trusts the freshly installed source on an
explicit yes, then offers its theme gallery. This probe drives exactly
that journey over a real ConPTY (pywinpty + pyte, the
scripts/test_setup_wizard_pty.py pattern):

    fresh HOME → Q1 empty-gallery note → recommended collection →
    Apply → untrusted install → trust-now question → pick agnoster →
    rc rewritten → fresh `niu -c` shows OSH_THEME=agnoster.

Everything runs inside a throwaway sandbox; the collection's `git
clone`s resolve through a seeded local mirror (mirrors.toml insteadOf
rewrite — the documented git-transport mirror layer), so no network, no
real HOME, no real plugin/mirror state is touched.

Exit codes: 0 pass, 1 fail, 2 skip (missing python deps / git / pty).

Usage: python scripts/smoke-wizard-journey.py <niu.exe> [sandbox-root]
       (called by scripts/smoke-test-1.3.0.sh leg D2)
"""

import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

try:
    import pyte
    from winpty import PtyProcess
except ImportError as err:  # pragma: no cover - environment guard
    print(f"SKIP: python deps missing ({err}); needs pywinpty + pyte")
    sys.exit(2)

REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "tests" / "fixtures" / "sources"

# Keys sent to the wizard's menus. Menus drop input queued while they
# draw (interactive_menu drains the console queue after cursor::position
# returns), so every answer settles before pressing — send-too-early
# keys are silently discarded and the menu would hang. The digit and the
# Enter go as separate writes (the scripts/test_setup_wizard_pty.py
# pattern): one burst can lose the trailing Enter on the ConPTY bridge.
SETTLE_SECONDS = 0.5
ENTER_GAP_SECONDS = 0.2
UP = "\x1b[A"
ENTER = "\r"


class Session:
    """A niu.exe under ConPTY with a pyte-parsed screen."""

    def __init__(self, argv, cwd, env, cols=120, rows=36):
        self.proc = PtyProcess.spawn(argv, cwd=str(cwd), env=env,
                                     dimensions=(rows, cols))
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.Stream(self.screen)
        self._reader = threading.Thread(target=self._pump, daemon=True)
        self._reader.start()

    def _pump(self):
        while self.proc.isalive():
            try:
                data = self.proc.read()
            except Exception:
                break
            if data:
                self.stream.feed(data)

    def text(self) -> str:
        return "\n".join(self.screen.display)

    def wait_for(self, *needles, timeout=60):
        deadline = time.time() + timeout
        while time.time() < deadline:
            body = self.text()
            for needle in needles:
                if needle in body:
                    return needle
            time.sleep(0.05)
        raise TimeoutError(
            f"timed out waiting for {needles}; screen:\n{self.text()}"
        )

    def answer(self, keys):
        """Settle out the menu draw, then press."""
        time.sleep(SETTLE_SECONDS)
        if keys.endswith(ENTER) and len(keys) > 1:
            self.proc.write(keys[:-1])
            time.sleep(ENTER_GAP_SECONDS)
            self.proc.write(ENTER)
        else:
            self.proc.write(keys)

    def close(self):
        try:
            self.proc.terminate(force=True)
        except Exception:
            pass


def seed_mirror_repo(git: Path, src: Path, dest: Path):
    """`dest` becomes a real git repo holding the fixture tree — the
    offline mirror niu's `git clone` resolves to."""
    shutil.copytree(src, dest)
    for args in (
        ["init", "-q"],
        ["-c", "user.email=smoke@niu", "-c", "user.name=niu-smoke", "add", "-A"],
        ["-c", "user.email=smoke@niu", "-c", "user.name=niu-smoke",
         "commit", "-qm", "seed"],
    ):
        done = subprocess.run(
            [str(git), "-C", str(dest), *args],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        if done.returncode != 0:
            raise RuntimeError(
                f"git {args[0]} failed: {done.stderr.decode(errors='replace')}"
            )


def run_journey(exe: Path, root: Path) -> None:
    home = root / "home"
    sources = root / "sources"
    mirror = root / "mirror"
    (home / ".niubash").mkdir(parents=True, exist_ok=True)
    sources.mkdir(parents=True, exist_ok=True)

    git = shutil.which("git")
    if git is None:
        print("SKIP: git not on PATH (the offline mirror needs it)")
        sys.exit(2)
    git = Path(git)
    # Mirror layout matches the canonical recipe origins so the insteadOf
    # rewrite lands on these trees: ohmybash/oh-my-bash.git,
    # scop/bash-completion.git.
    seed_mirror_repo(git, FIXTURES / "oh-my-bash",
                      mirror / "ohmybash" / "oh-my-bash.git")
    seed_mirror_repo(git, FIXTURES / "bash-completion",
                      mirror / "scop" / "bash-completion.git")
    mirror_base = mirror.resolve().as_posix()
    (home / ".niubash" / "mirrors.toml").write_text(
        "# smoke journey: rewrite GitHub fetches to the seeded local mirror\n"
        'schema = "niubash:mirrors@0.1.0"\n'
        'active = "custom"\n\n'
        "[github]\n"
        f'git_instead_of = "file:///{mirror_base}/"\n',
        encoding="utf-8",
    )

    system_root = os.environ.get("SystemRoot", r"C:\Windows")
    env = {
        "SystemRoot": system_root,
        "COMSPEC": system_root + r"\System32\cmd.exe",
        "HOME": str(home),
        "USERPROFILE": str(home),
        "LOCALAPPDATA": str(home / "local-appdata"),
        "APPDATA": str(home / "appdata"),
        "TEMP": str(home / "tmp"),
        "TMP": str(home / "tmp"),
        "TERM": "xterm",
        "NIU_LANG": "en",
        "NIU_PLUGIN_SOURCES_ROOT": str(sources),
        "NIU_PLUGIN_SPEC": str(home / ".niubash" / "plugins.toml"),
        "NIU_MIRRORS": str(home / ".niubash" / "mirrors.toml"),
        "PATH": os.pathsep.join([
            str(git.parent), system_root + r"\System32", system_root,
        ]),
    }
    (home / "tmp").mkdir(exist_ok=True)

    s = Session([str(exe), "setup"], home, env)
    try:
        # Q1: the empty-ecosystem note (no menu to answer).
        s.wait_for("No external themes installed yet")
        # Q2.5: the recommended collection is displayed option 3.
        s.wait_for("Plugin collection?")
        s.answer("3\r")
        if os.name == "nt":
            s.wait_for("niu-git")
            s.answer(ENTER)  # default Skip
        s.wait_for("Apply this configuration?")
        s.answer(ENTER)  # Apply is the highlighted default
        # Post-install trust question: Trust now is option 2.
        s.wait_for("to list its themes?")
        s.answer("2\r")
        # The gallery from the freshly trusted source: agnoster follows
        # Skip.
        s.wait_for("Pick a theme")
        s.answer("2\r")
        # Completion = the wizard process exiting (it prints its finish
        # screen and returns). winpty may drop tail bytes at process exit,
        # so the screen tail is not assertable here — the durable receipts
        # are the files below; the finish screen itself is asserted by the
        # Rust mirror of this journey (tests/smoke_1_3_0.rs d2).
        deadline = time.time() + 60
        while s.proc.isalive() and time.time() < deadline:
            time.sleep(0.1)
        if s.proc.isalive():
            raise AssertionError(
                "wizard still running after the theme pick:\n" + s.text()
            )
    except (TimeoutError, AssertionError) as err:
        raise AssertionError(f"journey stalled: {err}") from err
    finally:
        s.close()

    # The same run wrote the guarded activation block into the rc.
    rc = (home / ".niubashrc").read_text(encoding="utf-8")
    assert "OSH_THEME='agnoster'" in rc, f"rc misses the theme: {rc}"
    journal = (home / ".niubash" / "setup-journal.toml").read_text(
        encoding="utf-8")
    assert "theme = 'agnoster'" in journal, journal
    assert "collection = 'recommended'" in journal, journal

    # 1.3.1: the journey ends spec-managed — the post-pick adoption
    # declared the collection's sources (canonical origins; the mirror is
    # transport-only) with the picked theme snapshotted.
    spec = (home / ".niubash" / "plugins.toml").read_text(encoding="utf-8")
    assert "https://github.com/ohmybash/oh-my-bash.git" in spec, spec
    assert "https://github.com/scop/bash-completion.git" in spec, spec
    assert "theme = 'agnoster'" in spec, spec

    # And a fresh shell sees the theme.
    probe = subprocess.run(
        [str(exe), "-c", ". ~/.niubashrc; echo $OSH_THEME"],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=60,
    )
    got = probe.stdout.decode(errors="replace").strip()
    assert got == "agnoster", (
        f"fresh shell saw OSH_THEME={got!r}, expected 'agnoster': "
        f"{probe.stderr.decode(errors='replace')}"
    )


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    exe = Path(sys.argv[1])
    if not exe.is_file():
        print(f"SKIP: niu binary not found: {exe}")
        return 2
    parent = Path(sys.argv[2]) if len(sys.argv) > 2 else None
    if parent is not None:
        parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="journey-", dir=parent) as tmp:
        try:
            run_journey(exe, Path(tmp))
        except AssertionError as err:
            print(f"FAIL: {err}")
            return 1
        print("PASS: one-run journey — recommended → trust → agnoster "
              "→ OSH_THEME=agnoster in a fresh shell")
        return 0


if __name__ == "__main__":
    sys.exit(main())
