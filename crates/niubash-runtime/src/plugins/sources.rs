//! External plugin-manager sources (oh-my-bash, bash-it, bpkg) as first-class
//! plugin origins.
//!
//! Design: `docs/planning/oh-my-niu-ecosystem.md` §11 (source adapters) and
//! §12 (trust + download protocol). A *source* is a plugin manager's native
//! tree (no `bundle.toml`); it installs under `~/.niubash/sources/<id>/`,
//! registers untrusted in `~/.niubash/sources/registry.toml`, and only
//! contributes assets after an explicit `niu plugin source trust <id>`.
//! Built-in packs stay as the fallback layer (§0: external primary, native
//! fallback; `native:<name>` reaches the built-in explicitly).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::path_utils::shell_home_dir;

/// Schema marker for the source registry file.
pub const SOURCE_REGISTRY_SCHEMA: &str = "niubash:plugin-source-registry@0.1.0";
/// Ref recorded for local-directory installs (no git ref exists).
pub const LOCAL_ORIGIN_REF: &str = "local";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAssetKind {
    Theme,
    Plugin,
    Alias,
}

impl SourceAssetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Plugin => "plugin",
            Self::Alias => "alias",
        }
    }
}

/// One loadable asset inside a source tree (a theme script, a plugin, an
/// alias bundle). Names are flat so they can collide-resolve against
/// built-in packs per §11.3 (external wins, `native:` escapes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAsset {
    pub kind: SourceAssetKind,
    pub name: String,
    pub path: PathBuf,
}

/// Rollback state recorded by `update_source` (§12.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePreviousState {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub version: String,
    pub checksum_sha256: String,
}

/// One registered external source. Lives in `~/.niubash/sources/registry.toml`.
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

/// Result of `verify_source` (§12.3 checksum re-computation).
#[derive(Debug, Clone, Serialize)]
pub struct SourceVerifyReport {
    pub id: String,
    pub verified: bool,
    pub degraded: bool,
    pub recorded_checksum: String,
    pub actual_checksum: Option<String>,
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
/// manager-specific surface (detect / version / list / loader); the generic
/// install/update/uninstall protocol in this module is shared by all
/// adapters.
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
    /// Layout fingerprint: does this tree belong to this plugin manager?
    fn detect(&self, root: &Path) -> bool;
    /// Human-readable version of the installed tree (git HEAD, marker file).
    fn installed_version(&self, root: &Path) -> String;
    /// Enumerate loadable assets (themes, plugins, aliases).
    fn list_assets(&self, root: &Path) -> Vec<SourceAsset>;
    /// Guarded rc snippet that activates the source. Must keep the
    /// existence guard so a missing tree silently falls back to the native
    /// layers (§11.4).
    fn loader_snippet(&self, record: &SourceRecord) -> String;
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
        assets.sort_by(|left, right| {
            (left.kind.as_str(), &left.name).cmp(&(right.kind.as_str(), &right.name))
        });
        assets
    }
    fn loader_snippet(&self, record: &SourceRecord) -> String {
        // §3.3 + §11.4: keep oh-my-bash's own loading path (source
        // oh-my-bash.sh after exporting OSH/OSH_THEME), guard on readability
        // so a missing tree falls back to native themes, and mark the theme
        // channel for the bash-compatible PS1 renderer.
        let base = format!(
            "${{NIU_PLUGIN_SOURCES_ROOT:-$HOME/.niubash/sources}}/{}",
            record.id
        );
        format!(
            "# {id} source (external, primary; native themes stay as fallback)\n\
             if [ -r \"{base}/oh-my-bash.sh\" ]; then\n\
             \x20 export OSH=\"{base}\"\n\
             \x20 export OSH_THEME=\"${{OSH_THEME:-robbyrussell}}\"\n\
             \x20 export NIU_THEME_SOURCE=omb\n\
             \x20 . \"$OSH/oh-my-bash.sh\"\n\
             fi\n",
            id = record.id,
            base = base,
        )
    }
}

/// Best-effort `<sha>` (12 hex chars) from a `.git` directory, shallow or
/// not. Used only for display/versioning; the trust anchor is the tree
/// checksum, not this.
fn git_head_short_sha(root: &Path) -> Option<String> {
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
    let short: String = sha.chars().take(12).collect();
    (!short.is_empty()).then(|| format!("git-{short}"))
}

/// Adapters compiled into this build. bash-it / bpkg land in WP-S2/S3.
pub fn builtin_source_adapters() -> &'static [&'static dyn PluginSourceAdapter] {
    &[&OhMyBashAdapter]
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
    /// Expected tree checksum (hex sha256). Verified on staging before
    /// promotion; mismatch aborts and leaves any existing install intact.
    pub expected_checksum: Option<String>,
}

struct FetchedSource {
    staging: PathBuf,
    adapter: &'static dyn PluginSourceAdapter,
    version: String,
    checksum_sha256: String,
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
        // Origin semantics: an explicit ref forces git semantics (a local
        // git repository path is also a directory and must clone, not
        // snapshot — the working tree may be ahead of the requested ref).
        // Without a ref, directories snapshot and everything else clones.
        let explicit_ref = request
            .ref_name
            .clone()
            .filter(|value| value != LOCAL_ORIGIN_REF);
        let ref_name = match &explicit_ref {
            Some(value) => value.clone(),
            None if origin_path.is_dir() => LOCAL_ORIGIN_REF.to_string(),
            None => "HEAD".to_string(),
        };
        if ref_name == LOCAL_ORIGIN_REF {
            copy_tree(origin_path, &staging)?;
        } else {
            let mut command = Command::new("git");
            // OMB has no .gitattributes; CRLF would kill sourcing (§9).
            command
                .arg("-c")
                .arg("core.autocrlf=false")
                .arg("clone")
                .arg("--depth")
                .arg("1");
            if ref_name != "HEAD" {
                command.arg("--branch").arg(&ref_name);
            }
            command.arg(origin).arg(&staging);
            let status = command
                .status()
                .with_context(|| "failed to run git; is git.exe on PATH?")?;
            if !status.success() {
                anyhow::bail!(
                    "git clone exited with status {}",
                    status.code().unwrap_or(1)
                );
            }
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
        Ok(FetchedSource {
            staging,
            adapter,
            version,
            checksum_sha256,
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
    };
    fs::remove_dir_all(&record.path)
        .with_context(|| format!("failed to remove old tree {}", record.path.display()))?;
    promote_staging(&fetched.staging, &record.path)?;

    let updated = SourceRecord {
        id: record.id.clone(),
        adapter: record.adapter.clone(),
        url: request.origin.trim().to_string(),
        ref_name: request.ref_name.clone().unwrap_or(record.ref_name.clone()),
        version: fetched.version,
        path: record.path.clone(),
        trusted: record.trusted && !origin_changed,
        license: fetched.adapter.license().to_string(),
        checksum_sha256: fetched.checksum_sha256,
        installed_at: now_timestamp(),
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

    let ref_name = if previous.ref_name == LOCAL_ORIGIN_REF {
        None
    } else {
        Some(previous.ref_name.clone())
    };
    let fetched = fetch_source_to_staging(&SourceInstallRequest {
        adapter: Some(record.adapter.clone()),
        origin: record.url.clone(),
        ref_name,
        expected_checksum: Some(previous.checksum_sha256.clone()),
    })?;

    fs::remove_dir_all(&record.path)
        .with_context(|| format!("failed to remove tree {}", record.path.display()))?;
    promote_staging(&fetched.staging, &record.path)?;

    let restored = SourceRecord {
        trusted: record.trusted,
        ref_name: previous.ref_name.clone(),
        version: previous.version.clone(),
        checksum_sha256: previous.checksum_sha256.clone(),
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
/// the catalog/loader (§12.2).
pub fn trust_source(id: &str) -> anyhow::Result<SourceRecord> {
    let mut registry = read_source_registry();
    let record = registry
        .iter_mut()
        .find(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    record.trusted = true;
    let trusted = record.clone();
    write_source_registry(&registry)?;
    Ok(trusted)
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

/// Re-compute the tree checksum and compare with the registry record
/// (§12.3).
pub fn verify_source(id: &str) -> anyhow::Result<SourceVerifyReport> {
    let record = read_source_registry()
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| anyhow!("unknown source '{id}'"))?;
    if !record.path.is_dir() {
        return Ok(SourceVerifyReport {
            id: record.id,
            verified: false,
            degraded: true,
            recorded_checksum: record.checksum_sha256,
            actual_checksum: None,
        });
    }
    let actual = tree_sha256(&record.path)?;
    Ok(SourceVerifyReport {
        id: record.id,
        verified: actual.eq_ignore_ascii_case(&record.checksum_sha256),
        degraded: false,
        recorded_checksum: record.checksum_sha256,
        actual_checksum: Some(actual),
    })
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
    }

    fn local_request(origin: &Path) -> SourceInstallRequest {
        SourceInstallRequest {
            adapter: None,
            origin: origin.to_string_lossy().into_owned(),
            ref_name: None,
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
            previous: None,
        };
        let snippet = OhMyBashAdapter.loader_snippet(&record);
        assert!(snippet.contains("if [ -r "), "{snippet}");
        assert!(snippet.contains("/oh-my-bash/oh-my-bash.sh"), "{snippet}");
        assert!(snippet.contains("export OSH="), "{snippet}");
        assert!(snippet.contains("export OSH_THEME="), "{snippet}");
        assert!(snippet.contains("export NIU_THEME_SOURCE=omb"), "{snippet}");
        assert!(snippet.contains(". \"$OSH/oh-my-bash.sh\""), "{snippet}");
        // Native fallback note must be visible in the snippet.
        assert!(
            snippet.contains("native themes stay as fallback"),
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
        assert_eq!(statuses[0].asset_count, Some(4));

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
}
