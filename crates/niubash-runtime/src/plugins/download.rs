//! Independent executable-asset download driver (owner ruling 2026-10-03).
//!
//! The plugin system never shells out to an external package manager for
//! executable installs: transport is pure Rust (`ureq` over rustls), unpack
//! is `zip` / `flate2`+`tar`, integrity is `sha2` — all in-tree crates, all
//! platforms. This mirrors mason's `generic`/`github` download compilers in
//! *shape* (see `docs/planning/lazy-family-source-study.md` §6) but is an
//! independent implementation with a two-driver world: git tree sources
//! (`plugins::sources`) and direct binary downloads (this module).
//!
//! Install protocol (mason's staging discipline, simplified):
//!   1. download to `<tools>/.staging/<id>.download`
//!   2. verify sha256 when the recipe pins one (warn when it does not —
//!      the trust protocol §12.3 still records the post-install digest)
//!   3. unpack to `<tools>/.staging/<id>/` (path-traversal guarded by the
//!      zip/tar crates' `enclosed_name` handling)
//!   4. rename into `<tools>/<id>/` and record `<tools>/registry.toml`
//!
//! PATH exposure is mason-style policy as data: each recipe names the
//! binaries it ships (`bin`), activation writes one managed rc block that
//! prepends the tool directory to PATH (`niu plugin enable <id>`).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::path_utils::shell_home_dir;

/// Schema marker for the executable-tool registry.
pub const TOOL_REGISTRY_SCHEMA: &str = "niubash:plugin-tool-registry@1.0.0";

/// Hard cap on a single asset download (mason has no cap; we do — a
/// corrupted content-length should never fill the disk).
const MAX_DOWNLOAD_BYTES: usize = 512 * 1024 * 1024;
/// Per-request timeout (transport + read combined).
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
/// Declared client, as GitHub asks of API/download clients.
const USER_AGENT: &str = concat!("niubash-plugin-driver/", env!("CARGO_PKG_VERSION"));

/// Archive container of a downloadable asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveKind {
    Zip,
    TarGzip,
}

impl ArchiveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGzip => "tar-gzip",
        }
    }
}

/// One platform-resolved downloadable artifact (recipe data, mason
/// `source.asset` row shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadAsset {
    pub url: String,
    pub archive: ArchiveKind,
    /// Pinned digest of the exact bytes; `None` means "not pinned" (the
    /// install proceeds but prints an unpinned-checksum warning).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Executable paths inside the archive to expose on PATH (forward or
    /// back slashes; usually one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<String>,
}

/// Registered executable tool record (`~/.niubash/tools/registry.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecord {
    pub id: String,
    pub version: String,
    pub url: String,
    /// Digest of the downloaded archive (always recorded, pinned or not —
    /// the §12.3 hash-lock equivalent for executables).
    pub archive_sha256: String,
    /// Whether the recipe pinned this digest up front.
    pub pinned: bool,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<String>,
    pub installed_at: String,
}

/// Platform key matching the recipe asset table (mason `target` naming,
/// dash-joined os-arch). Unknown combinations return a synthetic key that
/// matches no recipe, and `resolve_platform_asset` then errors naming the
/// platform — a clear failure, not a wrong-platform download.
pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("macos", "aarch64") => "darwin-arm64",
        _ => "unknown-platform",
    }
}

/// `~/.niubash/tools` — sibling of `~/.niubash/sources`.
pub fn tools_root() -> PathBuf {
    if let Some(value) = std::env::var_os("NIU_PLUGIN_TOOLS_ROOT") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    shell_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".niubash")
        .join("tools")
}

pub fn tool_registry_path() -> PathBuf {
    tools_root().join("registry.toml")
}

pub fn read_tool_registry() -> Vec<ToolRecord> {
    let Ok(text) = fs::read_to_string(tool_registry_path()) else {
        return Vec::new();
    };
    #[derive(Deserialize)]
    struct RegistryFile {
        /// Present for forward compatibility; unread on purpose (schema
        /// mismatches degrade to "unknown tools ignored", never a crash).
        #[serde(default, skip_serializing)]
        #[allow(dead_code)]
        schema: Option<String>,
        #[serde(default)]
        tools: Vec<ToolRecord>,
    }
    toml::from_str::<RegistryFile>(&text)
        .map(|file| file.tools)
        .unwrap_or_default()
}

fn write_tool_registry(tools: &[ToolRecord]) -> anyhow::Result<()> {
    let root = tools_root();
    fs::create_dir_all(&root)?;
    #[derive(Serialize)]
    struct RegistryFile<'a> {
        schema: &'a str,
        tools: &'a [ToolRecord],
    }
    let text = toml::to_string_pretty(&RegistryFile {
        schema: TOOL_REGISTRY_SCHEMA,
        tools,
    })?;
    fs::write(tool_registry_path(), text)?;
    Ok(())
}

/// Pure-Rust HTTP GET over rustls. Follows redirects (GitHub release
/// downloads are 302 to the CDN), enforces the size cap and timeout.
/// Mirror rewriting (§14.8) happens here, at the transport layer: the
/// caller's URL stays the canonical origin (tool records keep GitHub URLs
/// so registries stay portable); only the actual request is redirected
/// through the active mirror.
pub fn http_get_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let url = super::mirrors::rewrite_download_url(url);
    let response = ureq::get(&url)
        .timeout(HTTP_TIMEOUT)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| anyhow!("download failed for {url}: {err}"))?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut chunk)
            .with_context(|| format!("reading {url}"))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAX_DOWNLOAD_BYTES {
            bail!("asset exceeds the {MAX_DOWNLOAD_BYTES} byte cap: {url}");
        }
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Unpack an archive into `dest` (zip via the `zip` crate's traversal-safe
/// `extract`; tar.gz via `flate2`+`tar` `unpack`, both of which reject
/// entries escaping the destination).
fn unpack_archive(kind: ArchiveKind, bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dest)?;
    match kind {
        ArchiveKind::Zip => {
            let cursor = std::io::Cursor::new(bytes);
            let mut archive = zip::ZipArchive::new(cursor).context("opening zip archive")?;
            archive.extract(dest).context("extracting zip archive")?;
        }
        ArchiveKind::TarGzip => {
            let decoder = flate2::read::GzDecoder::new(bytes);
            let mut archive = tar::Archive::new(decoder);
            archive.unpack(dest).context("extracting tar.gz archive")?;
        }
    }
    Ok(())
}

/// Outcome of an executable install.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInstall {
    pub record: ToolRecord,
    /// Executable paths that exist on disk after unpack (joined with the
    /// tool dir; what the PATH block will expose).
    pub bin_paths: Vec<PathBuf>,
    pub pinned_checksum: bool,
}

/// Resolve one declared bin to the file actually present in the unpacked
/// tree. Recipes declare bins without a platform extension (mason shape:
/// `bins = ["fzf"]`), but Windows archives ship `fzf.exe`; the driver
/// accepts either, preferring the exact name, and returns the on-disk
/// relative path so the registry records reality (wt49/smokesweep: found
/// by the 1.3.0 smoke on `niu plugin recipe add fzf`, which refused the
/// verified archive because `fzf` was declared but `fzf.exe` shipped).
fn resolve_bin(stage_dir: &Path, bin: &str) -> Option<String> {
    let rel = bin.replace('\\', "/");
    let direct = stage_dir.join(&rel);
    if direct.is_file() {
        return Some(rel);
    }
    if cfg!(windows) {
        let exe = format!("{rel}.exe");
        if stage_dir.join(&exe).is_file() {
            return Some(exe);
        }
    }
    None
}

/// Unpack `bytes` into `stage_dir`, then verify every declared bin resolves
/// to a file in the tree; returns the resolved relative paths. Any failure
/// must leave the caller's staging clean (handled by the caller).
fn unpack_and_verify_bins(
    asset: &DownloadAsset,
    bytes: &[u8],
    stage_dir: &Path,
    id: &str,
) -> anyhow::Result<Vec<PathBuf>> {
    unpack_archive(asset.archive, bytes, stage_dir)?;
    let mut bin_paths = Vec::new();
    for bin in &asset.bins {
        match resolve_bin(stage_dir, bin) {
            Some(rel) => bin_paths.push(PathBuf::from(rel)),
            None => bail!("recipe declares bin '{bin}' for {id} but it is not in the archive"),
        }
    }
    Ok(bin_paths)
}

/// Install (or reinstall over) one executable tool. `bins` are paths inside
/// the archive; missing ones are an error (mason fails the install when the
/// declared bin is absent — same contract here).
pub fn install_executable(
    id: &str,
    version: &str,
    asset: &DownloadAsset,
) -> anyhow::Result<ToolInstall> {
    let bytes = http_get_bytes(&asset.url)?;
    commit_install(id, version, asset, &bytes)
}

/// The install protocol after transport: checksum gate, staging unpack,
/// bin verification, atomic commit, registry write. Split from
/// [`install_executable`] so the staging discipline is testable without
/// the network.
fn commit_install(
    id: &str,
    version: &str,
    asset: &DownloadAsset,
    bytes: &[u8],
) -> anyhow::Result<ToolInstall> {
    let root = tools_root();
    let staging = root.join(".staging");
    fs::create_dir_all(&staging)?;

    let actual = sha256_hex(bytes);
    let pinned = match &asset.sha256 {
        Some(pin) => {
            if !actual.eq_ignore_ascii_case(pin) {
                bail!(
                    "checksum mismatch for {id}: recipe pins {pin}, downloaded {actual}; \
                     refusing to install"
                );
            }
            true
        }
        None => false,
    };

    let stage_dir = staging.join(format!("{id}.unpacked"));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)?;
    }
    // Unpack and verify the declared bins as one fallible step: a failed
    // install (bad archive, missing bin) leaves no staging residue, so the
    // next attempt — or `tool list` — never observes half-installed state.
    let bin_paths = unpack_and_verify_bins(asset, bytes, &stage_dir, id).inspect_err(|_| {
        let _ = fs::remove_dir_all(&stage_dir);
    })?;

    let dest = root.join(id);
    if dest.exists() {
        fs::remove_dir_all(&dest).context("removing previous install")?;
    }
    fs::rename(&stage_dir, &dest).context("committing tool directory")?;
    let _ = fs::remove_file(staging.join(format!("{id}.download")));

    let record = ToolRecord {
        id: id.to_string(),
        version: version.to_string(),
        url: asset.url.clone(),
        archive_sha256: actual,
        pinned,
        path: dest.clone(),
        // The resolved on-disk names (`fzf.exe` on Windows for a declared
        // `fzf`), so the registry and `tool list` show what actually shipped.
        bins: bin_paths
            .iter()
            .map(|rel| rel.to_string_lossy().into_owned())
            .collect(),
        installed_at: now_timestamp(),
    };
    let mut tools = read_tool_registry();
    tools.retain(|existing| existing.id != id);
    tools.push(record.clone());
    tools.sort_by(|left, right| left.id.cmp(&right.id));
    write_tool_registry(&tools)?;

    Ok(ToolInstall {
        record,
        bin_paths,
        pinned_checksum: pinned,
    })
}

/// Remove a tool directory and its registry record. Returns false when the
/// tool was never installed.
pub fn uninstall_executable(id: &str) -> anyhow::Result<bool> {
    let mut tools = read_tool_registry();
    let Some(index) = tools.iter().position(|tool| tool.id == id) else {
        return Ok(false);
    };
    let record = tools.remove(index);
    if record.path.is_dir() {
        fs::remove_dir_all(&record.path)?;
    }
    write_tool_registry(&tools)?;
    Ok(true)
}

/// Remove a tool completely: the rc PATH block (when it was enabled), the
/// installed directory, and the registry record. Returns false when the
/// tool was never installed. This is the undo verb for download recipes
/// and collection applies.
pub fn remove_tool(id: &str) -> anyhow::Result<bool> {
    let _ = super::assets::remove_tool_path_block(id)?;
    uninstall_executable(id)
}

fn now_timestamp() -> String {
    // Keep the timestamp locale-free and filesystem-safe (same shape the
    // sources registry uses).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

/// Write (or replace) the archive bytes to a caller-supplied path — used by
/// tests and future streaming installs.
pub fn write_download(bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(dest)?;
    file.write_all(bytes)?;
    Ok(())
}

/// Pick the asset for the current platform out of a recipe's platform
/// table; the error names the platform and the available keys (health-style
/// "name the object and the repair", study §10.1).
pub fn resolve_platform_asset(
    assets: &BTreeMap<String, DownloadAsset>,
) -> anyhow::Result<&DownloadAsset> {
    let key = platform_key();
    assets.get(key).ok_or_else(|| {
        anyhow!(
            "no download asset for platform '{key}' (recipe provides: {})",
            assets.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both env-override tests below mutate the process-global
    /// `NIU_PLUGIN_TOOLS_ROOT`; cargo runs tests in parallel, so they take
    /// this lock to stay serialized.
    static TOOLS_ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn zip_bytes(names_and_contents: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, contents) in names_and_contents {
                writer.start_file(*name, options).unwrap();
                std::io::Write::write_all(&mut writer, contents).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn tar_gz_bytes(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, name, std::io::Cursor::new(contents.to_vec()))
            .unwrap();
        let raw = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn zip_unpack_extracts_declared_bins() {
        let bytes = zip_bytes(&[("bin/tool.exe", b"MZfake"), ("README.md", b"hi")]);
        let dest = tempfile::tempdir().unwrap();
        unpack_archive(ArchiveKind::Zip, &bytes, dest.path()).unwrap();
        assert!(dest.path().join("bin").join("tool.exe").is_file());
    }

    /// wt49/smokesweep regression: recipes declare extension-less bins
    /// (`bins = ["fzf"]`); Windows archives ship `fzf.exe`. The install
    /// protocol must resolve the `.exe` (Windows only), record the resolved
    /// name in the registry, and leave no staging residue when a declared
    /// bin is genuinely missing (found by the 1.3.0 smoke on
    /// `niu plugin recipe add fzf`, which refused the verified archive
    /// because `fzf` was declared but `fzf.exe` shipped).
    #[test]
    fn extensionless_bin_resolves_windows_exe_and_cleans_staging() {
        let _guard = TOOLS_ROOT_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("NIU_PLUGIN_TOOLS_ROOT", root.path());
        let staging = root.path().join(".staging");

        // Windows archive shape: fzf.exe at the root, bin declared as "fzf".
        let payload: &[(&str, &[u8])] = if cfg!(windows) {
            &[("fzf.exe", b"MZfake"), ("LICENSE", b"mit")]
        } else {
            &[("fzf", b"#!/bin/sh\n"), ("LICENSE", b"mit")]
        };
        let bytes = zip_bytes(payload);

        // Failure leg: a declared bin present in no form. The install
        // refuses AND removes its staging unpack dir (no residue).
        let missing = DownloadAsset {
            url: "https://example.invalid/missing.zip".into(),
            archive: ArchiveKind::Zip,
            sha256: None,
            bins: vec!["nowhere".into()],
        };
        let err = commit_install("m", "1.0.0", &missing, &bytes)
            .map(|install| install.record.bins)
            .unwrap_err()
            .to_string();
        assert!(err.contains("declares bin 'nowhere'"), "{err}");
        assert!(
            !staging.join("m.unpacked").exists(),
            "failed install must not leave staging residue"
        );

        // Success leg: the extensionless declaration resolves to the
        // platform's on-disk name, and the registry records it.
        let asset = DownloadAsset {
            url: "https://example.invalid/fzf.zip".into(),
            archive: ArchiveKind::Zip,
            sha256: None,
            bins: vec!["fzf".into()],
        };
        let install = commit_install("f", "1.0.0", &asset, &bytes).unwrap();
        let expected = if cfg!(windows) { "fzf.exe" } else { "fzf" };
        assert_eq!(install.record.bins, vec![expected.to_string()]);
        assert!(install.record.path.join(expected).is_file());
        assert!(read_tool_registry()
            .iter()
            .any(|tool| tool.id == "f" && tool.bins == vec![expected.to_string()]));

        std::env::remove_var("NIU_PLUGIN_TOOLS_ROOT");
    }

    #[test]
    fn tar_gz_unpack_extracts_declared_bins() {
        let bytes = tar_gz_bytes("tool", b"#!/bin/sh\n");
        let dest = tempfile::tempdir().unwrap();
        unpack_archive(ArchiveKind::TarGzip, &bytes, dest.path()).unwrap();
        assert!(dest.path().join("tool").is_file());
    }

    #[test]
    fn sha256_hex_is_stable_lower_hex() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn platform_asset_resolution_names_missing_platform() {
        let mut assets = BTreeMap::new();
        // A key that is a valid platform but never the host running the
        // test suite (windows tests run on windows-x64, unix on linux-*).
        let foreign = if std::env::consts::OS == "windows" {
            "darwin-arm64"
        } else {
            "windows-x64"
        };
        assets.insert(
            foreign.to_string(),
            DownloadAsset {
                url: "https://example.invalid/t.zip".into(),
                archive: ArchiveKind::Zip,
                sha256: None,
                bins: vec![],
            },
        );
        let err = resolve_platform_asset(&assets).unwrap_err().to_string();
        assert!(err.contains("platform"), "{err}");
        assert!(err.contains(foreign), "{err}");
    }

    #[test]
    fn tool_registry_roundtrip_in_tmp_root() {
        let _guard = TOOLS_ROOT_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        // SAFETY-free env override for the duration of the test.
        std::env::set_var("NIU_PLUGIN_TOOLS_ROOT", root.path());
        let record = ToolRecord {
            id: "demo".into(),
            version: "1.0.0".into(),
            url: "https://example.invalid/d.zip".into(),
            archive_sha256: "abc".into(),
            pinned: true,
            path: root.path().join("demo"),
            bins: vec!["demo.exe".into()],
            installed_at: "123".into(),
        };
        write_tool_registry(&[record]).unwrap();
        let read = read_tool_registry();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, "demo");
        assert_eq!(read[0].bins, vec!["demo.exe".to_string()]);
        std::env::remove_var("NIU_PLUGIN_TOOLS_ROOT");
    }
}
