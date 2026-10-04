#!/usr/bin/env python3
"""Golden check for the embedded WinuxCmd completion assets.

Regenerates the completion TOMLs and the Rust asset module from the committed
`--help` transcript corpus (`crates/niubash-runtime/tests/fixtures/
winuxcmd-help-corpus/`) and diffs both against what is committed. Drift means
either the generator changed without regenerating the assets, or the assets
were hand-edited — both must be resolved by re-running the generator.

Usage:
    python scripts/test-winuxcmd-completions.py

Exits 0 with `PASS winuxcmd-completions` when the committed assets are
exactly what the generator produces from the corpus.
"""

from __future__ import annotations

import filecmp
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GENERATOR = ROOT / "scripts" / "generate-winuxcmd-completions.py"
CORPUS = ROOT / "crates" / "niubash-runtime" / "tests" / "fixtures" / "winuxcmd-help-corpus"
ASSETS = ROOT / "crates" / "niubash-runtime" / "assets" / "completions" / "winuxcmd"
RUST_MODULE = ROOT / "crates" / "niubash-runtime" / "src" / "completion" / "winuxcmd_assets.rs"


def main() -> int:
    if not CORPUS.is_dir():
        print(f"FAIL winuxcmd-completions: corpus dir missing: {CORPUS}")
        return 1

    with tempfile.TemporaryDirectory(prefix="niu-winuxcmd-golden-") as tmp:
        out_dir = Path(tmp) / "completions"
        module = Path(tmp) / "winuxcmd_assets.rs"
        proc = subprocess.run(
            [
                sys.executable,
                str(GENERATOR),
                "--from-corpus",
                str(CORPUS),
                "--out-dir",
                str(out_dir),
                "--rust-module",
                str(module),
            ],
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            print(f"FAIL winuxcmd-completions: generator exited {proc.returncode}")
            print(proc.stdout)
            print(proc.stderr)
            return 1

        drift = []

        generated_tomls = sorted(out_dir.glob("*.toml"))
        committed_tomls = sorted(ASSETS.glob("*.toml"))
        generated_names = [p.name for p in generated_tomls]
        committed_names = [p.name for p in committed_tomls]
        if generated_names != committed_names:
            missing = sorted(set(generated_names) - set(committed_names))
            extra = sorted(set(committed_names) - set(generated_names))
            drift.append(f"file set differs (missing={missing[:5]} extra={extra[:5]})")

        for path in generated_tomls:
            committed = ASSETS / path.name
            if not committed.is_file():
                continue
            if not filecmp.cmp(path, committed, shallow=False):
                drift.append(f"{path.name} differs")

        if not filecmp.cmp(module, RUST_MODULE, shallow=False):
            drift.append("winuxcmd_assets.rs differs")

    if drift:
        print("FAIL winuxcmd-completions: committed assets drifted from the corpus:")
        for item in drift[:20]:
            print(f"  - {item}")
        print("re-run: python scripts/generate-winuxcmd-completions.py --winuxcmd <exe>")
        return 1

    print(f"PASS winuxcmd-completions ({len(committed_names)} applet definitions)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
