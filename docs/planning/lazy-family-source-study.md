# Lazy family source study (lazy.nvim / LazyVim / mason) — wt47/lazystudy

Phase 1 deliverable of lane wt47/lazystudy (2026-10-03). Sources were shallow-cloned
and read directly:

- `target/refs/lazy.nvim` — folke/lazy.nvim @ 306a055 (plugin manager)
- `target/refs/LazyVim` — LazyVim/LazyVim (distribution layer)
- `target/refs/mason.nvim` — mason-org/mason.nvim v2.x (installer engine)
- `target/refs/mason-registry` — mason-org/mason-registry (recipe data)

Line numbers are from those checkouts. Verdict per mechanism:
**[portable]** we can adopt the shape · **[nvim-only]** tied to the Neovim runtime ·
**[have-it]** niu already has the equivalent (cite ours).

---

## 1. lazy.nvim — spec parsing & merging

The spec system is no longer a `spec.lua`; it is split into
`Spec` (parse driver), `Fragments` (spec units), and `Meta` (plugin table).

- **Normalize dispatch** — `lua/lazy/core/plugin.lua:93 Spec:normalize()`:
  a spec is (a) a string (`"owner/repo"` shorthand → one-fragment plugin),
  (b) a list (recurse), (c) a table with `[1]`/`dir`/`url` (a plugin spec, may
  carry `import`), or (d) an import-only table. Errors are *collected* into
  `self.notifs` (`plugin.lua:76 Spec:log`), not thrown — a bad extra never
  kills startup.
- **Shorthand resolution** — `lua/lazy/core/fragments.lua:108-121`: `pkg[1]`
  containing `/` becomes name = suffix and url =
  `Config.options.git.url_format:format(...)` (`core/config.lua:32` default
  `https://github.com/%s.git`); `http`/`git@` prefixes are treated as full
  URLs. Name derivation from URL: `plugin.lua:59 Spec.get_name()` (strip
  `.git`, take last path segment).
- **Import (module walking)** — `plugin.lua:118 Spec:import()`: guard against
  duplicate module names (`self.modules`), honor `cond`/`enabled` gates
  (`:136-141`), then `Util.lsmod` lists every file under the module path,
  **sorted by module name** (`:170-172`), each loaded and re-normalized
  recursively. A module that yields nothing is an error (`:209`).
- **Fragment chaining = merge order** — `fragments.lua:85 M:add()`: each spec
  table becomes a fragment with parent (`pid`) and dependency links; deps are
  normalized with a dep-stack so `fragment.dep = true` marks transitive
  fragments (`:150-156`). `lua/lazy/core/meta.lua:166 M:_rebuild()` then chains
  a plugin's fragments through `setmetatable(spec, {__index = super})` —
  **later fragments shadow earlier ones**, which is how "a distro spec + a user
  spec for the same plugin" merge. `optional` is AND-ed across fragments
  (`meta.lua:197`), `url` first-wins (`meta.lua:198`).
- **Opts/value merging** — `plugin.lua:429 M.values()` / `:447 M._values()`:
  walks the same super chain, deep-merges tables, extends lists, and supports
  `<prop>_extend` path keys for list-append semantics.
- **Resolution passes** — `meta.lua:346 M:resolve()`: rebuild → `fix_pkgs` →
  `fix_cond` → loop `fix_disabled + fix_optional` until stable. Plugins whose
  every fragment is `optional` drop out unless explicitly requested.

**Verdict.** Shorthand + collected-errors + import-list **[have-it]** (our
`catalog.rs` shorthand and `normalize_origin`, `sources.rs:673`); fragment
metatable merge and `opts` merging **[nvim-only]** — our specs are static data
(TOML), there are no Lua functions to merge. The *shape* worth keeping is:
specs are lists of data units, later units win, and resolution is a fixpoint.

## 2. lazy.nvim — lockfile (lazy-lock.json)

- `lua/lazy/manage/lock.lua:11 M.update()`: writes a JSON object
  `{ "<plugin>": { "branch": …, "commit": … } }`, keys **sorted**, entries kept
  for disabled/cond-false plugins (`:18-22`), one entry per *installed,
  non-local* plugin (`:24-32`). `M.get()` (`:69`) is what `restore` reads:
  update-checkers compare `lock.commit` vs the tracking branch tip.
- Load is plain `vim.json.decode` with `pcall` (`:49-65`) — a corrupt
  lockfile degrades to "no lock", never crashes.
- Lockfile path default `core/config.lua:24`: `stdpath(config)/lazy-lock.json`
  — i.e. *the lockfile lives with the user's config, not the manager's data*,
  so it can be committed to dotfiles.

**Verdict [have-it].** Our `~/.niubash/sources/registry.toml`
(`sources.rs:32 SOURCE_REGISTRY_SCHEMA @0.2.0`) already pins
`commit_sha` + `checksum_sha256` per source and `niu plugin restore` refetches
exactly that state — same role as lazy-lock.json. Deltas worth adopting:
sorted keys (we have them via TOML table) and "lock survives disabled state"
(ours records regardless — fine). Mason-style version fields: see §6.

## 3. lazy.nvim — bootstrap chain

- `lua/lazy/init.lua:123 M.bootstrap()`: if `stdpath("data")/lazy/lazy.nvim`
  is absent, `git clone --filter=blob:none --branch=stable` from GitHub, then
  `rtp:prepend`. The *stable branch* is the pin (not a commit), and blob
  filtering keeps first fetch cheap.
- lazy.nvim adds itself to the spec (`core/plugin.lua:333
  specs[#specs+1] = { "folke/lazy.nvim" }`) so the manager is managed by
  itself; its own `config` is neutered (`plugin.lua:338-346`).

**Verdict [have-it]** — niu is a shipped binary, not a git clone; the
equivalent of "bootstrap" is the installer/self-update path (`src/self_update.rs`).

## 4. lazy.nvim — the view (TUI information architecture)

Rendering is vim-buffer-based (float + `vim.diagnostic` virtual text) — the
*rendering* is nvim-only; the **information architecture** is what we port:

- **Command table, not key handling** — `lua/lazy/view/config.lua:42
  M.commands`: every verb is a data row with `id` (sort), `button` (shows as a
  top pill), `key` (global), `key_plugin` (acts on the cursor plugin),
  `desc`/`desc_plugin` (help text), `toggle`. The help page and the pill bar
  are *generated from the same table* (`view/render.lua:134 M:title`,
  `:185 M:help`). One table = UI, help, and keymaps can never drift.
- **Sections by lifecycle state** — `lua/lazy/view/sections.lua:16-120`: an
  ordered filter list — Failed, Working, Build, Breaking Changes, Updated,
  Installed, Updates, Log, Clean, Not Installed, Outdated, Loaded, Not Loaded,
  Disabled. `view/render.lua:265 M:section()` consumes plugins out of the pool
  in section order (a plugin appears in exactly one section), header shows the
  count.
- **Row anatomy** — `view/render.lua:444 M:plugin()`: `[loaded?] icon` +
  name + *reasons* (why this plugin is here: which handler/dependency pulled
  it, `render.lua:306 M:reason`) + a right-aligned diagnostic
  (`render.lua:381 M:diagnostics`: running task → "task: status", failed →
  error, `needs build`, `updated from x to y`, `version v… available`). Enter
  expands details + task log inline (`render.lua:496 M:tasks`).
- **Progress** — `render.lua:248 M:progressbar()` renders only while
  `done < total` (a `─` line split done/todo), and the title area swaps
  "Total: N plugins" for "Tasks: done/total" (`render.lua:168-182`).
- **Per-plugin vs global verbs** — `view/init.lua:324 M:setup_modes()` maps
  both `key` and `key_plugin` from the command table; visual mode collects a
  range of plugins and runs the verb over all of them. Hover/diff/restore act
  contextually on patterns under the cursor (`init.lua:201 M:setup_patterns`).
- **Async tasks** — every mutation runs as a task (`manage/task/*`),
  the view re-renders on `LazyRender` autocommands; nothing blocks.

**Verdict [portable] as information architecture only.** For `niu plugin ui`
we adopt: the command-table pattern, sections-by-state, row = state-icon +
name + status + hint, per-item vs global verbs — rendered with our
`interactive_menu` (menu level, no float/ncurses). Async task streaming stays
out of the MVP: our verbs are synchronous git/wpm calls.

## 5. lazy.nvim — pkg / rock support

- `lua/lazy/pkg/init.lua`: pkg sources `lazy`/`packspec`/`rockspec` parse
  *package manager manifests found inside plugin trees* into extra spec
  fragments (tagged `optional`, `meta.lua:32 M:load_pkgs()`), cached in a
  versioned cache file (`pkg/init.lua:9 M.VERSION = 12`).
- `lua/lazy/pkg/rockspec.lua`: rocks support shells out to **hererocks**
  (a python script that vendors Lua+luarocks, `rockspec.lua:37
  M.hererocks.build`), with hardcoded rewrites (`plenary.nvim`).

**Verdict [nvim-only].** This whole layer exists to bridge Lua package
ecosystems into the nvim runtime. For niu, the analogous bridge is wpm
(see §7) and it is a *driver*, not a manifest parser.

## 6. mason — registry data form (the recipe pattern)

- **One data file per package** — `packages/<name>/package.yaml` (600 files).
  Anatomy (`packages/stylua/package.yaml`):
  - metadata: `name`, `description`, `homepage`, `licenses`, `languages`,
    `categories`;
  - `source.id` — a **purl**: `pkg:github/johnnymorganz/stylua@v2.5.2`
    (scheme = which installer compiles it, `@version` = pin);
  - `asset` — per-target rows (`darwin_arm64`…`win_x64`) with `file` (release
    asset name) and `bin` (executable inside the archive);
  - `version_overrides` — older-version recipes selected by
    `constraint: semver:<=v0.20.0`;
  - `bin:` — output name → source (`stylua: "{{source.asset.bin}}"`,
    interpolated; manager-prefixed forms like `luacheck: luarocks:luacheck`,
    `typescript-language-server: npm:typescript-language-server`).
- **Package type census** (grep over `id: pkg:`): github 288, npm 133, pypi
  101, golang 33, cargo 29, gem 14, generic 12, openvsx 11, nuget 6, composer
  6, luarocks 5, opam 4. Each scheme maps to one compiler:
  `lua/mason-core/installer/compiler/compilers/{github,npm,pypi,cargo,gem,
  golang,composer,nuget,opam,openvsx,luarocks,generic}/*.lua`.
- **Compiler anatomy** — `compilers/github/release.lua:20 M.parse()`:
  interpolate `{{version}}` into the asset table → pick the row for the
  current platform (`util.coalesce_by_target`) → build download URLs from
  `settings.github.download_url_template`; `M.install()` just downloads;
  `M.get_versions()` asks a live provider. `compilers/generic/download.lua:17`
  is the no-toolchain escape hatch: a `files: {out: url}` table.
- **Version resolution** — `lua/mason-core/installer/compiler/init.lua:80-98`:
  walk `version_overrides`, match `semver:<=/>=/=` against the *requested*
  version, first match wins; providers
  (`lua/mason-core/providers/{registry-api,client/{gh,npm,pypi,golang,
  rubygems,openvsx}}`) answer "what versions exist".
- **Layout & PATH injection** — `lua/mason-core/installer/InstallLocation.lua:
  44-77`: `<root>/{bin,share,opt,packages,system_packages,staging}`; the
  linker (`installer/linker.lua:11-24`) puts executable shims into
  `location:bin(path)` and records links in the receipt (Windows 1.0-schema
  receipts got `.cmd` shims, `linker.lua:83-85`). PATH policy is a setting —
  `lua/mason/settings.lua:12-17` `PATH = "prepend"|"append"|"skip"`, applied
  via `InstallLocation:set_env { PATH = … }` (`lua/mason/init.lua:16-17`).

**Verdict [portable — this is the core pattern for our recipe registry].**
Key transferable ideas: (1) the registry is *pure data*, one file per package;
(2) the `source` field picks the installer *driver* and pins a version;
(3) `bin` maps exposed names. For niu the driver set collapses to two:
`git-clone source adapter` (what `sources.rs` does today) and **wpm** for
executables — because wpm already owns per-platform artifact download with
sha256 (see §7). We deliberately do **not** port npm/pypi/cargo compilers:
that is wpm's job in this ecosystem.

## 7. wpm as the executable driver (our environment)

Checked against the local WinuxCmd checkout (wpm = `winuxcmd wpm`,
`D:/repo/unixwin-winuxcmd/src/commands/wpm.cpp`):

- Subcommands: `wpm install|uninstall|list|search|info`, `wpm links
  rebuild|list|remove`, `wpm sources add|remove|list` (`wpm.cpp:4109-4165`).
- Package manifests (`D:/repo/wpm-source/index.json`, 281 packages) carry
  per-platform artifacts: `artifacts."windows-x64" = { type: zip, sha256,
  urls[], files[{from,to}], layout }` plus exposed `commands[]` — i.e. wpm
  already implements mason's `generic`/`github` download compilers with
  integrity and layout handling.
- niu already invokes wpm as a driver elsewhere:
  `crates/niubash-runtime/src/winuxcmd.rs:386` (`wpm links rebuild --root …`)
  and the setup wizard's `setup_wizard.rs:1227 wpm_command()` — "`wpm` on PATH,
  else `winuxcmd.exe wpm`". The recipe registry should reuse exactly that
  resolution, not reinvent it.

## 8. LazyVim — extras / distribution layer

- **Base set as importable module** — `lua/lazyvim/plugins/init.lua` returns
  the core plugin specs (lazy.nvim itself pinned `version = "*"` at `:15`,
  LazyVim self-import `priority = 10000, version = "*"` at `:16`). The starter
  imports it with `{ import = "lazyvim.plugins" }`, then extras, then the
  user's `plugins` dir.
- **Extras = spec modules + metadata** — `lua/lazyvim/plugins/extras/**`
  grouped by category dirs (ai/coding/dap/editor/formatting/lang/linting/lsp/
  test/ui/util). Each extra file is a plain spec list that may set `desc` and
  `recommended` (bool / fn / `{ft=…, root=…}` wants-check,
  `lua/lazyvim/util/extras.lua:43 M.wants`).
- **Extra discovery** — `util/extras.lua:30 M.sources`: two module trees —
  `lazyvim.plugins.extras` (distro) and `plugins.extras` (**user extras**, same
  shape, zero code difference). `M.get()` (`:60`) walks the trees;
  `M.get_extra()` (`:85`) dry-parses the module with `optional = true` to
  split its plugins into *required* vs *optional*, extracts `recommended`,
  and derives `managed` (toggleable here vs pinned by user config).
- **Enable/disable persistence** — the enabled-extras list lives in
  `lazyvim.json` (`lua/lazyvim/config/init.lua:143` path, `:148 extras = {}`);
  `X:toggle` (`util/extras.lua:167`) flips the entry, saves via
  `util/json.lua:47 M.save` (deterministic encode, sorted keys, version
  stamped), and tells the user to restart. `M.migrate` (`json.lua:57+`) is a
  versioned rename chain — schema evolution without breakage.
- **One-of defaults** — `config/init.lua:357 M.register_defaults` /
  `:420 M.get_defaults`: groups like picker ∈ {snacks, fzf, telescope} pick
  exactly one extra (user override via `vim.g.lazyvim_picker`, else first
  already-imported, else default), gated by `install_version` so existing
  installs keep their old default (`:444`).
- **Import order discipline** — `config/init.lua:229-241`: warns when the
  lazy.nvim import order isn't `lazyvim.plugins` → extras → user `plugins`.
- **Version pinning strategy** — distro pins itself and lazy.nvim with
  `version = "*"` (semver tags, resolved+locked by lazy-lock.json at the
  user's machine); releases via release-please tags. Extras are *not*
  individually pinned — they ship as part of the distro version.

**Verdict [portable — this is the pattern for `niu plugin distro`]**: a
collection is a *data manifest* (list of recipe ids + optional preset picks);
built-in collections live in the product, user collections are the same format
imported from a repo/path; enable state persists in niu's own state file with
a schema version; "apply" installs untrusted and defers activation to the
existing trust protocol. LazyVim's one-of defaults map to our wizard's
preset choice.

## 9. Mapping to the four wt47 candidates

| Candidate | Source pattern | niu owner module |
| --- | --- | --- |
| 1. Recipe registry | mason `package.yaml` (§6) + independent pure-Rust download driver (§10.4) | new `plugins/recipes.rs` + `plugins/download.rs`; data in `assets/plugins/` |
| 2. Distro/collections | LazyVim extras + lazyvim.json (§8) | new `plugins/distros.rs` + wizard hook |
| 3. TUI MVP | lazy view IA (§4): command table, state sections, row anatomy | new `plugin_ui.rs` over `interactive_menu` |
| 4. Binary direct download / rocks | mason download compilers (§6), lazy rockspec (§5) | download **implemented independently** (owner ruling §10); rocks not implemented (nvim-only) |

### Explicitly rejected / deferred (with reasons)

- ~~**Binary direct download in niu** — rejected: wpm's index already carries
  per-platform artifact tables with sha256 (§7); duplicating mason's download
  compilers in niu would fork integrity handling. Recipes reference
  `driver = "wpm"`.~~ **SUPERSEDED by owner ruling 2026-10-03** (see §10):
  executable installs are implemented *inside* niubash-runtime with pure Rust
  HTTP + unpack (ureq/flate2/tar), cross-platform, binding to no external
  package manager. §7 stays as environment background only.
- **npm/pypi/cargo/gem/… drivers** — rejected: language-package-manager
  installers are out of scope for niu's plugin system; the independent driver
  covers git trees and direct binary downloads only (the two forms bash
  ecosystem assets actually ship).
- **Fragment metatable merge, `opts` merging, handlers (event/ft/cmd/keys
  lazy-loading)** — nvim-only: a bash rc has no deferred-loading event system;
  our sources load at rc time through the manager's own loader.
- **Async task streaming UI** — deferred: our verbs are short synchronous
  git/wpm invocations; the MVP UI runs them and re-renders.
- **rockspec/hererocks** — nvim-only by definition.

---

## 10. Addendum (owner corrections 2026-10-03) — design philosophy & UX

Owner corrections for this lane: (1) executable installs are implemented
independently inside niubash-runtime (pure Rust HTTP + unpack, cross-platform)
— *never* by driving wpm/apt; (2) the recipe seed must scale to the whole
popular bash ecosystem; (3) this philosophy/UX section; (4) docs and `--help`
ship with the implementation. What the sources say about *why* the lazy family
feels good, with citations:

### 10.1 lazy.nvim — "everything is a spec; the manager is a userland program"

- **Convention over configuration.** A bare string `"folke/lazy.nvim"` is a
  complete spec (`fragments.lua:108-121`): the URL format and the name
  derivation are conventions (`core/config.lua:32`), so the 95% case is one
  line. Nothing needs to be declared that can be derived from the shorthand.
- **Declarative data, imperative islands.** Specs are data tables; the only
  code a user writes is small hooks (`config`, `build`, `init`). Resolution is
  a *fixpoint over data* (`meta.lua:346 M:resolve`), not an imperative install
  script — the same reason mason recipes are YAML.
- **Errors never break the world.** Spec errors are collected
  (`plugin.lua:76 Spec:log`) and reported after loading (`:80 Spec:report`);
  a corrupt lockfile degrades to "no lock" (`manage/lock.lua:55 pcall`); a
  missing dependency in health output is a *warning with the found version vs
  needed version* (`health.lua:47 "`%s` version `%s` needed, but found `%s`"),
  and health *detects a foreign plugin manager on the rtp and names it*
  (`health.lua:96`). Lesson for niu: every failure mode should name the exact
  object and the exact repair verb.
- **The manager manages itself.** lazy.nvim inserts itself into its own spec
  (`core/plugin.lua:333`), so update/lockfile/restore treat the manager as
  just another plugin. Bootstrap is two lines in the user's config
  (`init.lua:123 M.bootstrap`) — progressive disclosure: day-1 user pastes a
  snippet, day-30 user never sees it again.

### 10.2 LazyVim — "a distro is just an opinionated spec import + one toggle file"

- **A distribution as data.** LazyVim is a *spec module* the user imports
  before their own specs (`plugins/init.lua`); extras are the same shape, one
  category directory deep (`plugins/extras/**`). No code distinguishes
  "distro" from "user" — the *user extras source is the identical module
  shape* (`util/extras.lua:30-33 M.sources`: `lazyvim.plugins.extras` vs
  `plugins.extras`). First-class means: anything the distro can do, a user
  repo can do, byte-identically.
- **Progressive disclosure as the core UX.** Day 1: paste starter → works.
  Day 2: `:LazyExtras` → toggle a star-marked extra with `x`
  (`util/extras.lua:152`), restart. The toggle file (`lazyvim.json`) is tiny,
  sorted, versioned, and migrates forward (`util/json.lua:47-57`) — the user
  never edits it by hand and never breaks it.
- **One-of defaults instead of hard deps.** Competing extras (picker ∈
  {snacks, fzf, telescope}) are a *group with exactly one winner*
  (`config/init.lua:357 M.register_defaults`): user override > already
  imported > default, and existing installs keep their old choice via
  `install_version` (`:444`). Lesson: when a collection must choose,
  make the choice visible, overridable, and sticky — never a silent flip.
- **Onboarding is a notice, not a wizard.** First run shows "Welcome to
  LazyVim!" and opens NEWS.md only when it changed (`util/news.lua:24
  M.welcome`, `:16 M.setup` gating on the stored hash). Import-order mistakes
  produce a *warning listing the correct order* (`config/init.lua:229-241`),
  not a crash. Lesson for niu: hint at the moment of need, with the fix in
  the message.

### 10.3 mason — "the registry is the product; the installer is a table walk"

- **One data file per package** (§6) means the registry is reviewable in PRs,
  greppable, andCI-refreshable — mason-registry is *literally a repo of YAML*,
  updated by bots. Adding a package is a data contribution, not a code
  change. This is the single most portable idea in the family.
- **The driver set is closed and small.** 600 packages collapse to 12 source
  schemes, and the census is heavily skewed (github 288 / npm 133 / pypi 101
  = 87%). niu's driver set is smaller still: `git` (tree sources, existing
  adapters) and `download` (direct binary, §10.4). A small closed set is what
  makes "no behavior code per recipe" sustainable.
- **Failure UX.** Every async outcome becomes one notify line naming the
  package (`ui/instance.lua:615 "%s was successfully installed."`,
  `:638 "%s failed to install."`); cancellation is explicit
  (`:384 "Cancelling installation of %q."`). Missing tooling (npm, python) is
  checked *before* install starts by health/providers, not discovered
  mid-unpack.
- **PATH as policy, not accident.** `PATH = "prepend"|"append"|"skip"` is a
  *setting* (`lua/mason/settings.lua:12-17`) and shims land in one predictable bin dir
  (`InstallLocation.lua:44`). Lesson for niu: executable installs must state
  where they put binaries and how PATH changes, as data.

### 10.4 CLI verbs vs TUI — what the family teaches

- **The TUI is a view over verbs, not a separate world.** lazy.nvim's view
  keys call the *same* command table the CLI/API uses (`view/config.lua:42`,
  dispatched via `view/commands.lua cmd()`); Mason's UI buttons call the same
  installer the `:MasonInstall` command uses. Zero UI-only logic. Our
  `niu plugin ui` must only call `plugins::sources/assets/recipes` functions
  that the CLI verbs already call.
- **Per-item verbs + global verbs.** lazy's key split (`key` global, e.g.
  `S`=sync; `key_plugin` on cursor, e.g. `u`=update this one) matches two CLI
  shapes: `niu plugin sync` vs `niu plugin update <id>`. Keep both verb forms
  in the menu.
- **State-first listing.** Sections order plugins by *what needs attention*
  (`view/sections.lua`: Failed → Working → … → Disabled), not alphabetically;
  the row shows the state icon + name + reason + right-column diagnostic
  (`render.lua:444`). Our menu rows adopt: `[state] id — hint`.
- **Progress only while in flight** (`render.lua:168-182`): no fake bars.

### 10.5 What this dictates for niu (decisions carried into implementation)

1. Recipes are pure data (TOML), one registry file, generator-composable,
   no behavior code per recipe (§6/§10.3).
2. Driver set: `git` + `download` only; `download` is pure Rust in
   niubash-runtime (ureq + flate2/tar + zip + sha256), cross-platform,
   calling no external package manager (owner ruling; §7 demoted to
   background).
3. Executable installs state their bin dir and PATH plan as data, mason
   `bin:`-style.
4. `niu plugin ui` = thin view over the same functions the CLI verbs call,
   state-first rows, per-item + global verbs (§10.4).
5. Collections are the same manifest shape built-in or imported from a repo
   (LazyVim §10.2 first-class rule); apply never auto-trusts (owner ruling
   2026-10-02: trust stays an explicit review step).
6. Errors name the object and the repair verb, health-style (§10.1).


---

## 11. Implementation log (2026-10-02, lane continuation)

What the lane shipped against §10.5's decisions, with the owning modules:

1. **Recipe index (§6/§10.3)** — `assets/plugins/recipes.toml`, 498 rows:
   theme 166 · completion 149 · plugin 118 · alias 59 · prompt 5 ·
   manager 2 (+3 adapter-derived at runtime). The five named classes are
   all non-empty (`every_category_has_rows`). New standalone rows:
   bash-git-prompt (git driver, entry `gitprompt.sh`, BSD-2-Clause,
   6.9k★), sexy-bash-prompt (info-only — `make install` model), basher
   (info-only — PATH+eval model). Data quality: every download row pins
   sha256 (`download_recipes_pin_sha256_per_platform`); starship's three
   platform digests were backfilled from the GitHub release-asset `digest`
   field and fzf's pins re-verified byte-for-byte against the same API.
   Regeneration from the corpus is byte-stable outside the new rows.
2. **Collections (§8/§10.2)** — `plugins/distros.rs` + built-in manifests
   `assets/plugins/collections/{minimal,recommended,full}.toml`; import
   (git clone or local dir) with name-collision policy: builtin-name
   collision refused, different-origin-under-same-name refused with the
   repair verb, same-origin re-import = refresh. `apply` collects
   per-entry failures (Spec:log pattern) instead of aborting.
   CLI: `niu plugin distro list|import|remove|apply`.
3. **Wizard hook (§10.2 progressive disclosure)** — Q2.5 asks for a
   collection only when the ecosystem is empty, Skip default, applied
   after the explicit Apply gate; the setup journal records what landed
   and the finish screen prints one undo command per source/tool
   (`niu plugin source remove <id>` / `niu plugin tool remove <id>`).
4. **Tool channel (§10.3 PATH-as-policy)** — `download::remove_tool`
   (PATH block + dir + record) and `niu plugin tool list|remove`; this is
   the undo verb for download recipes and collection applies.
5. **Menu UI (§4/§10.4)** — `plugins/ui.rs`: `niu plugin ui`, menu-level
   over `interactive_menu`. Command table `UI_VERBS` generates both the
   verb menus and the help footer (one table = UI and help cannot drift);
   sections ordered attention-first (untrusted → ready → tools →
   recipes → collections → global verbs); rows `[state] id — hint`;
   `apply_verb` is the single dispatch and calls exactly the runtime
   functions the CLI verbs call. Non-tty invocation is refused up front
   with the CLI alternatives named. (Interactive key-driving could not be
   verified in this dev environment: the repo's own
   `scripts/test_setup_wizard_pty.py` fails the same way here — keys
   don't reach the app through the harness; rendering, counts, and the
   non-tty guard were verified.)
6. **Docs** — `docs/src/plugins.md` (user guide + AI quick reference,
   SUMMARY updated); `niu plugin --help` now lists recipe/distro/tool/ui.

Citation audit 2026-10-02: 14 file:line references spot-checked against the
`target/refs/` trees; four were off and corrected (LazyVim
`register_defaults` :349→:357, `get_defaults` :411→:420, install_version
gate :411-415→:444, `extras.lua M.sources` :28→:30, mason settings path
`mason-core/`→`mason/`).
