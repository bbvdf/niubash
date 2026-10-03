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

One install driver exists, and only one: `git` (tree sources, via
`niu plugin add <git-url>`). Niubash carries zero network/HTTP download
responsibility (download retraction, owner ruling 2026-10-04): executable
tools (fzf, starship, …) install through your real package managers —
wpm first on Windows (owner correction 2026-10-03), winget/scoop as
alternatives for what wpm does not carry (GUI apps, fonts), and
apt/dnf/yum/brew on other platforms. The recipe rows for those tools stay
in the index as catalog metadata and `add` prints the commands.

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
`source rollback`) and the setup wizard journals one undo
command per thing it changed.

## Recipes: the index

```console
$ niu plugin recipe list                     # the whole index
$ niu plugin recipe list --category prompt   # manager|theme|plugin|alias|completion|prompt
$ niu plugin recipe list --json              # machine-readable (agents: prefer this)
$ niu plugin recipe show starship            # driver, version, license, state
$ niu plugin add starship                    # prints package-manager install commands
```

Rows with `driver = git` install as tree sources; executable-tool rows
(`driver = "download"` in the generated index) are catalog metadata —
`add` prints package-manager install commands (wpm first on Windows,
native managers elsewhere) and never fetches; rows with no driver are
info-only (e.g. ble.sh, which builds from source) and `add` explains
instead of installing.

## Collections: starting points

```console
$ niu plugin distro list            # built-in + imported
$ niu plugin distro apply recommended
$ niu plugin distro import <git-repo-or-path>   # any repo with niu-collection.toml
$ niu plugin distro remove <name>
```

Built-ins: `minimal` (bash-completion only), `recommended` (oh-my-bash + its
default theme + completions — also offered by the first-run wizard),
`full` (both frameworks + bash-preexec; its fzf/starship entries print
package-manager suggestions, since niu downloads nothing).

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

## Tools: package-manager recommendations

```console
$ niu plugin add fzf          # prints: wpm install fzf (Windows, first choice),
                              #        winget/scoop alternatives, apt/dnf/brew,
                              #        plus the upstream release URL
```

Nothing is fetched and nothing lands on disk — run the printed command with
your package manager. The retired `niu plugin tool` verbs fail with a
pointer here; a download-era install under `~/.niubash/tools/<id>` can be
removed by deleting the directory.

## The menu UI

`niu plugin ui` opens a menu-level UI (needs an interactive terminal):
sections ordered by what needs attention — untrusted sources first, then
ready sources, installable recipes, collections, global verbs. Every
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
- Mutations print their undo (`disable`, `source remove`,
  `source rollback <id>`). Surface those undo verbs to the user.
- `niu plugin update <id>` moves a source to its ref tip (no id = every
  source); `niu plugin sync` reconciles the spec with reality;
  `niu plugin restore` rebuilds from the lockfile pin in
  `~/.niubash/sources/registry.toml`.
- Exit codes: success `0`; usage errors and refused operations (unknown id,
  builtin-name collision, non-tty `ui`) exit non-zero with a message that
  names the object and the repair verb.
- Env overrides used by tests/tools: `NIU_PLUGIN_SOURCES_ROOT`,
  `NIU_PLUGIN_DISTROS_ROOT`.
