# Download Surface Audit (wt49/smokesweep, 2026-10-02)

Owner ruling: "类似所有的问题都要处理" — every place niubash fetches bytes
over the network must be pure-Rust driven (no shelling out to `curl` /
`wget` / `Invoke-WebRequest`), except where an explicit ruling sanctions a
process boundary. This is the repo-wide audit behind that sweep.

## Call-point ledger

Every network-touching call point in `crates/`, `src/`, `scripts/`,
`installer/`, found by grepping for `curl|wget|Invoke-WebRequest`,
`Command::new`, and hardcoded `https://` constants:

| Call point | Mechanism | Ruling |
| --- | --- | --- |
| `crates/niubash-runtime/src/fonts.rs` | system `curl.exe` (System32) / `curl` | **The known case** — migration to the pure-Rust driver is owned by the wt49/wpmretract lane; this lane must not touch it. |
| `crates/niubash-runtime/src/plugins/download.rs` | `ureq` over rustls (`http_get_bytes`) | The sanctioned pure-Rust driver (owner ruling 2026-10-03). All executable-asset downloads route here. |
| `src/self_update.rs` | WinHTTP via `windows-sys` bindings | Pure-Rust driven (native API, no external process). Compliant. |
| `crates/niubash-runtime/src/plugins/sources.rs`, `plugins/distros.rs` | `git` external command | Sanctioned two-driver world (git tree sources vs direct binary downloads, see the `plugins::download` module doc). `git` is the VCS driver, not a download helper. |
| `crates/niubash-runtime/src/setup_wizard.rs` (`install_niu_git`) | `wpm install niugit` | Explicit user choice in the wizard delegating to the user's own package manager (owner directive, Windows-only); not a niubash fetch of bytes. |
| `crates/niubash-runtime/src/completion/command.rs`, `shell.rs` | `curl`/`wget` as completion word-list entries | Not download call points. |

**Conclusion: no residual external-tool download call points exist outside
`fonts.rs`.** Scripts (`scripts/*.py`, `scripts/*.ps1`) and the installer
contain no runtime download invocations (release packaging scripts fetch
nothing; `test_setup_wizard_pty.py` only spawns local binaries).

## Driver fix found by the audit's smoke run

The 1.3.0 smoke suite (`scripts/smoke-test-1.3.0.sh`, leg B3) found that the
download driver refused a verified Windows archive: recipes declare
extension-less bins (mason shape, `bins = ["fzf"]`), but Windows archives
ship `fzf.exe`, and `install_executable`'s verification only accepted the
exact declared name. Fixed in `plugins/download.rs`:

- `resolve_bin()` accepts `<bin>` or `<bin>.exe` on Windows, preferring the
  exact name;
- `ToolRecord.bins` records the resolved on-disk name (so `niu plugin tool
  list` shows reality);
- the staging directory is removed when unpack/bin verification fails (a
  failed install no longer leaves `.staging/<id>.unpacked` residue);
- `install_executable` is split into transport (`http_get_bytes`) +
  `commit_install(bytes)` so the staging discipline is unit-testable
  offline.

Known non-blocker noted during the fix: the PATH block prepends the tool
root directory; a recipe declaring a *nested* bin (`bin/tool.exe`) would
need the bin's parent on PATH instead. No current recipe declares nested
bins (`fzf`, `starship` are root-level), so this is latent, not shipped.
