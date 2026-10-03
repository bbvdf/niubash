#!/usr/bin/env python3
"""Generate the niu plugin recipe seed index (recipes.toml).

The recipe registry is pure data (mason-registry pattern: one registry file,
one row per asset, no behavior code). This script derives the bulk of the
seed — oh-my-bash and bash-it asset inventories — from the corpus
checkouts, plus a hand-curated standalone set (managers, prompts, tools).

Usage:
    python scripts/gen-plugin-recipes.py \
        --corpus D:/repo/rubash/target-ecosys/repos \
        --out crates/niubash-runtime/assets/plugins/recipes.toml

Re-run when the corpus moves; the committed TOML is the artifact.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

OMB_REPO = "https://github.com/ohmybash/oh-my-bash"
BASHIT_REPO = "https://github.com/Bash-it/bash-it"

ABOUT_ALIAS = re.compile(r"^about-alias ['\"](.+?)['\"]")
ABOUT_PLUGIN = re.compile(r"^about-plugin ['\"](.+?)['\"]")
ABOUT_COMPLETION = re.compile(r"^about-completion ['\"](.+?)['\"]")

# Hand-curated standalone recipes: the popular bash-family assets that are
# not inside a manager tree. Driver is one of: git (tree source via the
# named adapter kind) or download (direct binary via plugins::download).
STANDALONE = [
    # id, category, summary, license, url, driver, extra
    ("bash-preexec", "plugin",
     "precmd/preexec hook layer for bash (the zsh-preeq equivalent)",
     "MIT", "https://github.com/rcaloras/bash-preexec",
     {"driver": "git", "kind": "generic", "entry": "bash-preexec"}),
    ("liquidprompt", "prompt",
     "adaptive prompt: git/virtualenv/weather-aware, bash 3.2+",
     "AGPL-3.0-only", "https://github.com/liquidprompt/liquidprompt",
     {"driver": "git", "kind": "generic", "entry": "liquidprompt"}),
    ("bash-git-prompt", "prompt",
     "git status prompt line with themes (source gitprompt.sh)",
     "BSD-2-Clause", "https://github.com/magicmonty/bash-git-prompt",
     {"driver": "git", "kind": "generic", "entry": "gitprompt.sh"}),
    ("sexy-bash-prompt", "prompt",
     "minimal colored prompt (`make install` model; info-only in niu)",
     "MIT", "https://github.com/twolfson/sexy-bash-prompt",
     None),
    ("bpkg", "manager",
     "lightweight bash package manager (bpkg install owner/pkg)",
     "MIT", "https://github.com/bpkg/bpkg",
     {"driver": "git", "kind": "generic", "entry": "bpkg.sh"}),
    ("basher", "manager",
     "package manager with per-package bins (PATH + `basher init` model; info-only in niu)",
     "MIT", "https://github.com/basherpm/basher",
     None),
    ("ble.sh", "plugin",
     "line editor: syntax highlight, autosuggest, vim mode for bash (build from source)",
     "BSD-3-Clause", "https://github.com/akinomyoga/ble.sh",
     None),
    ("fzf", "plugin",
     "fuzzy finder with official bash key-bindings and completion",
     "MIT", "https://github.com/junegunn/fzf",
     {"driver": "download", "version": "v0.74.4", "bins": ["fzf"],
      "assets": {
          "windows-x64": ("https://github.com/junegunn/fzf/releases/download/v0.74.4/fzf-0.74.4-windows_amd64.zip",
                          "zip", "5e63c0e798406fcb9c51a9fed4988398e25fdabf8c670e32c16caf7b4a7ed02d"),
          "linux-x64": ("https://github.com/junegunn/fzf/releases/download/v0.74.4/fzf-0.74.4-linux_amd64.tar.gz",
                        "tar-gzip", "05e6813a337cc722c3ed07e54a764b75cc5d671e2e60459db0ba696ee5fa7504"),
      }}),
    ("starship", "prompt",
     "cross-shell prompt: fast, configurable, git/k8s/module aware",
     "ISC", "https://github.com/starship/starship",
     {"driver": "download", "version": "v1.26.0", "bins": ["starship"],
      "assets": {
          "windows-x64": ("https://github.com/starship/starship/releases/download/v1.26.0/starship-x86_64-pc-windows-msvc.zip",
                          "zip", "690021e22f2bf2c57d5867dc0fababe5e55e714c26942bd909ffddb6c5df42f2"),
          "linux-x64": ("https://github.com/starship/starship/releases/download/v1.26.0/starship-x86_64-unknown-linux-gnu.tar.gz",
                        "tar-gzip", "321f0dd7af8340a5f2e6a8fec6538a04f617486f9ec70d878f91c09cd8deef22"),
          "linux-arm64": ("https://github.com/starship/starship/releases/download/v1.26.0/starship-aarch64-unknown-linux-musl.tar.gz",
                          "tar-gzip", "dc30189378d2f2e287384e8a692d3f95ad1df64cf0e8c36aa9201516028aed6b"),
      }}),
]


def toml_str(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def first_summary(path: Path, pattern: re.Pattern) -> str | None:
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    for line in text.splitlines():
        line = line.strip()
        match = pattern.match(line)
        if match:
            return match.group(1)
    return None


def emit(out, recipe: dict) -> None:
    out.append("[[recipe]]")
    for key in ("id", "category", "summary", "license", "url"):
        if key in recipe:
            out.append(f"{key} = {toml_str(recipe[key])}")
    if recipe.get("driver"):
        out.append(f"driver = {toml_str(recipe['driver'])}")
    if recipe.get("kind"):
        out.append(f"kind = {toml_str(recipe['kind'])}")
    if recipe.get("entry"):
        out.append(f"entry = {toml_str(recipe['entry'])}")
    if recipe.get("version"):
        out.append(f"version = {toml_str(recipe['version'])}")
    if recipe.get("manager"):
        out.append(f"manager = {toml_str(recipe['manager'])}")
    if recipe.get("asset_kind"):
        out.append(f"asset_kind = {toml_str(recipe['asset_kind'])}")
    if recipe.get("asset"):
        out.append(f"asset = {toml_str(recipe['asset'])}")
    if recipe.get("origin"):
        out.append(f"origin = {toml_str(recipe['origin'])}")
    bins = recipe.get("bins") or []
    if bins:
        out.append("bins = [" + ", ".join(toml_str(b) for b in bins) + "]")
    for platform, (url, archive, sha) in (recipe.get("assets") or {}).items():
        out.append(f"[recipe.downloads.{platform}]")
        out.append(f"url = {toml_str(url)}")
        out.append(f"archive = {toml_str(archive)}")
        if sha:
            out.append(f"sha256 = {toml_str(sha)}")
    out.append("")


def omb_assets(corpus: Path, out: list[str]) -> tuple[int, set[str]]:
    root = corpus / "oh-my-bash"
    count = 0
    seen: set[str] = set()
    kinds = (
        ("themes", "theme", lambda d, n: d / f"{n}.theme.sh",
         lambda p, n: f"oh-my-bash theme {n}"),
        ("plugins", "plugin", lambda d, n: d / f"{n}.plugin.sh",
         lambda p, n: first_summary(p, ABOUT_PLUGIN) or f"oh-my-bash plugin {n}"),
        ("aliases", "alias", None, None),
        ("completions", "completion", None, None),
    )
    for subdir, kind, script_path, summarize in kinds:
        directory = root / subdir
        if not directory.is_dir():
            continue
        for entry in sorted(directory.iterdir()):
            name = None
            path = None
            if kind == "theme":
                if entry.is_dir() and (entry / f"{entry.name}.theme.sh").is_file():
                    name, path = entry.name, entry / f"{entry.name}.theme.sh"
            elif kind == "plugin":
                if entry.is_dir() and (entry / f"{entry.name}.plugin.sh").is_file():
                    name, path = entry.name, entry / f"{entry.name}.plugin.sh"
            elif kind == "alias":
                if entry.is_file():
                    stem = entry.name
                    for suffix in (".aliases.sh", ".aliases.bash"):
                        if stem.endswith(suffix):
                            name, path = stem[: -len(suffix)], entry
                            break
            elif kind == "completion":
                if entry.is_file():
                    stem = entry.name
                    for suffix in (".completion.sh", ".completion.bash"):
                        if stem.endswith(suffix):
                            name, path = stem[: -len(suffix)], entry
                            break
            if not name or not path:
                continue
            recipe_id = f"omb-{kind}-{name}"
            if recipe_id in seen:
                continue
            seen.add(recipe_id)
            summary = summarize(path, name) if summarize else f"oh-my-bash {kind} {name}"
            emit(out, {
                "id": recipe_id,
                "category": kind,
                "summary": summary,
                "license": "MIT",
                "url": f"{OMB_REPO}/tree/master/{subdir}/{name}",
                "manager": "oh-my-bash",
                "asset_kind": kind,
                "asset": name,
            })
            count += 1
    return count, seen


def bash_it_assets(corpus: Path, out: list[str]) -> tuple[int, set[str]]:
    root = corpus / "bash-it"
    count = 0
    seen: set[str] = set()
    # aliases / plugins / completion: available/<name>.<kind>.bash
    table = (
        ("aliases", "alias", ".aliases.bash", ABOUT_ALIAS),
        ("plugins", "plugin", ".plugin.bash", ABOUT_PLUGIN),
        ("completion", "completion", ".completion.bash", ABOUT_COMPLETION),
    )
    for subdir, kind, suffix, pattern in table:
        directory = root / subdir / "available"
        if not directory.is_dir():
            continue
        for entry in sorted(directory.iterdir()):
            if not entry.is_file():
                continue
            stem = entry.name
            if not stem.endswith(suffix):
                continue
            name = stem[: -len(suffix)]
            recipe_id = f"bashit-{kind}-{name}"
            if recipe_id in seen:
                continue
            seen.add(recipe_id)
            summary = first_summary(entry, pattern) or f"bash-it {kind} {name}"
            emit(out, {
                "id": recipe_id,
                "category": kind,
                "summary": summary,
                "license": "MIT",
                "url": f"{BASHIT_REPO}/tree/master/{subdir}/available/{entry.name}",
                "manager": "bash-it",
                "asset_kind": kind,
                "asset": name,
            })
            count += 1
    # themes: themes/<name>/<name>.theme.bash
    directory = root / "themes"
    if directory.is_dir():
        for entry in sorted(directory.iterdir()):
            if entry.is_dir() and (entry / f"{entry.name}.theme.bash").is_file():
                recipe_id = f"bashit-theme-{entry.name}"
                if recipe_id in seen:
                    continue
                seen.add(recipe_id)
                emit(out, {
                    "id": recipe_id,
                    "category": "theme",
                    "summary": f"bash-it theme {entry.name}",
                    "license": "MIT",
                    "url": f"{BASHIT_REPO}/tree/master/themes/{entry.name}",
                    "manager": "bash-it",
                    "asset_kind": "theme",
                    "asset": entry.name,
                })
                count += 1
    return count, seen


def standalone(out: list[str]) -> int:
    count = 0
    for recipe_id, category, summary, license_, url, extra in STANDALONE:
        recipe = {
            "id": recipe_id,
            "category": category,
            "summary": summary,
            "license": license_,
            "url": url,
        }
        if extra:
            for key in ("driver", "kind", "entry", "version", "bins", "assets"):
                if key in extra:
                    recipe[key] = extra[key]
            if extra.get("driver") == "git":
                recipe["origin"] = (url + ".git") if "github.com" in url else url
        emit(out, recipe)
        count += 1
    return count


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    header = [
        "# niubash plugin recipe seed index (generated — do not hand-edit bulk rows).",
        "# Regenerate: python scripts/gen-plugin-recipes.py --corpus <ecosys-repos> --out <this file>",
        "#",
        "# Data shape (mason-registry pattern; see docs/planning/lazy-family-source-study.md §6):",
        "#   id          unique recipe id (`niu plugin add <id>`)",
        "#   category    manager | theme | plugin | alias | completion | prompt",
        "#   driver      git (tree source) | download (direct binary) | absent (info-only)",
        "#   manager/asset_kind/asset  asset recipes riding on a manager source",
        "#   kind/entry  source adapter kind + entry file (generic sources)",
        "#   downloads.<platform>  per-platform download rows (url/archive/sha256)",
        "#",
        "# Manager rows for oh-my-bash / bash-it / bash-completion are derived from",
        "# the compiled-in adapters at runtime (single source of truth); this file",
        "# only carries their per-asset rows and standalone ecosystem entries.",
        "",
    ]

    out: list[str] = list(header)
    omb_count, _ = omb_assets(args.corpus, out)
    bashit_count, _ = bash_it_assets(args.corpus, out)
    standalone_count = standalone(out)

    text = "\n".join(out).rstrip() + "\n"
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(
        f"wrote {args.out}: {omb_count} oh-my-bash, {bashit_count} bash-it, "
        f"{standalone_count} standalone recipes"
    )


if __name__ == "__main__":
    main()
