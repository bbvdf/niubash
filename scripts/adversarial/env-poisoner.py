#!/usr/bin/env python3
"""env-poisoner.py -- launches a command under a deliberately hostile
environment (adversarial-matrix lane AM07, and the env variants of AM05).

The golden journey scrubs the environment before spawning niu; a real
user's terminal never does (niu#117: every prompt nags `__git_ps1: command
not found` because the shell inherited a Git Bash PS1). This tool is the
env-per-session capability made reusable: pick presets, run the product,
assert on what the CHILD sees.

Presets (composable, applied in order):
  gitbash          PS1 (Git Bash shape) + MSYSTEM=MINGW64 + SHELL=/usr/bin/bash
  both-homes       HOME points at dir A, USERPROFILE at dir B (the known
                   pitfall: this product prefers USERPROFILE)
  no-home          HOME and USERPROFILE both unset
  giant-var        a 64 KB environment value (terminal env bloat)
  crlf-values      PATH-like value with embedded CRLF + unicode dir name
  bash-env         BASH_ENV -> a file that echoes a marker (rc-injection probe)
  garbage-niu      NIU_LANG=zh_CN.bogus, NIU_THEME='%s%d%%', TERM=/
  msys-parent      MSYSTEM + SHELL + ORIGINAL_TEMP (the niu#141 shape)

Usage:
    python scripts/adversarial/env-poisoner.py --preset gitbash,garbage-niu \
        -- target/release/niu.exe -c 'echo ok'

Exit code = child's exit code; 2 on usage error. With --dry-run prints the
delta instead of running.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile

CJK_DIR = "工作区"  # unicode dir name for crlf-values


def preset_delta(name: str, sandbox: str) -> dict:
    if name == "gitbash":
        return {
            "PS1": r"\[\e]0;\u@\h: \w\a\]${debian_chroot:+($debian_chroot)}\[\e[32m\]\u@\h\[\e[33m\] \w",
            "MSYSTEM": "MINGW64",
            "SHELL": "/usr/bin/bash",
        }
    if name == "both-homes":
        return {"HOME": sandbox, "USERPROFILE": os.path.join(sandbox, "other-home")}
    if name == "no-home":
        return {"HOME": None, "USERPROFILE": None}
    if name == "giant-var":
        return {"NIU_GIANT": "x" * 65536}
    if name == "crlf-values":
        return {
            "NIU_CRLF_PATH": rf"C:\{CJK_DIR}\bin\r\nD:\temp\r\n",
            "NIU_UNICODE_HOME": os.path.join(sandbox, CJK_DIR),
        }
    if name == "bash-env":
        marker = os.path.join(sandbox, "bashenv-injected.sh")
        with open(marker, "w", encoding="utf-8", newline="\n") as fh:
            fh.write("echo BASH_ENV_RAN_MARKER\n")
        return {"BASH_ENV": marker}
    if name == "garbage-niu":
        return {"NIU_LANG": "zh_CN.bogus", "NIU_THEME": "%s%d%%", "TERM": "/"}
    if name == "msys-parent":
        return {
            "MSYSTEM": "MINGW64",
            "SHELL": "/usr/bin/bash",
            "ORIGINAL_TEMP": "C:/Users/ADMINI~1/AppData/Local/Temp",
            "TMP": "/tmp",
        }
    raise SystemExit(f"unknown preset '{name}'")


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--preset", required=True, help="comma-separated preset list")
    p.add_argument(
        "--sandbox",
        default=None,
        help="dir used by home presets (default: a temp dir; children must "
        "never see the real profile through this tool)",
    )
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("cmd", nargs=argparse.REMAINDER, help="-- <cmd> [args...]")
    args = p.parse_args(argv)

    cmd = args.cmd
    if cmd and cmd[0] == "--":
        cmd = cmd[1:]
    if not cmd:
        p.error("a child command is required: -- <cmd> [args...]")

    sandbox = args.sandbox or tempfile.mkdtemp(prefix="env-poison-")
    env = dict(os.environ)
    delta = {}
    for name in [s for s in args.preset.split(",") if s]:
        delta.update(preset_delta(name, sandbox))

    for k, v in delta.items():
        if v is None:
            env.pop(k, None)
        else:
            env[k] = v

    print("[env-poisoner] applied presets:", args.preset)
    for k in sorted(delta):
        v = delta[k]
        shown = "<unset>" if v is None else (v[:60] + ("..." if len(v) > 60 else ""))
        print(f"  {k} = {shown}")

    if args.dry_run:
        return 0

    try:
        proc = subprocess.run(cmd, env=env)
        return proc.returncode
    except FileNotFoundError as exc:
        print(f"[env-poisoner] child spawn failed: {exc}", file=sys.stderr)
        return 127


if __name__ == "__main__":
    sys.exit(main())
