//! External plugin-manager sources (oh-my-bash, bash-it, bash-completion)
//! as first-class plugin origins.
//!
//! Design: `docs/planning/oh-my-niu-ecosystem.md` §11 (source adapters) and
//! §12 (trust + download protocol), plus the owner calibrations of
//! 2026-10-02: no vendoring (fetch-on-demand over git clone, license stays
//! between user and upstream), a curated catalog of well-known origins, and
//! vim-plug/lazy.nvim-style ergonomics (GitHub shorthand `owner/repo`,
//! commit pinning in the registry lockfile, `restore`/`sync`/`clean`).
//!
//! A *source* is a plugin manager's native tree (no `bundle.toml`); it
//! installs under `~/.niubash/sources/<id>/`, registers untrusted in
//! `~/.niubash/sources/registry.toml`, and only contributes assets after an
//! explicit `niu plugin trust <id>`. Asset-level activation (the managed rc
//! block, `niu plugin enable/disable`) lives in `plugins::assets`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::trust::{SourceSignature, TrustPolicy};
use crate::path_utils::shell_home_dir;

/// Schema marker for the source registry file. `@0.1.0` records carry no
/// trust policy (implicit checksum lock); `@0.2.0` added
/// `trust_policy`/`signature` (§12.1: bump the schema when signatures
/// arrive — both versions stay readable, writes use the current one).
pub const SOURCE_REGISTRY_SCHEMA: &str = "niubash:plugin-source-registry@0.2.0";
/// Ref recorded for local-directory installs (no git ref exists).
pub const LOCAL_ORIGIN_REF: &str = "local";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAssetKind {
    Theme,
    Plugin,
    Alias,
    Completion,
}

impl SourceAssetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Plugin => "plugin",
            Self::Alias => "alias",
            Self::Completion => "completion",
        }
    }
}

/// One loadable asset inside a source tree (a theme script, a plugin, an
/// alias bundle, a completion). Names are flat so they can collide-resolve
/// per §11.3 (external wins, `native:` escapes the external layer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAsset {
    pub kind: SourceAssetKind,
    pub name: String,
    pub path: PathBuf,
}

/// How an adapter's assets are activated and deactivated (`niu plugin
/// enable/disable`). The models keep each manager's *own* selection
/// mechanism (§11: preserve the manager's native layout) instead of
/// inventing a niubash-only switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionModel {
    /// The manager's loader consumes rc arrays plus a theme variable
    /// (oh-my-bash: `plugins=(…)`/`aliases=(…)`/`completions=(…)` read by
    /// `oh-my-bash.sh`, theme via `OSH_THEME`).
    LoaderArrays {
        arrays: &'static [(&'static str, SourceAssetKind)],
        theme_var: &'static str,
    },
    /// The manager keeps its own `enabled/` directory (bash-it): enabling
    /// links/copies `available/<file>` to `enabled/<prio>---<file>`; the
    /// theme still comes from an rc variable.
    EnabledDir {
        subdirs: &'static [(&'static str, SourceAssetKind)],
        theme_var: &'static str,
    },
    /// Activation is whole-source only (bash-completion): the guarded
    /// loader snippet is the unit; enumerated assets are informational.
    WholeSource,
}

/// Rollback state recorded by `update_source` (§12.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePreviousState {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub version: String,
    pub checksum_sha256: String,
    /// Pinned commit (git origins) so `rollback` refetches the exact tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
}

/// One registered external source. Lives in `~/.niubash/sources/registry.toml`.
/// The registry doubles as the lockfile (lazy.nvim's lazy-lock.json): each
/// git-origin record pins `commit_sha` + `checksum_sha256`, and
/// `niu plugin restore` refetches exactly that state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecord {
    /// Source id; equals the adapter id (one tree per manager per machine).
    pub id: String,
    /// Adapter kind that owns detection/listing/loading.
    pub adapter: String,
    /// Origin: git URL or local directory path (snapshot-copied at install).
    pub url: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub version: String,
    pub path: PathBuf,
    pub trusted: bool,
    pub license: String,
    pub checksum_sha256: String,
    pub installed_at: String,
    /// Exact upstream commit this tree was fetched from (git origins; the
    /// lockfile pin behind `niu plugin restore`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    /// Trust tier: `checksum` (hash lock, §12.3) or `local_sign` (this
    /// exact tree was reviewed and signed locally, §12.1 signature tier).
    #[serde(default)]
    pub trust_policy: TrustPolicy,
    /// Local signature over the tree digest (`trust_policy = local_sign`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SourceSignature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<SourcePreviousState>,
}

/// `niu plugin source list` row.
#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    #[serde(flatten)]
    pub record: SourceRecord,
    pub adapter_display: String,
    /// True when the registered directory is missing (native fallback active).
    pub degraded: bool,
    /// "ready" | "untrusted" | "degraded" (§11.4 state machine).
    pub state: String,
    pub asset_count: Option<usize>,
    pub asset_kinds: Vec<String>,
}

/// Result of `verify_source` (§12.3 checksum re-computation, extended with
/// the local-signature tier).
#[derive(Debug, Clone, Serialize)]
pub struct SourceVerifyReport {
    pub id: String,
    pub verified: bool,
    pub degraded: bool,
    pub recorded_checksum: String,
    pub actual_checksum: Option<String>,
    /// Trust tier in effect (`checksum` hash lock / `local_sign`).
    pub trust_policy: TrustPolicy,
    /// Signature check under `local_sign`: `Some(true)` signed and matching,
    /// `Some(false)` present but stale/tampered, `None` not applicable.
    pub signature_ok: Option<bool>,
}

/// `niu plugin source verify <id>` result (§12.3).
pub fn verify_source(id: &str) -> anyhow::Result<SourceVerifyReport> {
    let record = read_source_registry()
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    verify_record(&record)
}

fn verify_record(record: &SourceRecord) -> anyhow::Result<SourceVerifyReport> {
    if !record.path.is_dir() {
        return Ok(SourceVerifyReport {
            id: record.id.clone(),
            verified: false,
            degraded: true,
            recorded_checksum: record.checksum_sha256.clone(),
            actual_checksum: None,
            trust_policy: record.trust_policy,
            signature_ok: record.signature.as_ref().map(|_| false),
        });
    }
    let actual = tree_sha256(&record.path)?;
    let checksum_ok = actual.eq_ignore_ascii_case(&record.checksum_sha256);
    let signature_ok = record
        .signature
        .as_ref()
        .map(|signature| checksum_ok && super::trust::verify_signature(signature, &actual));
    Ok(SourceVerifyReport {
        id: record.id.clone(),
        verified: checksum_ok && signature_ok.unwrap_or(true),
        degraded: false,
        recorded_checksum: record.checksum_sha256.clone(),
        actual_checksum: Some(actual),
        trust_policy: record.trust_policy,
        signature_ok,
    })
}

/// `update_source` result.
#[derive(Debug, Clone, Serialize)]
pub struct SourceUpdateSummary {
    pub id: String,
    pub version: String,
    pub checksum_sha256: String,
    pub previous: Option<SourcePreviousState>,
}

/// Adapter for one external plugin manager (§11.2). The trait covers the
/// manager-specific surface (detect / version / list / loader / selection
/// model); the generic install/update/uninstall protocol in this module is
/// shared by all adapters.
pub trait PluginSourceAdapter: Sync + Send {
    /// Stable adapter id, also the source id and directory name.
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// License of the manager's tree (recorded in the registry and shown at
    /// the trust boundary; §11.5).
    fn license(&self) -> &'static str;
    /// Canonical git origin, shown by `niu plugin discover` so the user can
    /// compose the `add` command themselves. `None` for managers that only
    /// install from local paths.
    fn default_origin(&self) -> Option<&'static str> {
        None
    }
    /// One-line summary for the curated catalog / discover listing.
    fn summary(&self) -> &'static str;
    /// Layout fingerprint: does this tree belong to this plugin manager?
    fn detect(&self, root: &Path) -> bool;
    /// Human-readable version of the installed tree (git HEAD, marker file).
    fn installed_version(&self, root: &Path) -> String;
    /// Enumerate loadable assets (themes, plugins, aliases, completions).
    fn list_assets(&self, root: &Path) -> Vec<SourceAsset>;
    /// Guarded rc snippet that activates the source. Must keep the
    /// existence guard so a missing tree silently falls back to the native
    /// layers (§11.4).
    fn loader_snippet(&self, record: &SourceRecord) -> String;
    /// How `niu plugin enable/disable` operates on this manager's assets
    /// (§11 first-class management; see [`SelectionModel`]).
    fn selection_model(&self) -> SelectionModel;
}

/// oh-my-bash loader adapter. Layout verified against the corpus checkout at
/// `D:/repo/rubash/target-ecosys/repos/oh-my-bash`: root `oh-my-bash.sh`,
/// `themes/<name>/<name>.theme.sh` (82 themes, all directory-shaped),
/// `plugins/<name>/<name>.plugin.sh`, `aliases/*.aliases.{sh,bash}`.
struct OhMyBashAdapter;

impl PluginSourceAdapter for OhMyBashAdapter {
    fn id(&self) -> &'static str {
        "oh-my-bash"
    }
    fn display_name(&self) -> &'static str {
        "oh-my-bash"
    }
    fn license(&self) -> &'static str {
        "MIT"
    }
    fn default_origin(&self) -> Option<&'static str> {
        // §12.1 index entry: source-url of the official oh-my-bash record.
        Some("https://github.com/ohmybash/oh-my-bash.git")
    }
    fn summary(&self) -> &'static str {
        "bash framework: 80+ themes, plugins and aliases"
    }
    fn detect(&self, root: &Path) -> bool {
        root.join("oh-my-bash.sh").is_file()
    }
    fn installed_version(&self, root: &Path) -> String {
        git_head_short_sha(root).unwrap_or_else(|| "unknown".to_string())
    }
    fn list_assets(&self, root: &Path) -> Vec<SourceAsset> {
        let mut assets = Vec::new();
        // themes/<name>/<name>.theme.sh (corpus shape) and defensive flat
        // themes/<name>.theme.sh.
        let themes_dir = root.join("themes");
        if let Ok(entries) = fs::read_dir(&themes_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    let script = path.join(format!("{name}.theme.sh"));
                    if script.is_file() {
                        assets.push(SourceAsset {
                            kind: SourceAssetKind::Theme,
                            name: name.to_string(),
                            path: script,
                        });
                    }
                } else if path.extension().and_then(|e| e.to_str()) == Some("sh") {
                    if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                        if let Some(name) = name.strip_suffix(".theme") {
                            assets.push(SourceAsset {
                                kind: SourceAssetKind::Theme,
                                name: name.to_string(),
                                path: path.clone(),
                            });
                        }
                    }
                }
            }
        }
        // plugins/<name>/<name>.plugin.sh (or any *.plugin.sh inside).
        let plugins_dir = root.join("plugins");
        if let Ok(entries) = fs::read_dir(&plugins_dir) {
            for entry in entries.filter_map(Result::ok) {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let script = dir.join(format!("{name}.plugin.sh"));
                if script.is_file() {
                    assets.push(SourceAsset {
                        kind: SourceAssetKind::Plugin,
                        name: name.to_string(),
                        path: script,
                    });
                }
            }
        }
        // aliases/<name>.aliases.sh|bash — flat files.
        let aliases_dir = root.join("aliases");
        if let Ok(entries) = fs::read_dir(&aliases_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(name) = file
                    .strip_suffix(".aliases.sh")
                    .or_else(|| file.strip_suffix(".aliases.bash"))
                else {
                    continue;
                };
                if !name.is_empty() {
                    assets.push(SourceAsset {
                        kind: SourceAssetKind::Alias,
                        name: name.to_string(),
                        path: path.clone(),
                    });
                }
            }
        }
        // completions/<name>.completion.sh|bash — flat files
        // (`_omb_module_require_completion` layout).
        let completions_dir = root.join("completions");
        if let Ok(entries) = fs::read_dir(&completions_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(name) = file
                    .strip_suffix(".completion.sh")
                    .or_else(|| file.strip_suffix(".completion.bash"))
                else {
                    continue;
                };
                if !name.is_empty() {
                    assets.push(SourceAsset {
                        kind: SourceAssetKind::Completion,
                        name: name.to_string(),
                        path: path.clone(),
                    });
                }
            }
        }
        assets.sort_by(|left, right| {
            (left.kind.as_str(), &left.name).cmp(&(right.kind.as_str(), &right.name))
        });
        assets
    }
    fn loader_snippet(&self, record: &SourceRecord) -> String {
        // §3.3 + §11.4: keep oh-my-bash's own loading path (source
        // oh-my-bash.sh after exporting OSH), guard on readability so a
        // missing tree falls back silently (§11.4), and mark the theme
        // channel for the bash-compatible PS1 renderer. The theme itself is
        // NOT defaulted here: `OSH_THEME` comes from the managed rc block
        // when the user picked one; unset means "load plugins/aliases only,
        // keep the niubash prompt".
        //
        // wt44/niu365 (owner ruling: loader fidelity, no shim layer):
        // framework assets are enabled ONLY through this native loader —
        // the rc arrays are consumed by oh-my-bash.sh itself, which also
        // provides the framework lib (`_omb_deprecate_*`, `_omb_util_*`).
        // A user manually sourcing a framework plugin file outside the
        // loader gets the same missing-function errors GNU bash would
        // produce — that equivalence is the fidelity contract (design doc
        // appendix D, "框架资产经 loader、独立资产直接 source，无垫片层").
        let base = format!(
            "${{NIU_PLUGIN_SOURCES_ROOT:-$HOME/.niubash/sources}}/{}",
            record.id
        );
        format!(
            "# {id} source (external, primary; fallback stays active when absent)\n\
             if [ -r \"{base}/oh-my-bash.sh\" ]; then\n\
             \x20 OSH=\"{base}\"\n\
             \x20 OSH=\"${{OSH//\\\\//}}\"\n\
             \x20 export OSH\n\
             \x20 export NIU_THEME_SOURCE=omb\n\
             \x20 . \"$OSH/oh-my-bash.sh\"\n\
             fi\n",
            id = record.id,
            base = base,
        )
    }
    fn selection_model(&self) -> SelectionModel {
        SelectionModel::LoaderArrays {
            arrays: &[
                ("plugins", SourceAssetKind::Plugin),
                ("aliases", SourceAssetKind::Alias),
                ("completions", SourceAssetKind::Completion),
            ],
            theme_var: "OSH_THEME",
        }
    }
}

/// bash-it loader adapter. Layout verified against the corpus checkout at
/// `D:/repo/rubash/target-ecosys/repos/bash-it`: root `bash_it.sh` +
/// `lib/composure.bash`, `aliases/available/<name>.aliases.bash`,
/// `plugins/available/<name>.plugin.bash`,
/// `completion/available/<name>.completion.bash`, `themes/<name>/<name>.theme.bash`,
/// activation through `enabled/<priority>---<file>` entries that
/// `scripts/reloader.bash` sources (design §11.2 / WP-S2).
struct BashItAdapter;

/// Subdir + kind table shared by listing and the EnabledDir selection model.
const BASH_IT_SUBDIRS: &[(&str, SourceAssetKind)] = &[
    ("aliases", SourceAssetKind::Alias),
    ("plugins", SourceAssetKind::Plugin),
    ("completion", SourceAssetKind::Completion),
];

impl PluginSourceAdapter for BashItAdapter {
    fn id(&self) -> &'static str {
        "bash-it"
    }
    fn display_name(&self) -> &'static str {
        "bash-it"
    }
    fn license(&self) -> &'static str {
        "MIT"
    }
    fn default_origin(&self) -> Option<&'static str> {
        Some("https://github.com/Bash-it/bash-it.git")
    }
    fn summary(&self) -> &'static str {
        "bash framework: community aliases, plugins, completions, themes"
    }
    fn detect(&self, root: &Path) -> bool {
        root.join("bash_it.sh").is_file() && root.join("lib").join("composure.bash").is_file()
    }
    fn installed_version(&self, root: &Path) -> String {
        git_head_short_sha(root).unwrap_or_else(|| "unknown".to_string())
    }
    fn list_assets(&self, root: &Path) -> Vec<SourceAsset> {
        let mut assets = Vec::new();
        for (subdir, kind) in BASH_IT_SUBDIRS {
            let available = root.join(subdir).join("available");
            let Ok(entries) = fs::read_dir(&available) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(name) = file
                    .strip_suffix(".aliases.bash")
                    .or_else(|| file.strip_suffix(".plugin.bash"))
                    .or_else(|| file.strip_suffix(".completion.bash"))
                else {
                    continue;
                };
                if !name.is_empty() {
                    assets.push(SourceAsset {
                        kind: *kind,
                        name: name.to_string(),
                        path: path.clone(),
                    });
                }
            }
        }
        // themes/<name>/<name>.theme.bash
        let themes_dir = root.join("themes");
        if let Ok(entries) = fs::read_dir(&themes_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let script = path.join(format!("{name}.theme.bash"));
                if script.is_file() {
                    assets.push(SourceAsset {
                        kind: SourceAssetKind::Theme,
                        name: name.to_string(),
                        path: script,
                    });
                }
            }
        }
        assets.sort_by(|left, right| {
            (left.kind.as_str(), &left.name).cmp(&(right.kind.as_str(), &right.name))
        });
        assets
    }
    fn loader_snippet(&self, record: &SourceRecord) -> String {
        // Keep bash-it's own loading path (§11.2): source bash_it.sh, which
        // loads libraries and everything in enabled/. BASH_IT_THEME comes
        // from the managed block when a theme was picked; unset keeps the
        // niubash prompt. The existence guard keeps the fallback alive.
        let base = format!(
            "${{NIU_PLUGIN_SOURCES_ROOT:-$HOME/.niubash/sources}}/{}",
            record.id
        );
        format!(
            "# {id} source (external, primary; fallback stays active when absent)\n\
             BASH_IT=\"{base}\"\n\
             BASH_IT=\"${{BASH_IT//\\\\//}}\"\n\
             export BASH_IT\n\
             if [ -r \"$BASH_IT/bash_it.sh\" ]; then\n\
             \x20 . \"$BASH_IT/bash_it.sh\"\n\
             fi\n",
            id = record.id,
            base = base,
        )
    }
    fn selection_model(&self) -> SelectionModel {
        SelectionModel::EnabledDir {
            subdirs: BASH_IT_SUBDIRS,
            theme_var: "BASH_IT_THEME",
        }
    }
}

/// bash-completion adapter (design §4.1, adapted to the source protocol):
/// the upstream tree (GPL-2.0-or-later — fetch-on-demand only, never
/// vendored, owner ruling 2026-10-02). Layout per corpus: root
/// `bash_completion` script + `completions/` scripts. Activation is
/// whole-source: the guarded snippet sources `bash_completion`, which
/// registers `complete -D` dynamic loading (engine rubash#133 closed).
struct BashCompletionAdapter;

impl PluginSourceAdapter for BashCompletionAdapter {
    fn id(&self) -> &'static str {
        "bash-completion"
    }
    fn display_name(&self) -> &'static str {
        "bash-completion"
    }
    fn license(&self) -> &'static str {
        "GPL-2.0-or-later"
    }
    fn default_origin(&self) -> Option<&'static str> {
        Some("https://github.com/scop/bash-completion.git")
    }
    fn summary(&self) -> &'static str {
        "the standard bash completion library (fetched on demand; GPL-2.0+)"
    }
    fn detect(&self, root: &Path) -> bool {
        root.join("bash_completion").is_file()
    }
    fn installed_version(&self, root: &Path) -> String {
        git_head_short_sha(root).unwrap_or_else(|| "unknown".to_string())
    }
    fn list_assets(&self, root: &Path) -> Vec<SourceAsset> {
        let mut assets = Vec::new();
        let completions_dir = root.join("completions");
        if let Ok(entries) = fs::read_dir(&completions_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let extension = path.extension().and_then(|e| e.to_str());
                if extension != Some("bash") && extension != Some("sh") {
                    continue;
                }
                let Some(name) = path.file_stem().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !name.is_empty() {
                    assets.push(SourceAsset {
                        kind: SourceAssetKind::Completion,
                        name: name.to_string(),
                        path: path.clone(),
                    });
                }
            }
        }
        assets.sort_by(|left, right| left.name.cmp(&right.name));
        assets
    }
    fn loader_snippet(&self, record: &SourceRecord) -> String {
        let base = format!(
            "${{NIU_PLUGIN_SOURCES_ROOT:-$HOME/.niubash/sources}}/{}",
            record.id
        );
        format!(
            "# {id} source (external, primary; native completions stay as fallback)\n\
             if [ -r \"{base}/bash_completion\" ]; then\n\
             \x20 . \"{base}/bash_completion\"\n\
             fi\n",
            id = record.id,
            base = base,
        )
    }
    fn selection_model(&self) -> SelectionModel {
        SelectionModel::WholeSource
    }
}

/// Full commit sha from a `.git` directory (shallow or not), if it resolves
/// to a concrete commit. This is the lockfile pin (`niu plugin restore`).
fn git_head_full_sha(root: &Path) -> Option<String> {
    let head = fs::read_to_string(root.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    let sha = if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        if let Ok(direct) = fs::read_to_string(root.join(".git").join(reference)) {
            direct.trim().to_string()
        } else {
            let packed = fs::read_to_string(root.join(".git").join("packed-refs")).ok()?;
            packed
                .lines()
                .find_map(|line| {
                    let (hash, name) = line.split_once(' ')?;
                    (name.trim() == reference).then(|| hash.to_string())
                })
                .unwrap_or_default()
        }
    } else {
        head.to_string()
    };
    (!sha.is_empty()).then(|| sha.to_string())
}

/// Best-effort `<sha>` (12 hex chars) from a `.git` directory, shallow or
/// not. Used only for display/versioning; the trust anchor is the tree
/// checksum, not this.
fn git_head_short_sha(root: &Path) -> Option<String> {
    git_head_full_sha(root).map(|sha| {
        let short: String = sha.chars().take(12).collect();
        format!("git-{short}")
    })
}

/// Adapters compiled into this build. bash-it / bash-completion land here
/// (WP-S2 + §4.1-as-source); bpkg stays future work (WP-S3).
pub fn builtin_source_adapters() -> &'static [&'static dyn PluginSourceAdapter] {
    &[&OhMyBashAdapter, &BashItAdapter, &BashCompletionAdapter]
}

/// Normalize an install target the way vim-plug/lazy.nvim users expect:
/// `owner/repo` expands to the GitHub origin (`https://github.com/owner/repo.git`)
/// unless it names an existing local directory. Full URLs, absolute paths,
/// and relative paths (`./x`, `../x`) pass through untouched.
pub fn normalize_origin(target: &str) -> String {
    let target = target.trim();
    if target.is_empty() {
        return String::new();
    }
    let path = Path::new(target);
    if path.is_dir()
        || path.is_absolute()
        || target.starts_with("./")
        || target.starts_with("../")
        || target.contains("://")
        || target.starts_with("git@")
    {
        return target.to_string();
    }
    // GitHub shorthand: exactly `owner/repo` with sane characters.
    let looks_like_shorthand = target
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        && target.matches('/').count() == 1
        && !target.starts_with('.')
        && !target.ends_with('/');
    if looks_like_shorthand {
        return format!("https://github.com/{target}.git");
    }
    target.to_string()
}

/// Look up an adapter by kind id.
pub fn adapter_for(kind: &str) -> Option<&'static dyn PluginSourceAdapter> {
    builtin_source_adapters()
        .into_iter()
        .copied()
        .find(|adapter| adapter.id() == kind)
}

/// Detect which adapter owns a tree by layout fingerprint.
pub fn detect_source_adapter(root: &Path) -> Option<&'static dyn PluginSourceAdapter> {
    builtin_source_adapters()
        .into_iter()
        .copied()
        .find(|adapter| adapter.detect(root))
}

fn supported_adapters_hint() -> String {
    let ids: Vec<&str> = builtin_source_adapters().iter().map(|a| a.id()).collect();
    format!("supported plugin-manager sources: {}", ids.join(", "))
}

/// Root directory for external plugin-manager sources: `~/.niubash/sources`.
/// `NIU_PLUGIN_SOURCES_ROOT` overrides the location (tests, portable setup).
pub fn sources_root() -> PathBuf {
    if let Some(value) = std::env::var_os("NIU_PLUGIN_SOURCES_ROOT") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    shell_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".niubash")
        .join("sources")
}

fn registry_path() -> PathBuf {
    sources_root().join("registry.toml")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SourceRegistryToml {
    schema: Option<String>,
    #[serde(default)]
    sources: Vec<SourceRecord>,
}

/// Read all registered sources.
pub fn read_source_registry() -> Vec<SourceRecord> {
    let Ok(text) = fs::read_to_string(registry_path()) else {
        return Vec::new();
    };
    toml::from_str::<SourceRegistryToml>(&text)
        .map(|registry| registry.sources)
        .unwrap_or_else(|err| {
            log::warn!("failed to parse plugin source registry: {}", err);
            Vec::new()
        })
}

fn write_source_registry(sources: &[SourceRecord]) -> anyhow::Result<()> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(&SourceRegistryToml {
        schema: Some(SOURCE_REGISTRY_SCHEMA.to_string()),
        sources: sources.to_vec(),
    })?;
    fs::write(path, text)?;
    Ok(())
}

fn now_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Deterministic tree checksum (§12.3): recursively enumerate files, skip
/// `.git`, sort by POSIX relative path, hash `path \0 len \0 content` per
/// file. Stable across checkouts of the same tree and across Windows/POSIX
/// path separators.
pub fn tree_sha256(root: &Path) -> anyhow::Result<String> {
    let mut files = Vec::new();
    collect_relative_files(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        let full = root.join(&relative);
        let content =
            fs::read(&full).with_context(|| format!("failed to read {}", full.display()))?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(content.len().to_le_bytes());
        hasher.update([0]);
        hasher.update(&content);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_relative_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> anyhow::Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        if name == ".git" {
            continue;
        }
        if path.is_dir() {
            collect_relative_files(root, &path, out)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("path {} escaped root", path.display()))?;
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dest.join(entry.file_name());
        if path.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            copy_tree(&path, &target)?;
        } else if path.is_file() {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Install request for `add_source` / `update_source` (§12.2).
#[derive(Debug, Clone, Default)]
pub struct SourceInstallRequest {
    /// Adapter kind hint. When `None`, the fetched tree is auto-detected.
    pub adapter: Option<String>,
    /// Git URL or local directory path.
    pub origin: String,
    /// Git ref for URL origins; defaults to `HEAD`.
    pub ref_name: Option<String>,
    /// Exact commit to fetch (git origins) — the lockfile pin used by
    /// `restore_source`. Takes precedence over `ref_name`'s tip.
    pub commit: Option<String>,
    /// Expected tree checksum (hex sha256). Verified on staging before
    /// promotion; mismatch aborts and leaves any existing install intact.
    pub expected_checksum: Option<String>,
}

struct FetchedSource {
    staging: PathBuf,
    adapter: &'static dyn PluginSourceAdapter,
    version: String,
    checksum_sha256: String,
    commit_sha: Option<String>,
}

fn git_clone_to(staging: &Path, origin: &str, ref_name: &str) -> anyhow::Result<()> {
    let mut command = Command::new("git");
    // OMB has no .gitattributes; CRLF would kill sourcing (§9).
    command
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg("clone")
        .arg("--depth")
        .arg("1");
    if ref_name != "HEAD" {
        command.arg("--branch").arg(ref_name);
    }
    command.arg(origin).arg(staging);
    let status = command
        .status()
        .with_context(|| "failed to run git; is git.exe on PATH?")?;
    if !status.success() {
        anyhow::bail!(
            "git clone exited with status {}",
            status.code().unwrap_or(1)
        );
    }
    Ok(())
}

/// Fetch one exact commit without a branch clone: init + shallow
/// `fetch origin <sha>` + checkout FETCH_HEAD. Works on GitHub (allows
/// fetching reachable SHAs) and local repositories.
fn git_fetch_commit_to(staging: &Path, origin: &str, commit: &str) -> anyhow::Result<()> {
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let status = Command::new("git")
            .arg("-C")
            .arg(staging)
            .arg("-c")
            .arg("core.autocrlf=false")
            .args(args)
            .status()
            .with_context(|| "failed to run git; is git.exe on PATH?")?;
        if !status.success() {
            anyhow::bail!(
                "git {} exited with status {}",
                args.first().copied().unwrap_or(""),
                status.code().unwrap_or(1)
            );
        }
        Ok(())
    };
    fs::create_dir_all(staging)?;
    run(&["init", "-q"])?;
    run(&["remote", "add", "origin", origin])?;
    if run(&["fetch", "--depth", "1", "-q", "origin", commit]).is_err() {
        // Some servers refuse fetch-by-sha; fall back to a plain shallow
        // clone of the default branch and check the commit out from it.
        fs::remove_dir_all(staging)?;
        git_clone_to(staging, origin, "HEAD")?;
        run(&["checkout", "-q", commit])?;
        return Ok(());
    }
    run(&["checkout", "-q", "FETCH_HEAD"])?;
    Ok(())
}

/// Shared fetch pipeline (§12.2): fetch to staging, detect adapter, compute
/// the tree checksum, and verify the expected checksum when provided.
/// Staging is always removed on failure.
fn fetch_source_to_staging(request: &SourceInstallRequest) -> anyhow::Result<FetchedSource> {
    let origin = request.origin.trim();
    if origin.is_empty() {
        anyhow::bail!("source origin is empty");
    }
    let staging = unique_staging_path();
    if let Some(parent) = staging.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(&staging)?;
    let staging_for_cleanup = staging.clone();
    let origin_path = Path::new(origin);
    let result = (|| -> anyhow::Result<FetchedSource> {
        // Origin semantics: an explicit ref or commit forces git semantics
        // (a local git repository path is also a directory and must clone,
        // not snapshot — the working tree may be ahead of the requested
        // ref). Without either, directories snapshot and everything else
        // clones.
        let commit = request
            .commit
            .clone()
            .filter(|value| !value.trim().is_empty());
        let explicit_ref = request
            .ref_name
            .clone()
            .filter(|value| value != LOCAL_ORIGIN_REF);
        let ref_name = match &explicit_ref {
            Some(value) => value.clone(),
            None if commit.is_some() || !origin_path.is_dir() => "HEAD".to_string(),
            None => LOCAL_ORIGIN_REF.to_string(),
        };
        if commit.is_some() {
            git_fetch_commit_to(&staging, origin, commit.as_deref().unwrap())?;
        } else if ref_name == LOCAL_ORIGIN_REF {
            copy_tree(origin_path, &staging)?;
        } else {
            git_clone_to(&staging, origin, &ref_name)?;
        }

        let adapter = match &request.adapter {
            Some(kind) => {
                let adapter =
                    adapter_for(kind).ok_or_else(|| anyhow!("unknown source kind '{kind}'"))?;
                if !adapter.detect(&staging) {
                    anyhow::bail!(
                        "fetched tree at '{}' does not look like '{}'",
                        origin,
                        adapter.id()
                    );
                }
                adapter
            }
            None => detect_source_adapter(&staging).ok_or_else(|| {
                anyhow!(
                    "no supported plugin manager detected in '{}'; {}",
                    origin,
                    supported_adapters_hint()
                )
            })?,
        };
        let version = adapter.installed_version(&staging);
        let checksum_sha256 = tree_sha256(&staging)?;
        if let Some(expected) = request.expected_checksum.as_deref() {
            if !checksum_sha256.eq_ignore_ascii_case(expected.trim()) {
                anyhow::bail!(
                    "checksum mismatch for source '{}': expected {}, got {}",
                    adapter.id(),
                    expected,
                    checksum_sha256
                );
            }
        }
        let commit_sha = git_head_full_sha(&staging);
        Ok(FetchedSource {
            staging,
            adapter,
            version,
            checksum_sha256,
            commit_sha,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging_for_cleanup);
    }
    result
}

fn unique_staging_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    sources_root().join(format!(".staging-{}-{stamp}", std::process::id()))
}

/// Remove an installed tree, tolerating an already-missing directory
/// (degraded sources restore/rollback onto a fresh fetch).
fn remove_tree_if_present(path: &Path) -> anyhow::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn promote_staging(staging: &Path, dest: &Path) -> anyhow::Result<()> {
    if fs::rename(staging, dest).is_err() {
        copy_tree(staging, dest)?;
        let _ = fs::remove_dir_all(staging);
    }
    Ok(())
}

/// Install an external plugin-manager source. The fetched tree is verified,
/// promoted to `sources/<adapter-id>/`, and registered **untrusted** — none
/// of its assets activate until `trust_source` (§12.2 execution gate).
pub fn add_source(request: SourceInstallRequest) -> anyhow::Result<SourceRecord> {
    let fetched = fetch_source_to_staging(&request)?;
    let id = fetched.adapter.id();
    let mut registry = read_source_registry();
    if registry.iter().any(|record| record.id == id) {
        anyhow::bail!(
            "source '{id}' is already registered; remove it first with niu plugin source remove {id}"
        );
    }
    let dest = sources_root().join(id);
    if dest.exists() {
        anyhow::bail!("directory {} already exists", dest.display());
    }
    promote_staging(&fetched.staging, &dest)?;

    let record = SourceRecord {
        id: id.to_string(),
        adapter: id.to_string(),
        url: request.origin.trim().to_string(),
        ref_name: request
            .ref_name
            .clone()
            .unwrap_or_else(|| LOCAL_ORIGIN_REF.to_string()),
        version: fetched.version,
        path: dest,
        trusted: false,
        license: fetched.adapter.license().to_string(),
        checksum_sha256: fetched.checksum_sha256,
        installed_at: now_timestamp(),
        commit_sha: fetched.commit_sha,
        trust_policy: TrustPolicy::default(),
        signature: None,
        previous: None,
    };
    registry.push(record.clone());
    write_source_registry(&registry)?;
    Ok(record)
}

/// Update a registered source to a new ref/tree. The previous state is
/// recorded for `rollback_source` (§12.4). Trust persists only when the
/// origin URL is unchanged; a changed origin resets trust (re-pass the
/// execution gate).
pub fn update_source(
    id: &str,
    request: SourceInstallRequest,
) -> anyhow::Result<SourceUpdateSummary> {
    let mut registry = read_source_registry();
    let index = registry
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    let record = registry[index].clone();
    // An update without an explicit origin re-fetches the registered origin.
    let mut request = request;
    if request.origin.trim().is_empty() {
        request.origin = record.url.clone();
    }
    let fetched = fetch_source_to_staging(&request)?;

    let origin_changed = request.origin.trim() != record.url;
    let previous = SourcePreviousState {
        ref_name: record.ref_name.clone(),
        version: record.version.clone(),
        checksum_sha256: record.checksum_sha256.clone(),
        commit_sha: record.commit_sha.clone(),
    };
    remove_tree_if_present(&record.path)
        .with_context(|| format!("failed to remove old tree {}", record.path.display()))?;
    promote_staging(&fetched.staging, &record.path)?;

    // Trust semantics (§12.4 + the signature tier): same-origin updates
    // keep trust under the checksum policy; under the local-signature
    // policy only the exact signed tree stays trusted, so any changed tree
    // (new upstream fetch) re-enters the execution gate until re-signed.
    let tree_unchanged = fetched.checksum_sha256 == record.checksum_sha256;
    let keep_trust =
        !origin_changed && (record.trust_policy == TrustPolicy::Checksum || tree_unchanged);
    let updated = SourceRecord {
        id: record.id.clone(),
        adapter: record.adapter.clone(),
        url: request.origin.trim().to_string(),
        ref_name: request.ref_name.clone().unwrap_or(record.ref_name.clone()),
        version: fetched.version,
        path: record.path.clone(),
        trusted: record.trusted && keep_trust,
        license: fetched.adapter.license().to_string(),
        checksum_sha256: fetched.checksum_sha256,
        installed_at: now_timestamp(),
        commit_sha: fetched.commit_sha,
        trust_policy: record.trust_policy,
        signature: record.signature,
        previous: Some(previous),
    };
    let summary = SourceUpdateSummary {
        id: updated.id.clone(),
        version: updated.version.clone(),
        checksum_sha256: updated.checksum_sha256.clone(),
        previous: updated.previous.clone(),
    };
    registry[index] = updated;
    write_source_registry(&registry)?;
    Ok(summary)
}

/// Roll back to the recorded previous state (§12.4): re-fetch the previous
/// ref, verify it against the previous checksum, restore the record. Local
/// origins whose tree drifted fail the checksum and roll back is refused
/// (reinstall from the old tree instead).
pub fn rollback_source(id: &str) -> anyhow::Result<SourceUpdateSummary> {
    let mut registry = read_source_registry();
    let index = registry
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    let record = registry[index].clone();
    let previous = record
        .previous
        .clone()
        .ok_or_else(|| anyhow!("no previous state recorded for '{id}'"))?;

    // Prefer the exact commit pin; fall back to the recorded ref (and to a
    // plain snapshot for local origins, which rollback then rejects via the
    // checksum when the origin drifted).
    let ref_fallback = if previous.commit_sha.is_some() || previous.ref_name == LOCAL_ORIGIN_REF {
        None
    } else {
        Some(previous.ref_name.clone())
    };
    let fetched = fetch_source_to_staging(&SourceInstallRequest {
        adapter: Some(record.adapter.clone()),
        origin: record.url.clone(),
        commit: previous.commit_sha.clone(),
        ref_name: ref_fallback,
        expected_checksum: Some(previous.checksum_sha256.clone()),
    })?;

    remove_tree_if_present(&record.path)
        .with_context(|| format!("failed to remove tree {}", record.path.display()))?;
    promote_staging(&fetched.staging, &record.path)?;

    let restored = SourceRecord {
        trusted: record.trusted,
        ref_name: previous.ref_name.clone(),
        version: previous.version.clone(),
        checksum_sha256: previous.checksum_sha256.clone(),
        commit_sha: previous.commit_sha.clone().or(fetched.commit_sha.clone()),
        previous: None,
        installed_at: now_timestamp(),
        ..record
    };
    let summary = SourceUpdateSummary {
        id: restored.id.clone(),
        version: restored.version.clone(),
        checksum_sha256: restored.checksum_sha256.clone(),
        previous: Some(previous),
    };
    registry[index] = restored;
    write_source_registry(&registry)?;
    Ok(summary)
}

/// Flip the execution gate for a source: its assets start contributing to
/// the catalog/loader (§12.2). The gate is only flipped on a healthy tree —
/// a missing directory (degraded) or a checksum mismatch refuses trust
/// until the tree is restored (`niu plugin restore <id>`).
pub fn trust_source(id: &str) -> anyhow::Result<SourceRecord> {
    let mut registry = read_source_registry();
    let index = registry
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    let record = registry[index].clone();
    if !record.path.is_dir() {
        anyhow::bail!(
            "cannot trust '{}': the installed tree is missing (degraded); \
             restore it with `niu plugin restore {id}`",
            record.id
        );
    }
    let actual = tree_sha256(&record.path)?;
    if !actual.eq_ignore_ascii_case(&record.checksum_sha256) {
        anyhow::bail!(
            "cannot trust '{}': tree checksum mismatch (recorded {}, actual {}); \
             restore the pristine tree with `niu plugin restore {id}`",
            record.id,
            record.checksum_sha256,
            actual
        );
    }
    registry[index].trusted = true;
    let trusted = registry[index].clone();
    write_source_registry(&registry)?;
    Ok(trusted)
}

/// Promote a source to the local-signature trust tier (`niu plugin source
/// sign <id>`): verify the tree matches the recorded checksum, then sign
/// that digest with the machine-local ed25519 key. Signing implies trust —
/// you cannot meaningfully sign a tree and leave it untrusted — so the
/// execution gate flips too, exactly like `trust` (which still applies the
/// same health checks).
pub fn sign_source(id: &str) -> anyhow::Result<SourceRecord> {
    let mut registry = read_source_registry();
    let index = registry
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    let record = registry[index].clone();
    if !record.path.is_dir() {
        anyhow::bail!(
            "cannot sign '{}': the installed tree is missing (degraded); \
             restore it with `niu plugin restore {id}` first",
            record.id
        );
    }
    let actual = tree_sha256(&record.path)?;
    if !actual.eq_ignore_ascii_case(&record.checksum_sha256) {
        anyhow::bail!(
            "cannot sign '{}': tree checksum mismatch (recorded {}, actual {}); \
             restore the pristine tree first (`niu plugin restore {id}`)",
            record.id,
            record.checksum_sha256,
            actual
        );
    }
    let signature = super::trust::sign_digest(&actual)?;
    registry[index].trusted = true;
    registry[index].trust_policy = TrustPolicy::LocalSign;
    registry[index].signature = Some(signature);
    let signed = registry[index].clone();
    write_source_registry(&registry)?;
    Ok(signed)
}

/// Uninstall: delete the tree (only directories we placed under the sources
/// root) and the registry entry. Trust state is irrelevant for removal.
pub fn remove_source(id: &str) -> anyhow::Result<PathBuf> {
    let mut registry = read_source_registry();
    let index = registry
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    let record = registry.remove(index);
    write_source_registry(&registry)?;

    let root = sources_root();
    if record.path.starts_with(&root)
        && record.path != root
        && record.path.is_dir()
        // Only remove directories that still match the registered layout.
        && adapter_for(&record.adapter).is_some_and(|a| a.detect(&record.path))
    {
        fs::remove_dir_all(&record.path)
            .with_context(|| format!("failed to remove {}", record.path.display()))?;
    }
    Ok(record.path)
}

/// `niu plugin source list` rows (§11.4 state machine).
pub fn list_sources() -> Vec<SourceStatus> {
    let mut out = Vec::new();
    for record in read_source_registry() {
        let adapter = adapter_for(&record.adapter);
        let degraded = !record.path.is_dir();
        let (assets, adapter_display) = match adapter {
            Some(adapter) if !degraded => (
                adapter.list_assets(&record.path),
                adapter.display_name().to_string(),
            ),
            Some(adapter) => (Vec::new(), adapter.display_name().to_string()),
            None => (Vec::new(), record.adapter.clone()),
        };
        let state = if degraded {
            "degraded".to_string()
        } else if !record.trusted {
            "untrusted".to_string()
        } else {
            "ready".to_string()
        };
        let mut kinds: Vec<String> = assets
            .iter()
            .map(|asset| asset.kind.as_str().to_string())
            .collect();
        kinds.dedup();
        out.push(SourceStatus {
            degraded,
            state,
            asset_count: (!degraded).then(|| assets.len()),
            asset_kinds: kinds,
            adapter_display,
            record,
        });
    }
    out.sort_by(|left, right| left.record.id.cmp(&right.record.id));
    out
}

/// Result row of `restore_source` / `sync_sources`.
#[derive(Debug, Clone, Serialize)]
pub struct SourceSyncOutcome {
    pub id: String,
    pub outcome: String,
    pub detail: String,
}

/// Restore a source to the exact state pinned in the registry lockfile
/// (lazy.nvim `:Lazy restore` semantics): re-fetch the recorded commit (or
/// ref), verify against the recorded checksum, replace the tree in place.
/// Local-path origins cannot rebuild a tree; those restore attempts explain
/// and fail (reinstall from the origin instead).
pub fn restore_source(id: &str) -> anyhow::Result<SourceSyncOutcome> {
    let mut registry = read_source_registry();
    let index = registry
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    let record = registry[index].clone();
    if record.ref_name == LOCAL_ORIGIN_REF && record.commit_sha.is_none() {
        anyhow::bail!(
            "source '{id}' was installed from a local directory snapshot; \
             there is no upstream to restore from — reinstall it from the tree"
        );
    }
    let fetched = fetch_source_to_staging(&SourceInstallRequest {
        adapter: Some(record.adapter.clone()),
        origin: record.url.clone(),
        commit: record.commit_sha.clone(),
        ref_name: if record.commit_sha.is_some() {
            None
        } else {
            Some(record.ref_name.clone())
        },
        expected_checksum: Some(record.checksum_sha256.clone()),
    })?;
    remove_tree_if_present(&record.path)
        .with_context(|| format!("failed to remove tree {}", record.path.display()))?;
    promote_staging(&fetched.staging, &record.path)?;
    registry[index].installed_at = now_timestamp();
    write_source_registry(&registry)?;
    Ok(SourceSyncOutcome {
        id: record.id.clone(),
        outcome: "restored".to_string(),
        detail: format!(
            "tree matches the lockfile pin ({})",
            record
                .commit_sha
                .as_deref()
                .map(|sha| format!("commit {sha}"))
                .unwrap_or_else(|| record.ref_name.clone()),
        ),
    })
}

/// `niu plugin sync` (vim-plug `:PlugUpdate` semantics): update every
/// git-origin source to its recorded ref's current tip. Local snapshots are
/// reported as skipped. Returns one row per source.
pub fn sync_sources() -> Vec<SourceSyncOutcome> {
    let mut out = Vec::new();
    for record in read_source_registry() {
        if record.ref_name == LOCAL_ORIGIN_REF && record.commit_sha.is_none() {
            out.push(SourceSyncOutcome {
                id: record.id.clone(),
                outcome: "skipped".to_string(),
                detail: "local snapshot — update by reinstalling from the tree".to_string(),
            });
            continue;
        }
        let ref_name = if record.ref_name == LOCAL_ORIGIN_REF {
            "HEAD".to_string()
        } else {
            record.ref_name.clone()
        };
        match update_source(
            &record.id,
            SourceInstallRequest {
                adapter: Some(record.adapter.clone()),
                origin: record.url.clone(),
                ref_name: Some(ref_name),
                ..SourceInstallRequest::default()
            },
        ) {
            Ok(summary) => out.push(SourceSyncOutcome {
                id: record.id.clone(),
                outcome: "updated".to_string(),
                detail: format!("now at {} ({})", summary.version, summary.checksum_sha256),
            }),
            Err(err) => out.push(SourceSyncOutcome {
                id: record.id.clone(),
                outcome: "failed".to_string(),
                detail: err.to_string(),
            }),
        }
    }
    out
}

/// `niu plugin clean` (vim-plug `:PlugClean` semantics): remove leftover
/// `.staging-*` directories from interrupted installs and orphaned trees
/// under the sources root that no registry record claims but a known
/// adapter would own. Registered sources and unknown directories are never
/// touched.
pub fn clean_sources() -> Vec<SourceSyncOutcome> {
    let mut out = Vec::new();
    let root = sources_root();
    let registered: Vec<PathBuf> = read_source_registry()
        .into_iter()
        .map(|record| record.path.clone())
        .collect();
    let Ok(entries) = fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        if !path.is_dir() {
            continue;
        }
        if name.starts_with(".staging-") {
            match fs::remove_dir_all(&path) {
                Ok(()) => out.push(SourceSyncOutcome {
                    id: name.clone(),
                    outcome: "removed".to_string(),
                    detail: "interrupted-install staging directory".to_string(),
                }),
                Err(err) => out.push(SourceSyncOutcome {
                    id: name.clone(),
                    outcome: "failed".to_string(),
                    detail: err.to_string(),
                }),
            }
            continue;
        }
        if registered.iter().any(|registered| *registered == path) {
            continue;
        }
        // Orphan: a manager-shaped tree with no registry record.
        if builtin_source_adapters()
            .iter()
            .any(|adapter| adapter.detect(&path))
        {
            match fs::remove_dir_all(&path) {
                Ok(()) => out.push(SourceSyncOutcome {
                    id: name.clone(),
                    outcome: "removed".to_string(),
                    detail: "orphaned source tree (no registry record)".to_string(),
                }),
                Err(err) => out.push(SourceSyncOutcome {
                    id: name.clone(),
                    outcome: "failed".to_string(),
                    detail: err.to_string(),
                }),
            }
        }
    }
    out
}

/// A theme asset exposed by an installed, trusted source.
#[derive(Debug, Clone, Serialize)]
pub struct SourceThemeEntry {
    pub name: String,
    pub path: PathBuf,
    pub source_id: String,
    pub adapter_display: String,
}

/// Theme entries contributed by trusted, non-degraded sources (§11.3:
/// external-first layer between user TOML themes and bundle-native themes).
pub fn source_theme_entries() -> Vec<SourceThemeEntry> {
    let mut out = Vec::new();
    for status in list_sources() {
        if status.degraded || !status.record.trusted {
            continue;
        }
        let Some(adapter) = adapter_for(&status.record.adapter) else {
            continue;
        };
        for asset in adapter.list_assets(&status.record.path) {
            if asset.kind == SourceAssetKind::Theme {
                out.push(SourceThemeEntry {
                    name: asset.name,
                    path: asset.path,
                    source_id: status.record.id.clone(),
                    adapter_display: adapter.display_name().to_string(),
                });
            }
        }
    }
    out
}

/// Strip the `native:` override prefix (§11.3 rule 2): `native:<name>` skips
/// the external-source layer and reaches the built-in assets directly.
pub fn strip_native_override(name: &str) -> &str {
    name.strip_prefix("native:").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::PROCESS_STATE_LOCK;

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niu-sources-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Minimal oh-my-bash-shaped fixture (corpus layout:
    /// `D:/repo/rubash/target-ecosys/repos/oh-my-bash`).
    fn write_omb_fixture(root: &Path, marker: &str) {
        fs::create_dir_all(root.join("themes/robbyrussell")).unwrap();
        fs::create_dir_all(root.join("themes/agnoster")).unwrap();
        fs::create_dir_all(root.join("plugins/git")).unwrap();
        fs::create_dir_all(root.join("aliases")).unwrap();
        fs::create_dir_all(root.join("completions")).unwrap();
        fs::write(
            root.join("oh-my-bash.sh"),
            "#!/usr/bin/env bash\ncase $- in *i*) ;; *) return;; esac\n\
             _omb_util_print() { printf '%s\\n' \"$*\"; }\n",
        )
        .unwrap();
        fs::write(
            root.join("themes/robbyrussell/robbyrussell.theme.sh"),
            format!("# marker={marker}\nPS1='➜ %~ '\n"),
        )
        .unwrap();
        fs::write(
            root.join("themes/agnoster/agnoster.theme.sh"),
            format!("# marker={marker}\nPS1='%(?)%~# '\n"),
        )
        .unwrap();
        fs::write(
            root.join("plugins/git/git.plugin.sh"),
            format!("# marker={marker}\nalias gg='git status'\n"),
        )
        .unwrap();
        fs::write(
            root.join("aliases/cargo.aliases.sh"),
            "alias cb='cargo build'\n",
        )
        .unwrap();
        fs::write(
            root.join("completions/git.completion.sh"),
            format!("# marker={marker}\n_omb_completion_git_stub() {{ :; }}\n"),
        )
        .unwrap();
    }

    /// Minimal bash-it-shaped fixture (corpus layout:
    /// `D:/repo/rubash/target-ecosys/repos/bash-it`).
    fn write_bash_it_fixture(root: &Path, marker: &str) {
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("aliases/available")).unwrap();
        fs::create_dir_all(root.join("plugins/available")).unwrap();
        fs::create_dir_all(root.join("completion/available")).unwrap();
        fs::create_dir_all(root.join("themes/demox")).unwrap();
        fs::write(
            root.join("bash_it.sh"),
            "#!/usr/bin/env bash\ncite() { :; }\nabout-plugin() { :; }\n\
             for _f in \"$BASH_IT/enabled\"/*.bash; do\n  [ -r \"$_f\" ] && . \"$_f\"\ndone\nunset _f\n",
        )
        .unwrap();
        fs::write(root.join("lib/composure.bash"), "# composure stub\n").unwrap();
        fs::write(
            root.join("aliases/available/apt.aliases.bash"),
            format!("# marker={marker}\nalias apts='apt search'\n"),
        )
        .unwrap();
        fs::write(
            root.join("plugins/available/base.plugin.bash"),
            "# BASH_IT_LOAD_PRIORITY: 350\n_about 'base helpers'\n_base_fn() { :; }\n",
        )
        .unwrap();
        fs::write(
            root.join("completion/available/docker.completion.bash"),
            "# docker completion stub\n",
        )
        .unwrap();
        fs::write(
            root.join("themes/demox/demox.theme.bash"),
            "PS1='demox> '\n",
        )
        .unwrap();
    }

    /// Minimal bash-completion-shaped fixture (corpus layout:
    /// `D:/repo/rubash/target-ecosys/repos/bash-completion`).
    fn write_bash_completion_fixture(root: &Path) {
        fs::create_dir_all(root.join("completions")).unwrap();
        fs::write(
            root.join("bash_completion"),
            "# bash_completion stub\nBASH_COMPLETION_STUB=1\n",
        )
        .unwrap();
        fs::write(root.join("completions/git.bash"), "# git completion stub\n").unwrap();
    }

    fn local_request(origin: &Path) -> SourceInstallRequest {
        SourceInstallRequest {
            adapter: None,
            origin: origin.to_string_lossy().into_owned(),
            ref_name: None,
            commit: None,
            expected_checksum: None,
        }
    }

    #[test]
    fn oh_my_bash_adapter_detects_layout_and_enumerates_assets() {
        let temp = unique_temp_dir("adapter-detect");
        write_omb_fixture(&temp, "v1");
        let adapter = OhMyBashAdapter;
        assert!(adapter.detect(&temp));
        // A niubash bundle tree is not an oh-my-bash tree.
        let bundle = unique_temp_dir("adapter-detect-bundle");
        fs::write(bundle.join("bundle.toml"), "name = \"x\"\n").unwrap();
        assert!(!adapter.detect(&bundle));
        assert!(detect_source_adapter(&bundle).is_none());

        let assets = adapter.list_assets(&temp);
        let names: Vec<(String, String)> = assets
            .iter()
            .map(|a| (a.kind.as_str().to_string(), a.name.clone()))
            .collect();
        assert!(
            names.contains(&("theme".into(), "agnoster".into())),
            "{names:?}"
        );
        assert!(
            names.contains(&("theme".into(), "robbyrussell".into())),
            "{names:?}"
        );
        assert!(
            names.contains(&("plugin".into(), "git".into())),
            "{names:?}"
        );
        assert!(
            names.contains(&("alias".into(), "cargo".into())),
            "{names:?}"
        );
        let _ = fs::remove_dir_all(&temp);
        let _ = fs::remove_dir_all(&bundle);
    }

    #[test]
    fn loader_snippet_is_guarded_and_sets_omb_channel() {
        let record = SourceRecord {
            id: "oh-my-bash".to_string(),
            adapter: "oh-my-bash".to_string(),
            url: "unused".to_string(),
            ref_name: LOCAL_ORIGIN_REF.to_string(),
            version: "unknown".to_string(),
            path: PathBuf::from("/unused/oh-my-bash"),
            trusted: true,
            license: "MIT".to_string(),
            checksum_sha256: "0".to_string(),
            installed_at: String::new(),
            commit_sha: None,
            trust_policy: TrustPolicy::default(),
            signature: None,
            previous: None,
        };
        let snippet = OhMyBashAdapter.loader_snippet(&record);
        assert!(snippet.contains("if [ -r "), "{snippet}");
        assert!(snippet.contains("/oh-my-bash/oh-my-bash.sh"), "{snippet}");
        assert!(snippet.contains("export OSH"), "{snippet}");
        // Separator normalization keeps OMB's internal globs working when
        // NIU_PLUGIN_SOURCES_ROOT is in Windows form (the closed
        // `${var//\\//}` substitution, same form as the rc HOME bootstrap).
        assert!(snippet.contains(r"${OSH//\\//}"), "{snippet}");
        assert!(snippet.contains("export NIU_THEME_SOURCE=omb"), "{snippet}");
        assert!(snippet.contains(". \"$OSH/oh-my-bash.sh\""), "{snippet}");
        // The snippet must not force a theme default: plugins-only
        // activation keeps the niubash prompt, and theme picks come from
        // the managed block's OSH_THEME line.
        assert!(!snippet.contains("OSH_THEME"), "{snippet}");
        // The fallback note must be visible in the snippet.
        assert!(
            snippet.contains("fallback stays active when absent"),
            "{snippet}"
        );
    }

    #[test]
    fn add_trust_remove_round_trip_local_origin() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("add-round-trip");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);

        let record = add_source(local_request(&origin)).expect("add must succeed");
        assert_eq!(record.id, "oh-my-bash");
        assert!(!record.trusted, "new sources are untrusted");
        assert_eq!(record.license, "MIT");
        assert_eq!(record.ref_name, LOCAL_ORIGIN_REF);
        assert!(!record.checksum_sha256.is_empty());
        assert!(record.path.ends_with("oh-my-bash"));

        // Untrusted sources contribute no theme entries (execution gate).
        assert!(source_theme_entries().is_empty());

        let trusted = trust_source("oh-my-bash").expect("trust must succeed");
        assert!(trusted.trusted);
        let entries = source_theme_entries();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"robbyrussell"), "{names:?}");
        assert!(names.contains(&"agnoster"), "{names:?}");

        let statuses = list_sources();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, "ready");
        assert_eq!(statuses[0].asset_count, Some(5));

        let removed = remove_source("oh-my-bash").expect("remove must succeed");
        assert!(!removed.exists());
        assert!(read_source_registry().is_empty());
        assert!(source_theme_entries().is_empty());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn add_rejects_checksum_mismatch_and_records_nothing() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("add-checksum");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);

        let request = SourceInstallRequest {
            expected_checksum: Some("deadbeef".to_string()),
            ..local_request(&origin)
        };
        let err = add_source(request).expect_err("mismatch must fail");
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
        assert!(read_source_registry().is_empty());
        assert!(
            !root.join("oh-my-bash").exists(),
            "failed installs must not promote"
        );
        // No staging litter.
        let leftovers: Vec<_> = fs::read_dir(&root)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.file_name())
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            !leftovers
                .iter()
                .any(|n| n.to_string_lossy().starts_with(".staging-")),
            "staging must be removed on failure: {leftovers:?}"
        );
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn verify_detects_tampering() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("verify-tamper");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        let record = add_source(local_request(&origin)).unwrap();

        let ok = verify_source("oh-my-bash").expect("verify must run");
        assert!(ok.verified, "{ok:?}");

        fs::write(
            root.join("oh-my-bash/themes/robbyrussell/robbyrussell.theme.sh"),
            "PS1='tampered'\n",
        )
        .unwrap();
        let bad = verify_source("oh-my-bash").expect("verify must run");
        assert!(!bad.verified, "{bad:?}");
        assert_eq!(bad.recorded_checksum, record.checksum_sha256);

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn update_records_previous_and_rollback_restores_git_origin() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("update-rollback-git");
        let repo = temp.join("omb-repo");
        let root = temp.join("sources");
        // Two refs in one local git remote: v1 (install), v2 (update).
        write_omb_fixture(&repo, "v1");
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .expect("git must be available");
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["checkout", "-q", "-b", "v1"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "v1",
        ]);
        git(&["checkout", "-q", "-b", "v2"]);
        write_omb_fixture(&repo, "v2");
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "v2",
        ]);

        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        let request = |git_ref: &str| SourceInstallRequest {
            adapter: None,
            origin: repo.to_string_lossy().into_owned(),
            ref_name: Some(git_ref.to_string()),
            commit: None,
            expected_checksum: None,
        };
        let v1 = add_source(request("v1")).expect("install from git ref v1");
        trust_source("oh-my-bash").unwrap();

        let summary = update_source("oh-my-bash", request("v2")).expect("update to v2");
        assert_ne!(summary.checksum_sha256, v1.checksum_sha256);
        let previous = summary.previous.expect("previous recorded");
        assert_eq!(previous.ref_name, "v1");
        assert_eq!(previous.checksum_sha256, v1.checksum_sha256);
        // Same origin keeps trust (§12.4).
        assert!(read_source_registry()[0].trusted);

        let rolled = rollback_source("oh-my-bash").expect("rollback must succeed");
        assert_eq!(rolled.checksum_sha256, v1.checksum_sha256);
        assert!(read_source_registry()[0].previous.is_none());
        let verify = verify_source("oh-my-bash").unwrap();
        assert!(verify.verified, "{verify:?}");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn rollback_refuses_when_local_origin_drifted() {
        // §12.4: a local-path origin cannot rebuild the old tree; rollback
        // must fail the checksum and leave the current install intact.
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("rollback-drifted");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        add_source(local_request(&origin)).unwrap();
        trust_source("oh-my-bash").unwrap();

        write_omb_fixture(&origin, "v2");
        update_source("oh-my-bash", local_request(&origin)).unwrap();
        let err = rollback_source("oh-my-bash").expect_err("rollback must refuse");
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
        // The v2 install is untouched (still verifiable).
        assert!(verify_source("oh-my-bash").unwrap().verified);
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn update_with_changed_origin_resets_trust() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("update-origin");
        let origin_a = temp.join("origin-a");
        let origin_b = temp.join("origin-b");
        let root = temp.join("sources");
        write_omb_fixture(&origin_a, "a");
        write_omb_fixture(&origin_b, "b");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        add_source(local_request(&origin_a)).unwrap();
        trust_source("oh-my-bash").unwrap();

        let summary = update_source("oh-my-bash", local_request(&origin_b)).unwrap();
        assert_eq!(summary.id, "oh-my-bash");
        let record = &read_source_registry()[0];
        assert!(!record.trusted, "changed origin must reset trust");
        assert_eq!(record.url, origin_b.to_string_lossy());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn degraded_source_is_excluded_from_theme_entries() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("degraded");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        add_source(local_request(&origin)).unwrap();
        trust_source("oh-my-bash").unwrap();
        assert!(!source_theme_entries().is_empty());

        // Offline/degraded: tree disappears (disk cleanup, roaming profile).
        fs::remove_dir_all(root.join("oh-my-bash")).unwrap();
        let statuses = list_sources();
        assert_eq!(statuses[0].state, "degraded");
        assert_eq!(statuses[0].asset_count, None);
        assert!(
            source_theme_entries().is_empty(),
            "degraded sources must not contribute assets"
        );
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn tree_checksum_is_deterministic_and_ignores_git_dir() {
        let temp = unique_temp_dir("tree-sha");
        let tree_a = temp.join("a");
        let tree_b = temp.join("b");
        write_omb_fixture(&tree_a, "v1");
        write_omb_fixture(&tree_b, "v1");
        let digest_a = tree_sha256(&tree_a).unwrap();
        let digest_b = tree_sha256(&tree_b).unwrap();
        assert_eq!(digest_a, digest_b, "same content must hash identically");

        fs::create_dir_all(tree_a.join(".git")).unwrap();
        fs::write(tree_a.join(".git/HEAD"), "ref: refs/heads/master\n").unwrap();
        assert_eq!(
            tree_sha256(&tree_a).unwrap(),
            digest_a,
            ".git must not affect the tree checksum"
        );

        fs::write(tree_b.join("extra.txt"), "x").unwrap();
        assert_ne!(
            tree_sha256(&tree_b).unwrap(),
            digest_a,
            "content changes must change the checksum"
        );
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn native_override_prefix_is_stripped() {
        assert_eq!(strip_native_override("native:agnoster"), "agnoster");
        assert_eq!(strip_native_override("agnoster"), "agnoster");
    }

    #[test]
    fn duplicate_add_is_rejected() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("dup");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        add_source(local_request(&origin)).unwrap();
        let err = add_source(local_request(&origin)).expect_err("duplicate must fail");
        assert!(err.to_string().contains("already registered"), "{err}");
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn unknown_origin_without_manager_layout_fails_with_hint() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("unknown-layout");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        fs::create_dir_all(&origin).unwrap();
        fs::write(origin.join("random.txt"), "not a manager\n").unwrap();
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        let err = add_source(local_request(&origin)).expect_err("detect must fail");
        assert!(
            err.to_string().contains("no supported plugin manager"),
            "{err}"
        );
        assert!(err.to_string().contains("oh-my-bash"), "{err}");
        let _ = fs::remove_dir_all(&temp);
    }
    #[test]
    fn bash_it_adapter_detects_lists_and_loads_through_enabled_dir() {
        let temp = unique_temp_dir("bash-it-adapter");
        write_bash_it_fixture(&temp, "v1");
        let adapter = BashItAdapter;
        assert!(adapter.detect(&temp));
        assert!(!OhMyBashAdapter.detect(&temp));
        assert!(!BashCompletionAdapter.detect(&temp));

        let assets = adapter.list_assets(&temp);
        let names: Vec<(String, String)> = assets
            .iter()
            .map(|a| (a.kind.as_str().to_string(), a.name.clone()))
            .collect();
        assert!(names.contains(&("alias".into(), "apt".into())), "{names:?}");
        assert!(
            names.contains(&("plugin".into(), "base".into())),
            "{names:?}"
        );
        assert!(
            names.contains(&("completion".into(), "docker".into())),
            "{names:?}"
        );
        assert!(
            names.contains(&("theme".into(), "demox".into())),
            "{names:?}"
        );

        // Loader snippet keeps bash-it's own path: BASH_IT + guarded source
        // of bash_it.sh, and no forced theme.
        let record = SourceRecord {
            id: "bash-it".to_string(),
            adapter: "bash-it".to_string(),
            url: "unused".to_string(),
            ref_name: LOCAL_ORIGIN_REF.to_string(),
            version: "unknown".to_string(),
            path: temp.clone(),
            trusted: true,
            license: "MIT".to_string(),
            checksum_sha256: "0".to_string(),
            installed_at: String::new(),
            commit_sha: None,
            trust_policy: TrustPolicy::default(),
            signature: None,
            previous: None,
        };
        let snippet = adapter.loader_snippet(&record);
        assert!(
            snippet.contains("if [ -r \"$BASH_IT/bash_it.sh\" ]"),
            "{snippet}"
        );
        assert!(!snippet.contains("BASH_IT_THEME"), "{snippet}");

        // Selection model: enabled/ directory + BASH_IT_THEME.
        assert_eq!(
            adapter.selection_model(),
            SelectionModel::EnabledDir {
                subdirs: BASH_IT_SUBDIRS,
                theme_var: "BASH_IT_THEME",
            }
        );
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn bash_completion_adapter_detects_and_activates_whole_source() {
        let temp = unique_temp_dir("bash-completion-adapter");
        write_bash_completion_fixture(&temp);
        let adapter = BashCompletionAdapter;
        assert!(adapter.detect(&temp));
        assert!(!OhMyBashAdapter.detect(&temp));
        assert_eq!(adapter.license(), "GPL-2.0-or-later");
        let assets = adapter.list_assets(&temp);
        let names: Vec<(String, String)> = assets
            .iter()
            .map(|a| (a.kind.as_str().to_string(), a.name.clone()))
            .collect();
        assert!(
            names.contains(&("completion".into(), "git".into())),
            "{names:?}"
        );

        let record = SourceRecord {
            id: "bash-completion".to_string(),
            adapter: "bash-completion".to_string(),
            url: "unused".to_string(),
            ref_name: LOCAL_ORIGIN_REF.to_string(),
            version: "unknown".to_string(),
            path: temp.clone(),
            trusted: true,
            license: "GPL-2.0-or-later".to_string(),
            checksum_sha256: "0".to_string(),
            installed_at: String::new(),
            commit_sha: None,
            trust_policy: TrustPolicy::default(),
            signature: None,
            previous: None,
        };
        let snippet = adapter.loader_snippet(&record);
        assert!(snippet.contains("if [ -r "), "{snippet}");
        assert!(snippet.contains("/bash_completion\" ]"), "{snippet}");
        assert!(
            snippet.contains("native completions stay as fallback"),
            "{snippet}"
        );
        assert_eq!(adapter.selection_model(), SelectionModel::WholeSource);
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn trust_refuses_tampered_tree_and_reports_checksums() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("trust-tamper");
        let origin = temp.join("origin");
        let root = temp.join("sources");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        add_source(local_request(&origin)).unwrap();

        // Tamper before trusting: the execution gate must refuse.
        fs::write(
            root.join("oh-my-bash/plugins/git/git.plugin.sh"),
            "alias evil='rm -rf /'\n",
        )
        .unwrap();
        let err = trust_source("oh-my-bash").expect_err("tampered tree must refuse trust");
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
        assert!(!read_source_registry()[0].trusted);
        assert!(source_theme_entries().is_empty());

        // Restore the pristine tree by hand, then trust works.
        fs::write(
            root.join("oh-my-bash/plugins/git/git.plugin.sh"),
            "# marker=v1\nalias gg='git status'\n",
        )
        .unwrap();
        assert!(trust_source("oh-my-bash").unwrap().trusted);
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn github_shorthand_expands_only_for_owner_repo() {
        assert_eq!(
            normalize_origin("ohmybash/oh-my-bash"),
            "https://github.com/ohmybash/oh-my-bash.git"
        );
        // URLs, paths, and multi-segment targets pass through.
        assert_eq!(
            normalize_origin("https://example.com/x.git"),
            "https://example.com/x.git"
        );
        assert_eq!(normalize_origin("./local"), "./local");
        assert_eq!(
            normalize_origin("git@github.com:o/r.git"),
            "git@github.com:o/r.git"
        );
        assert_eq!(normalize_origin("a/b/c"), "a/b/c");
        // An existing directory wins over the shorthand reading.
        let temp = unique_temp_dir("shorthand-dir");
        let dir = temp.join("owner");
        fs::create_dir_all(dir.join("repo")).unwrap();
        assert_eq!(
            normalize_origin(&dir.join("repo").to_string_lossy()),
            dir.join("repo").to_string_lossy().into_owned()
        );
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn sign_source_upgrades_policy_and_update_re_gates() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("sign-update");
        let repo = temp.join("omb-repo");
        let root = temp.join("sources");
        write_omb_fixture(&repo, "v1");
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .expect("git must be available");
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["checkout", "-q", "-b", "v1"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "v1",
        ]);
        git(&["checkout", "-q", "-b", "v2"]);
        write_omb_fixture(&repo, "v2");
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "v2",
        ]);

        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        let request = |git_ref: &str| SourceInstallRequest {
            adapter: None,
            origin: repo.to_string_lossy().into_owned(),
            ref_name: Some(git_ref.to_string()),
            commit: None,
            expected_checksum: None,
        };
        let v1 = add_source(request("v1")).unwrap();
        assert!(v1.commit_sha.is_some(), "git installs pin the commit");

        // Local signature tier: sign implies trust.
        let signed = sign_source("oh-my-bash").unwrap();
        assert!(signed.trusted);
        assert_eq!(signed.trust_policy, TrustPolicy::LocalSign);
        assert!(signed.signature.is_some(), "signature recorded");
        let verify = verify_source("oh-my-bash").unwrap();
        assert!(verify.verified, "{verify:?}");
        assert_eq!(verify.signature_ok, Some(true), "{verify:?}");

        // Any changed tree re-enters the execution gate until re-signed.
        update_source("oh-my-bash", request("v2")).unwrap();
        let record = &read_source_registry()[0];
        assert!(!record.trusted, "signed sources re-gate on update");
        let verify = verify_source("oh-my-bash").unwrap();
        assert_eq!(verify.signature_ok, Some(false), "{verify:?}");
        assert!(
            source_theme_entries().is_empty(),
            "re-gated sources contribute nothing"
        );

        // Re-signing the reviewed v2 tree restores trust.
        sign_source("oh-my-bash").unwrap();
        assert!(read_source_registry()[0].trusted);
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn restore_source_rebuilds_the_pinned_commit() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("restore-pin");
        let repo = temp.join("omb-repo");
        let root = temp.join("sources");
        write_omb_fixture(&repo, "v1");
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .expect("git must be available");
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q"]);
        git(&["checkout", "-q", "-b", "v1"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "v1",
        ]);
        git(&["checkout", "-q", "-b", "v2"]);
        write_omb_fixture(&repo, "v2");
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "v2",
        ]);

        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);
        let request = |git_ref: &str| SourceInstallRequest {
            adapter: None,
            origin: repo.to_string_lossy().into_owned(),
            ref_name: Some(git_ref.to_string()),
            commit: None,
            expected_checksum: None,
        };
        let v1 = add_source(request("v1")).unwrap();
        trust_source("oh-my-bash").unwrap();
        let summary = update_source("oh-my-bash", request("v2")).unwrap();

        // `restore` is lockfile repair, not rollback (lazy.nvim :Lazy
        // restore): it rebuilds the *currently locked* state — here v2 —
        // from the pinned commit. Tamper first so repair is observable.
        fs::write(
            root.join("oh-my-bash/themes/agnoster/agnoster.theme.sh"),
            "PS1='tampered'\n",
        )
        .unwrap();
        assert!(!verify_source("oh-my-bash").unwrap().verified);

        let outcome = restore_source("oh-my-bash").expect("restore must succeed");
        assert_eq!(outcome.outcome, "restored");
        let record = &read_source_registry()[0];
        assert!(record.trusted, "same-lock restore keeps trust");
        let verify = verify_source("oh-my-bash").unwrap();
        assert!(verify.verified, "{verify:?}");
        assert_eq!(record.checksum_sha256, summary.checksum_sha256);
        assert!(
            record.commit_sha.is_some(),
            "restored record keeps the commit pin"
        );
        assert_ne!(
            record.checksum_sha256, v1.checksum_sha256,
            "restore repairs the lock (v2), it does not roll back to v1"
        );
        // Rebuilding a degraded (deleted) tree works the same way.
        fs::remove_dir_all(root.join("oh-my-bash")).unwrap();
        restore_source("oh-my-bash").expect("degraded restore must succeed");
        assert!(verify_source("oh-my-bash").unwrap().verified);

        // Local snapshots have no upstream to restore from.
        let local_temp = unique_temp_dir("restore-local");
        let local_origin = local_temp.join("origin");
        let local_root = local_temp.join("sources");
        write_omb_fixture(&local_origin, "v1");
        let _local_guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &local_root);
        add_source(local_request(&local_origin)).unwrap();
        let err = restore_source("oh-my-bash").expect_err("local snapshot restore must fail");
        assert!(
            err.to_string().contains("local directory snapshot"),
            "{err}"
        );
        let _ = fs::remove_dir_all(&temp);
        let _ = fs::remove_dir_all(&local_temp);
    }

    #[test]
    fn clean_removes_staging_leftovers_and_orphan_trees() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("clean");
        let root = temp.join("sources");
        let origin = temp.join("origin");
        write_omb_fixture(&origin, "v1");
        let _guard = EnvVarGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root);

        // A registered install stays; an orphan OMB-shaped tree and a
        // staging leftover go; a foreign directory is untouched.
        add_source(local_request(&origin)).unwrap();
        let orphan = root.join("orphan-omb");
        write_omb_fixture(&orphan, "stale");
        fs::create_dir_all(root.join(".staging-123-456")).unwrap();
        fs::create_dir_all(root.join("my-notes")).unwrap();
        fs::write(root.join("my-notes/keep.txt"), "user data\n").unwrap();

        let removed = clean_sources();
        let ids: Vec<(&str, &str)> = removed
            .iter()
            .map(|row| (row.id.as_str(), row.outcome.as_str()))
            .collect();
        assert!(ids.contains(&(".staging-123-456", "removed")), "{ids:?}");
        assert!(ids.contains(&("orphan-omb", "removed")), "{ids:?}");
        assert!(root.join("oh-my-bash").is_dir(), "registered tree stays");
        assert!(
            root.join("my-notes/keep.txt").is_file(),
            "foreign dirs stay"
        );
        let _ = fs::remove_dir_all(&temp);
    }
}
