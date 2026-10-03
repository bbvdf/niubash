# Niubash Plugin System — AI Quick Reference

Dense reference for agents operating niubash's plugin surface. Human
narrative: `docs/plugins-guide.md`. Design: `docs/planning/
oh-my-niu-ecosystem.md` §14.6. All verbs: `niu plugin --help`.

## Model

- **Spec = source of truth**: `~/.niubash/plugins.toml` declares what
  should be installed/enabled. Registry (`~/.niubash/sources/registry.toml`,
  schema `niubash:plugin-source-registry@0.3.0`) pins what exists
  (commit sha + tree sha256) and the last spec-materialized selection
  (`spec_enabled`, `spec_theme`).
- **CLI = sugar**: every mutating verb edits the spec then runs the
  reconciler (`plugins::sync`). Hand-editing the spec is equally valid.
- **Managers are data**: one descriptor table (`ManagerDescriptor` in
  `crates/niubash-runtime/src/plugins/descriptors.rs`) — id, origin,
  fingerprint, asset patterns, loader spec, selection spec. Adding a
  manager = one row, zero interpreter code. Current rows: `oh-my-bash`,
  `bash-it`, `bash-completion`, `bpkg`. Wild fallback adapter id `file`.
- **Id scope**: managers = one install per machine (source id = manager
  id); `file` sources and `bpkg` adoptions = one install per package
  (id = explicit `--id` or sanitized origin tail; bound into the spec at
  first sync).
- **Layout fingerprints** (detection, not filename blacklists): OMB =
  `oh-my-bash.sh`; bash-it = `bash_it.sh` + `lib/composure.bash`;
  bash-completion = `bash_completion`; bpkg = `bpkg.json`/`package.json`
  with `scripts` as an **array** (npm's object shape never matches).

## Verb table

| Verb | Effect | Exit |
|---|---|---|
| `add <id\|owner/repo\|url\|path> [--id N] [--ref R] [--checksum S] [--path D]` | Append spec entry + sync (install untrusted) | non-zero if the entry failed to install or is already declared |
| `list [--json]` | Sources, assets (`*` = enabled), tags for wild candidates | 0 |
| `enable <id\|asset\|id/asset>` | Spec edit + sync; wild/bpkg assets are tree-relative paths | non-zero when untrusted/degraded/ambiguous/whole-source misuse |
| `disable <target>` | Spec edit (entry removal for source targets) + sync; tree kept | 0 |
| `trust <id>` | Review + flip the trust gate (checksum tier) | 0 |
| `sync [--prune] [--bootstrap]` | Reconcile; `--prune` deletes undeclared, `--bootstrap` quiet startup form | 0 |
| `update [<id>]` | Move lockfile pin(s) to ref tip (no id = all) | 0 |
| `restore [<id>]` / `rollback <id>` / `clean` | Lockfile verbs | 0 |
| `discover` | Read-only overview | 0 |
| `source <sub>` | Full source protocol (add/trust/sign/verify/remove/update/rollback/list) | — |
| `mirror <sub>` | Transport mirroring: `list`, `set <url\|none>`, `test` | non-zero on garbage URL |

## Sync row actions

`awaiting-trust` (installed untrusted — gate is never automatic),
`activated`, `unchanged`, `deactivated` (spec no longer declares; block
dropped, tree kept), `degraded` (tree missing → `niu plugin restore`),
`drift`, `failed`, `removed` (--prune), `unsupported`.

## Invariants (pinned by tests/plugin_spec_sync.rs, tests/plugin_anyplug.rs)

1. **Idempotent materialization**: unchanged spec → byte-identical managed
   rc block, all rows `unchanged`, report clean, `--bootstrap` prints
   nothing.
2. **Hand-added survival**: `prev` records only the spec-owned selection;
   live-block entries outside `prev` survive every sync; spec-dropped
   entries (`prev − next`) are removed. Theme absent in spec = keep
   current (manual) theme; `theme = ''` clears it.
3. **CLI-spec consistency**: `add` + `enable` produce the same spec file
   and rc block as hand-writing the spec + sync (byte-for-byte).
4. **Cleanup honesty**: undeclared sources are suggested, never
   auto-deleted; `--prune` is the explicit confirm (removes tree +
   registry entry).
5. **Trust never automatic**: sync installs through the fetch gate only;
   activation requires `niu plugin trust <id>`.
6. **Honest enumeration**: wild sources list every `*.sh`/`*.bash`
   (≤256, depth-first) with tags (`script`, `fragment`,
   `installer/test-like — review before sourcing`, `bpkg script`); no
   guessed entry; repos with nothing sourceable fail at add.
7. **No shims (§14.4)**: enabling a file adds a guarded `. <path>` line
   identical to manual sourcing; framework plugins sourced bare fail with
   the same diagnostic and exit status as GNU bash (pinned against a real
   bash when present in tests/plugin_anyplug.rs).
8. **Engine zero plugin special-cases**: rubash source contains no plugin
   dispatch (comments citing ecosystem scripts as regression provenance
   only; audited read-only 2026-10-02).
9. **Mirrors are transport-only (§14.8)**: `~/.niubash/mirrors.toml`
   (`NIU_MIRRORS`) rewrites `https://github.com/` download requests
   (`[github] prefix`, `[github.releases] prefix` override) and injects
   `git -c url.<base>.insteadOf` for clone/fetch; spec/registry/tool
   records keep canonical URLs; missing/malformed/unknown config degrades
   to direct; no auto-select (doctor probes 3s and suggests only); zero
   bundled mirror list (community services are comments in the generated
   file). Pinned by plugins::mirrors unit tests, sources.rs
   `git_clone_args_carry_instead_of_mirror_for_github_origins_only`,
   tests/plugin_mirrors.rs.

## Spec schema (`niubash:plugin-spec@0.1.0`)

```toml
[[sources]]
target = "oh-my-bash"   # catalog id | owner/repo | url | path (reproducible origin)
id = "..."              # optional; wild/bpkg ids bind at first sync
kind = "bpkg"           # optional adapter pin; fingerprint must still match
ref = "v1.2"            # optional; first fetch only
enable = ["git"]        # manager-native asset names (wild: tree-relative paths)
theme = "agnoster"      # optional; absent = unmanaged, '' = explicitly cleared
```

Error/edge semantics: duplicate add → "already declared"; `--path` origin
is stored as the target verbatim (never re-resolved to a network URL);
unknown `enable` names are skipped and reported per row ("unknown spec
asset(s) skipped"); empty `enable` + no theme → managed block dropped,
tree kept.

## Env

`NIU_PLUGIN_SPEC` (spec path), `NIU_PLUGIN_SOURCES_ROOT` (install root),
`NIU_PLUGIN_BOOTSTRAP=off` (disable rc bootstrap line), `NIU_MIRRORS`
(mirror config path, default `~/.niubash/mirrors.toml`).
