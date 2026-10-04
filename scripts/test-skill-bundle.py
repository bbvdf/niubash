#!/usr/bin/env python3
"""Golden check for the niubash agent skill bundle (niubash#188).

Two invariants, mirroring scripts/test-winuxcmd-completions.py:

1. Generated regions match their sources. Re-runs scripts/generate-skill.py
   in --check mode: the regions between the GENERATED markers in
   skills/niubash/ must be exactly what the corpus transcripts and the
   winuxcmd applet inventory produce. Drift means the corpus is stale
   relative to a product change or a generated region was hand-edited.

2. The binary embeds the whole bundle. Every file under skills/niubash/
   must appear as an include_str! entry in the launcher's skill manifest
   (src/skill.rs), and every manifest entry must exist on disk —
   `niu skill install` ships exactly the committed bundle, never a stale
   subset.

Usage:
    python scripts/test-skill-bundle.py

Exits 0 with `PASS skill-bundle` when both hold.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GENERATOR = ROOT / "scripts" / "generate-skill.py"
SKILL_DIR = ROOT / "skills" / "niubash"
MANIFEST = ROOT / "src" / "skill.rs"

INCLUDE_RE = re.compile(r'include_str!\("\.\./skills/niubash/(?P<rel>[^"]+)"\)')


def check_generated() -> bool:
    proc = subprocess.run(
        [sys.executable, str(GENERATOR), "--check"], capture_output=True, text=True
    )
    if proc.stdout:
        print(proc.stdout, end="")
    if proc.stderr:
        print(proc.stderr, file=sys.stderr, end="")
    return proc.returncode == 0


def check_manifest() -> bool:
    ok = True
    manifest_text = MANIFEST.read_text(encoding="utf-8")
    embedded = {
        rel.replace("\\", "/") for rel in INCLUDE_RE.findall(manifest_text)
    }
    on_disk = {
        str(path.relative_to(SKILL_DIR)).replace("\\", "/")
        for path in SKILL_DIR.rglob("*")
        if path.is_file()
    }
    for missing in sorted(on_disk - embedded):
        print(
            f"DRIFT {missing}: on disk under skills/niubash/ but not embedded "
            f"in {MANIFEST.name} — add an include_str! entry"
        )
        ok = False
    for stale in sorted(embedded - on_disk):
        print(
            f"DRIFT {stale}: embedded in {MANIFEST.name} but missing on disk "
            "under skills/niubash/"
        )
        ok = False
    return ok


def main() -> int:
    if not SKILL_DIR.is_dir():
        print(f"FAIL skill-bundle: skill dir missing: {SKILL_DIR}")
        return 1
    ok = check_generated() and check_manifest()
    if ok:
        print("PASS skill-bundle")
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
