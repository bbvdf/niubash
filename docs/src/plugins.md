# Plugins & the Bash Ecosystem

Niubash does not vendor a plugin framework. The plugin system is the **external
bash ecosystem as first-class content**: oh-my-bash, bash-it and
bash-completion install over `git clone`, earn activation through an explicit
trust review, and expose their real themes/plugins/aliases/completions through
their own selection mechanisms. Nothing is sourced until you say so.

This page is the user guide; the last section is a compact quick reference for
AI agents driving niu on a user's behalf.

## The five nouns

| Noun | What it is | Where it lives |
|---|---|---|
| **Source** | A git tree installed by niu (a manager like oh-my-bash, or any single-purpose repo) | `~/.niubash/sources/<id>/` |
| **Asset** | A theme/plugin/alias/completion *inside* a manager source | enabled via the manager's own mechanism |
| **Recipe** | A data row describing one ecosystem asset and how to install it (~500-row compiled-in index) | `niu plugin recipe list` |
| **Collection** | A named manifest of recipe ids — a starting-point bundle (LazyVim-extras pattern) | built-in, or `~/.niubash/distros/` |
| **Tool** | A direct binary download (pure-Rust HTTP + unpack, sha256-pinned) | `~/.niubash/tools/<id>/` |

Two install drivers exist, and only two: `git` (tree sources) and `download`
(executables). Niubash never drives apt/wpm/Chocolatey to install plugin
content — the download driver is implemented in niubash itself, cross-platform.

## Everyday flow

```console
$ niu plugin discover              # read-only: what's out there
$ niu plugin add oh-my-bash        # git-clones the manager (UNTRUSTED)
$ niu plugin trust oh-my-bash      # review checksum, flip the execution gate
$ niu plugin list                  # sources, assets, activation state
$ niu plugin enable agnoster      # activate a theme through oh-my-bash
```

The trust gate is the load-bearing rule: **install and activation are separate
verbs**. `add`/`apply` land content on disk but nothing from it executes in
your shell until `niu plugin trust <id>` succeeds (checksum review) or you
`enable` an asset explicitly. Collection applies honor the same rule.

Manager sources (catalog ids like `oh-my-bash`) are also declared in the
spec `~/.niubash/plugins.toml` — the single source of truth for what is
declared; `niu plugin sync` reconciles the spec with what is installed,
and `niu plugin update <id>` (no id = all) moves sources to their ref tips.

Undo is printed with every mutating command (`disable`, `source remove`,
`tool remove`, `source rollback`) and the setup wizard journals one undo
command per thing it changed.

## Recipes: the index

```console
$ niu plugin recipe list                     # the whole index
$ niu plugin recipe list --category prompt   # manager|theme|plugin|alias|completion|prompt
$ niu plugin recipe list --json              # machine-readable (agents: prefer this)
$ niu plugin recipe show starship            # driver, version, license, state
$ niu plugin add starship                    # download driver, sha256-verified
```

Rows with `driver = git` install as tree sources; `driver = "download"` rows
fetch a pinned release artifact (every compiled-in download row pins a sha256
— a test enforces it); rows with no driver are info-only (e.g. ble.sh, which
builds from source) and `add` explains instead of installing.

## Collections: starting points

```console
$ niu plugin distro list            # built-in + imported
$ niu plugin distro apply recommended
$ niu plugin distro import <git-repo-or-path>   # any repo with niu-collection.toml
$ niu plugin distro remove <name>
```

Built-ins: `minimal` (bash-completion only), `recommended` (oh-my-bash + its
default theme + completions — also offered by the first-run wizard),
`full` (both frameworks + bash-preexec + fzf + starship).

A collection is a small TOML manifest of recipe ids:

```toml
schema = "niubash:plugin-collection@1"
name = "my-starter"
description = "what this bundle is"

[[entry]]
recipe = "oh-my-bash"
[[entry]]
recipe = "starship"
```

Imports are first-class: anything the built-ins can do, your own repo can do,
byte-identically. `apply` installs every entry through its recipe driver and
**never grants trust** — you review afterwards. Failures are collected per
entry (a bad entry never kills the rest) and reported with the retry verb.
The first-run wizard's plugin-collection question is a LazyVim-style
progressive-disclosure hook: it only appears when the ecosystem is empty, and
Skip is the default.

## Tools: downloaded binaries

```console
$ niu plugin add fzf          # release artifact, sha256 pinned in the recipe
$ niu plugin enable fzf       # managed PATH block in ~/.niubashrc
$ niu plugin tool list
$ niu plugin tool remove fzf  # PATH block + directory + record
```

## The menu UI

`niu plugin ui` opens a menu-level UI (needs an interactive terminal):
sections ordered by what needs attention — untrusted sources first, then
ready sources, tools, installable recipes, collections, global verbs. Every
action in the menu is the same verb the CLI runs (the footer shows the CLI
equivalent of each); there is no UI-only logic. Rows read
`[state] id — hint`.

## For AI agents (quick reference)

When driving niu on a user's machine:

- Prefer `niu plugin recipe list --json` and `niu plugin list [--json]` for
  state; plain-text output is for humans and changes freely.
- **Never auto-trust.** `niu plugin add ...` / `distro apply ...` are safe
  (content lands but does not execute). `niu plugin trust <id>` flips the
  execution gate — that is a human review step. Report untrusted ids to the
  user with the exact command they should run.
- Trust requires an intact tree; a degraded source (missing directory) cannot
  be trusted — suggest `niu plugin restore <id>`.
- Mutations print their undo (`disable`, `source remove`, `tool remove`,
  `source rollback <id>`). Surface those undo verbs to the user.
- `niu plugin update <id>` moves a source to its ref tip (no id = every
  source); `niu plugin sync` reconciles the spec with reality;
  `niu plugin restore` rebuilds from the lockfile pin in
  `~/.niubash/sources/registry.toml`.
- Exit codes: success `0`; usage errors and refused operations (unknown id,
  builtin-name collision, non-tty `ui`) exit non-zero with a message that
  names the object and the repair verb.
- Env overrides used by tests/tools: `NIU_PLUGIN_SOURCES_ROOT`,
  `NIU_PLUGIN_TOOLS_ROOT`, `NIU_PLUGIN_DISTROS_ROOT`.
