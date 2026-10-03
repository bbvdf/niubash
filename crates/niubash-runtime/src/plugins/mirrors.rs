//! Download / git-transport mirroring — the China-network comfort layer
//! (design §14.8, owner ruling 2026-10-03: "源/proxy 怎么让中国用户用的
//! 舒服 能不能复用别人的各种国内源的下载渠道"; follow-up 2026-10-03:
//! mirror services are unstable, so ship the *pipeline*, not a curated
//! source list — "用户自己知道去哪找能用的镜像 URL，我们的工作是让贴
//! 进来就能用").
//!
//! Model: one user-editable config file `~/.niubash/mirrors.toml` holding
//! at most a `custom` mirror (a prefix URL, optionally a release-asset
//! override and a git insteadOf base) or `active = "none"` for direct
//! connection. `niu plugin mirror set <url>` writes it in one command;
//! community mirror examples live as comments, not code — they may die at
//! any moment and are the user's to verify. Two rewrite shapes, matching
//! how public mirror services actually work:
//!
//! * **Prefix rewrite** (ghproxy class): the mirror proxies any GitHub URL
//!   as `<prefix><full-original-url>`. Applies to `https://github.com/…`
//!   (release assets, archive downloads). A `[github.releases] prefix`
//!   override wins for release-asset URLs specifically.
//! * **git insteadOf** (gitclone class): git itself rewrites
//!   `https://github.com/…` to the mirror at fetch time via
//!   `-c url.<mirror-base>.insteadOf=https://github.com/`. Git-only mirrors
//!   carry no download prefix.
//!
//! Iron invariant (§14.8): **rewrites happen at the transport layer only.**
//! The spec (`plugins.toml`), the lock (`registry.toml`), tool records and
//! recipe URLs keep canonical GitHub origins, so a tree installed through a
//! mirror stays identical to a direct install and the lockfile remains
//! portable across machines and networks. Mirroring is also never a trust
//! signal: checksums and the trust protocol are unaffected. No auto-select:
//! the product probes and *suggests* (`niu doctor`, `niu plugin mirror
//! test`), the user pastes the URL they trust.
//!
//! Degrade discipline: a missing file, unknown `active` name, or malformed
//! TOML degrades to direct connection in the transport (a mirror config
//! problem must never break a download); `niu plugin mirror list` and `niu
//! doctor` surface the condition instead.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::path_utils::shell_home_dir;

pub const MIRRORS_SCHEMA: &str = "niubash:mirrors@0.1.0";

/// The canonical GitHub HTTPS origin eligible for rewriting. Everything
/// else (company GitLab, local paths, other hosts) passes through — a
/// mirror prefix must never silently capture traffic it does not proxy.
const GITHUB_HOST: &str = "https://github.com/";

/// Probe timeout for `niu plugin mirror test` / the doctor advisory row
/// (wpm uses 3000 ms for the same one-round-trip reachability check).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// A small representative object mirrors are probed against (`mirror
/// test`): the oh-my-bash README via raw.githubusercontent.com, the exact
/// shape plugin downloads use.
const PROBE_OBJECT: &str = "https://raw.githubusercontent.com/ohmybash/oh-my-bash/master/README.md";

/// Where mirrors.toml lives: `$NIU_MIRRORS` overrides (tests, portable
/// setups); default `~/.niubash/mirrors.toml` (the spec.rs convention).
pub fn mirrors_path() -> PathBuf {
    if let Some(value) = std::env::var_os("NIU_MIRRORS") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    shell_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".niubash")
        .join("mirrors.toml")
}

/// The parsed mirrors.toml. `Ok(None)` = no file yet (direct connection).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MirrorConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// `"none"` (direct, the default) or `"custom"` (use the `[github]`
    /// section). Any other value degrades to direct in the transport and is
    /// surfaced by `mirror list` / `niu doctor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    /// The custom mirror definition, consulted only when
    /// `active = "custom"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubMirror>,
}

/// The user-editable custom mirror (`[github]` in mirrors.toml). Absent
/// fields mean "that channel goes direct". Both rewrite shapes may be
/// combined (a service that proxies both HTTPS downloads and git).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GithubMirror {
    /// Prefix-style rewrite for all GitHub HTTPS downloads:
    /// requests go to `<prefix><full-original-url>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// git insteadOf base (git-only mirror class):
    /// `git -c url.<base>.insteadOf=https://github.com/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_instead_of: Option<String>,
    /// Override for release-asset downloads (`…/releases/download/…`),
    /// winning over `prefix` when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub releases: Option<Box<GithubMirror>>,
}

pub fn load_mirror_config() -> anyhow::Result<Option<MirrorConfig>> {
    let path = mirrors_path();
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let config: MirrorConfig = toml::from_str(&text)?;
    Ok(Some(config))
}

/// Render the config as the file users edit, with the example services as
/// comments (the "community mirror, may die anytime" caveat ships in the
/// file itself, not as code).
fn render_config(config: &MirrorConfig) -> String {
    let mut text = String::new();
    text.push_str(&format!("schema = \"{MIRRORS_SCHEMA}\"\n"));
    let active = config.active.as_deref().unwrap_or("none");
    text.push_str(&format!("active = \"{active}\"\n"));
    let github = config.github.clone().unwrap_or_default();
    let has_github =
        github.prefix.is_some() || github.git_instead_of.is_some() || github.releases.is_some();
    if has_github {
        text.push_str("\n[github]\n");
        if let Some(prefix) = github.prefix.as_deref().filter(|p| !p.is_empty()) {
            text.push_str(&format!("prefix = \"{prefix}\"\n"));
        }
        if let Some(git) = github.git_instead_of.as_deref().filter(|g| !g.is_empty()) {
            text.push_str(&format!("git_instead_of = \"{git}\"\n"));
        }
        if let Some(releases) = github
            .releases
            .as_ref()
            .and_then(|r| r.prefix.as_deref())
            .filter(|p| !p.is_empty())
        {
            text.push_str("\n[github.releases]\n");
            text.push_str(&format!("prefix = \"{releases}\"\n"));
        }
    }
    text.push_str(
        "\n\
        # ── notes ──────────────────────────────────────────────────────────\n\
        # `niu plugin mirror set <url>` rewrites this file for you;\n\
        # `niu plugin mirror set none` goes back to direct connection.\n\
        # prefix       = prefix-style HTTPS proxy: requests go to\n\
        #               <prefix><full-github-url>\n\
        # git_instead_of = git-only mirror (git clone/fetch rewrites\n\
        #               https://github.com/ to this base)\n\
        # [github.releases] prefix = override for release-asset downloads\n\
        #\n\
        # Community mirror examples — UNSUPPORTED and may disappear at any\n\
        # time; verify a service works for you before relying on it:\n\
        #   prefix = \"https://mirror.ghproxy.com/\"\n\
        #   prefix = \"https://ghproxy.net/\"\n\
        #   git_instead_of = \"https://gitclone.com/github.com\"\n",
    );
    text
}

fn write_mirror_config(config: &MirrorConfig) -> anyhow::Result<()> {
    let path = mirrors_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, render_config(config))?;
    Ok(())
}

/// Normalize a pasted mirror URL into a prefix: keep a trailing `/` so
/// `<prefix><full-url>` concatenation is always well-formed (the
/// "paste it and it works" bar, §14.8).
fn normalize_prefix(url: &str) -> anyhow::Result<String> {
    let url = url.trim().trim_end_matches('/');
    if !url.starts_with("https://") && !url.starts_with("http://") {
        anyhow::bail!(
            "mirror url must start with https:// (or http:// for a local proxy): '{url}'"
        );
    }
    if url == "https://" || url == "http://" {
        anyhow::bail!("mirror url has no host");
    }
    Ok(format!("{url}/"))
}

/// Switch to direct connection (`niu plugin mirror set none`). Preserves
/// the `[github]` section so re-enabling is a one-line `active` flip.
pub fn set_direct() -> anyhow::Result<()> {
    let mut config = load_mirror_config()?.unwrap_or_default();
    config.schema = Some(MIRRORS_SCHEMA.to_string());
    config.active = Some("none".to_string());
    write_mirror_config(&config)
}

/// Configure a prefix-style mirror URL in one command
/// (`niu plugin mirror set <url>`): validates the URL, normalizes the
/// trailing slash, sets `active = "custom"` and `[github] prefix`.
/// Any existing `git_instead_of` / `releases` fields are preserved.
pub fn set_custom_prefix(url: &str) -> anyhow::Result<String> {
    let prefix = normalize_prefix(url)?;
    let mut config = load_mirror_config()?.unwrap_or_default();
    config.schema = Some(MIRRORS_SCHEMA.to_string());
    config.active = Some("custom".to_string());
    let github = config.github.get_or_insert_with(Default::default);
    github.prefix = Some(prefix.clone());
    write_mirror_config(&config)?;
    Ok(prefix)
}

/// The mirror the transport should use right now. Infallible by design:
/// missing file, malformed TOML, and unknown names all degrade to the
/// `none` mirror (list/doctor report the condition; downloads never break).
pub fn resolve_active_mirror() -> ResolvedMirror {
    let Ok(Some(config)) = load_mirror_config() else {
        return ResolvedMirror::none();
    };
    match config.active.as_deref() {
        Some("custom") => {
            let github = config.github.unwrap_or_default();
            ResolvedMirror {
                name: "custom".to_string(),
                release_prefix: github
                    .releases
                    .as_ref()
                    .and_then(|releases| releases.prefix.clone()),
                download_prefix: github.prefix.clone(),
                git_instead_of_base: github.git_instead_of.clone(),
            }
        }
        _ => ResolvedMirror::none(),
    }
}

/// The fully-resolved mirror the transport consults.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedMirror {
    pub name: String,
    pub download_prefix: Option<String>,
    pub release_prefix: Option<String>,
    pub git_instead_of_base: Option<String>,
}

impl ResolvedMirror {
    pub fn none() -> Self {
        Self {
            name: "none".to_string(),
            download_prefix: None,
            release_prefix: None,
            git_instead_of_base: None,
        }
    }

    pub fn is_none(&self) -> bool {
        self.name == "none"
    }
}

/// Is this a GitHub release-asset download URL (both the pinned
/// `/releases/download/<tag>/…` and the floating
/// `/releases/latest/download/…` forms)?
fn is_release_download_url(url: &str) -> bool {
    url.contains("/releases/download/") || url.contains("/releases/latest/download/")
}

/// Rewrite one HTTPS download URL through `mirror` (pure; the transport
/// entry point is [`rewrite_download_url`]). Only GitHub URLs are touched;
/// everything else passes through byte-for-byte.
pub fn rewrite_download_url_with(mirror: &ResolvedMirror, url: &str) -> String {
    if mirror.is_none() || !url.starts_with(GITHUB_HOST) {
        return url.to_string();
    }
    if let Some(prefix) = mirror
        .release_prefix
        .as_deref()
        .filter(|p| !p.is_empty() && is_release_download_url(url))
    {
        return format!("{prefix}{url}");
    }
    if let Some(prefix) = mirror.download_prefix.as_deref().filter(|p| !p.is_empty()) {
        return format!("{prefix}{url}");
    }
    url.to_string()
}

/// Transport-layer URL rewrite using the currently active mirror. Callers
/// keep their canonical URL for records; only the actual request is
/// redirected through the active mirror (the §14.8 iron invariant).
pub fn rewrite_download_url(url: &str) -> String {
    let mirror = resolve_active_mirror();
    rewrite_download_url_with(&mirror, url)
}

/// Extra git arguments implementing insteadOf rewriting for `origin`
/// (pure; returns complete `-c` argument pairs to splice before the git
/// subcommand). Non-GitHub origins get nothing.
pub fn git_clone_args_with(mirror: &ResolvedMirror, origin: &str) -> Vec<String> {
    let Some(base) = mirror
        .git_instead_of_base
        .as_deref()
        .filter(|base| !base.is_empty())
    else {
        return Vec::new();
    };
    if !origin.starts_with(GITHUB_HOST) {
        return Vec::new();
    }
    vec![
        "-c".to_string(),
        format!("url.{base}.insteadOf={GITHUB_HOST}"),
    ]
}

/// git insteadOf arguments for the active mirror.
pub fn git_clone_args(origin: &str) -> Vec<String> {
    let mirror = resolve_active_mirror();
    git_clone_args_with(&mirror, origin)
}

/// The URL to probe for a configured mirror (`niu plugin mirror test`):
/// the download prefix against a representative object, or the git base
/// host for git-only mirrors. `None` when direct (probe GitHub itself).
pub fn mirror_probe_url(mirror: &ResolvedMirror) -> Option<String> {
    if mirror.is_none() {
        return None;
    }
    if let Some(prefix) = mirror.download_prefix.as_deref().filter(|p| !p.is_empty()) {
        return Some(format!("{prefix}{PROBE_OBJECT}"));
    }
    mirror
        .git_instead_of_base
        .as_deref()
        .filter(|base| !base.is_empty())
        .map(str::to_string)
}

/// One-round-trip reachability probe (wpm `probe_url` lesson: the answer
/// only suggests, it never gates a download). HEAD request; any completed
/// HTTP exchange — including an error status — proves the network path,
/// mirroring wpm's "reachable" definition. Returns the elapsed time.
pub fn probe_reachability(url: &str, timeout: Duration) -> Result<Duration, String> {
    let started = std::time::Instant::now();
    let response = ureq::head(url)
        .timeout(timeout)
        .set(
            "User-Agent",
            concat!("niubash-mirror-probe/", env!("CARGO_PKG_VERSION")),
        )
        .call();
    let elapsed = started.elapsed();
    match response {
        Ok(_) => Ok(elapsed),
        // 4xx/5xx still proves the server answered — reachable.
        Err(ureq::Error::Status(_, _)) => Ok(elapsed),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mirror(
        name: &str,
        download_prefix: Option<&str>,
        release_prefix: Option<&str>,
        git_instead_of_base: Option<&str>,
    ) -> ResolvedMirror {
        ResolvedMirror {
            name: name.to_string(),
            download_prefix: download_prefix.map(str::to_string),
            release_prefix: release_prefix.map(str::to_string),
            git_instead_of_base: git_instead_of_base.map(str::to_string),
        }
    }

    #[test]
    fn none_mirror_rewrites_nothing() {
        let none = ResolvedMirror::none();
        assert!(none.is_none());
        assert_eq!(
            rewrite_download_url_with(
                &none,
                "https://github.com/ohmybash/oh-my-bash/releases/download/v1/foo.zip"
            ),
            "https://github.com/ohmybash/oh-my-bash/releases/download/v1/foo.zip"
        );
        assert_eq!(
            rewrite_download_url_with(&none, "https://example.com/x"),
            "https://example.com/x"
        );
        assert!(git_clone_args_with(&none, "https://github.com/o/r.git").is_empty());
    }

    #[test]
    fn prefix_mirror_rewrites_github_urls_only() {
        let custom = mirror("custom", Some("https://mirror.example.com/"), None, None);
        assert_eq!(
            rewrite_download_url_with(
                &custom,
                "https://github.com/starship/starship/releases/download/v1.20/s.zip"
            ),
            "https://mirror.example.com/https://github.com/starship/starship/releases/download/v1.20/s.zip"
        );
        assert_eq!(
            rewrite_download_url_with(&custom, "https://github.com/o/r/archive/v1.tar.gz"),
            "https://mirror.example.com/https://github.com/o/r/archive/v1.tar.gz"
        );
        // Foreign hosts pass through untouched.
        assert_eq!(
            rewrite_download_url_with(&custom, "https://gitlab.com/o/r/-/archive/v1/r.tar.gz"),
            "https://gitlab.com/o/r/-/archive/v1/r.tar.gz"
        );
        // http (not https) GitHub URLs pass through too.
        assert_eq!(
            rewrite_download_url_with(&custom, "http://github.com/o/r"),
            "http://github.com/o/r"
        );
        // No trailing-slash prefix still concatenates sanely because
        // set_custom_prefix normalizes — but a bare custom mirror without
        // a prefix (git-only) leaves downloads alone.
        let git_only = mirror("custom", None, None, Some("https://git.example.com/gh"));
        assert_eq!(
            rewrite_download_url_with(
                &git_only,
                "https://github.com/starship/starship/releases/download/v1/s.zip"
            ),
            "https://github.com/starship/starship/releases/download/v1/s.zip"
        );
    }

    #[test]
    fn release_prefix_wins_over_general_prefix_for_release_assets() {
        let custom = mirror(
            "custom",
            Some("https://mirror.example.com/"),
            Some("https://release.example.com/"),
            None,
        );
        assert_eq!(
            rewrite_download_url_with(
                &custom,
                "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/Meslo.zip"
            ),
            "https://release.example.com/https://github.com/ryanoasis/nerd-fonts/releases/latest/download/Meslo.zip"
        );
        // Non-release GitHub URL still uses the general prefix.
        assert_eq!(
            rewrite_download_url_with(&custom, "https://github.com/o/r/archive/v1.tar.gz"),
            "https://mirror.example.com/https://github.com/o/r/archive/v1.tar.gz"
        );
    }

    #[test]
    fn git_instead_of_args_only_for_github_origins() {
        let custom = mirror(
            "custom",
            None,
            None,
            Some("https://gitclone.example.com/github.com"),
        );
        assert_eq!(
            git_clone_args_with(&custom, "https://github.com/ohmybash/oh-my-bash.git"),
            vec![
                "-c".to_string(),
                "url.https://gitclone.example.com/github.com.insteadOf=https://github.com/"
                    .to_string(),
            ]
        );
        // Non-GitHub origins get no insteadOf config.
        assert!(git_clone_args_with(&custom, "https://gitlab.com/o/r.git").is_empty());
        assert!(git_clone_args_with(&custom, "D:/local/repo").is_empty());
        assert!(git_clone_args_with(&custom, "git@github.com:o/r.git").is_empty());
    }

    #[test]
    fn prefix_normalization_for_pasted_urls() {
        assert_eq!(
            normalize_prefix("https://mirror.example.com").unwrap(),
            "https://mirror.example.com/"
        );
        assert_eq!(
            normalize_prefix("https://mirror.example.com/").unwrap(),
            "https://mirror.example.com/"
        );
        assert_eq!(
            normalize_prefix("  https://mirror.example.com/// ").unwrap(),
            "https://mirror.example.com/"
        );
        assert!(normalize_prefix("mirror.example.com").is_err());
        assert!(normalize_prefix("ftp://mirror.example.com").is_err());
        assert!(normalize_prefix("https://").is_err());
        assert!(normalize_prefix("").is_err());
    }

    #[test]
    fn probe_url_targets_the_configured_channels() {
        assert!(mirror_probe_url(&ResolvedMirror::none()).is_none());
        let prefix_mirror = mirror("custom", Some("https://m.example.com/"), None, None);
        assert_eq!(
            mirror_probe_url(&prefix_mirror).unwrap(),
            "https://m.example.com/https://raw.githubusercontent.com/ohmybash/oh-my-bash/master/README.md"
        );
        let git_mirror = mirror("custom", None, None, Some("https://g.example.com/gh"));
        assert_eq!(
            mirror_probe_url(&git_mirror).unwrap(),
            "https://g.example.com/gh"
        );
    }

    // ── config-file behavior (env-scoped temp file) ────────────────────────

    /// `NIU_MIRRORS` is process-global; serialize against every other
    /// env-mutating test in the crate (sources.rs uses the same lock for
    /// `NIU_PLUGIN_SOURCES_ROOT`).
    fn with_temp_mirrors(test: impl FnOnce(&std::path::Path)) {
        let _guard = crate::test_support::PROCESS_STATE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mirrors.toml");
        std::env::set_var("NIU_MIRRORS", &path);
        test(&path);
        std::env::remove_var("NIU_MIRRORS");
    }

    #[test]
    fn missing_config_means_direct() {
        with_temp_mirrors(|path| {
            assert!(!path.exists());
            assert!(resolve_active_mirror().is_none());
            assert_eq!(
                rewrite_download_url("https://github.com/o/r"),
                "https://github.com/o/r"
            );
            assert!(git_clone_args("https://github.com/o/r.git").is_empty());
        });
    }

    #[test]
    fn malformed_config_degrades_to_direct() {
        with_temp_mirrors(|path| {
            fs::write(path, "this is [ not toml").unwrap();
            assert!(resolve_active_mirror().is_none());
            assert!(load_mirror_config().is_err());
        });
    }

    #[test]
    fn set_custom_prefix_roundtrips_and_rewrites() {
        with_temp_mirrors(|path| {
            let stored = set_custom_prefix("https://mirror.example.com").unwrap();
            assert_eq!(stored, "https://mirror.example.com/");
            let text = fs::read_to_string(path).unwrap();
            assert!(text.contains(MIRRORS_SCHEMA));
            assert!(text.contains("active = \"custom\""));
            // the caveat ships in the file, not as a preset
            assert!(text.contains("UNSUPPORTED"));
            let config = load_mirror_config().unwrap().unwrap();
            assert_eq!(config.active.as_deref(), Some("custom"));
            assert_eq!(
                config.github.unwrap().prefix.as_deref(),
                Some("https://mirror.example.com/")
            );
            assert_eq!(
                rewrite_download_url("https://github.com/o/r/x.zip"),
                "https://mirror.example.com/https://github.com/o/r/x.zip"
            );
        });
    }

    #[test]
    fn set_direct_roundtrips_and_preserves_the_custom_section() {
        with_temp_mirrors(|_path| {
            set_custom_prefix("https://mirror.example.com/").unwrap();
            set_direct().unwrap();
            let config = load_mirror_config().unwrap().unwrap();
            assert_eq!(config.active.as_deref(), Some("none"));
            assert!(config.github.is_some(), "custom section preserved");
            assert!(resolve_active_mirror().is_none());
            assert_eq!(
                rewrite_download_url("https://github.com/o/r/x.zip"),
                "https://github.com/o/r/x.zip"
            );
        });
    }

    #[test]
    fn hand_edited_full_config_resolves_every_channel() {
        with_temp_mirrors(|_path| {
            fs::write(
                mirrors_path(),
                concat!(
                    "schema = \"niubash:mirrors@0.1.0\"\n",
                    "active = \"custom\"\n",
                    "\n[github]\n",
                    "prefix = \"https://mirror.corp.cn/\"\n",
                    "git_instead_of = \"https://git.corp.cn/github.com\"\n",
                    "\n[github.releases]\n",
                    "prefix = \"https://release.corp.cn/\"\n",
                ),
            )
            .unwrap();
            assert_eq!(
                rewrite_download_url("https://github.com/o/r/releases/download/v1/a.zip"),
                "https://release.corp.cn/https://github.com/o/r/releases/download/v1/a.zip"
            );
            assert_eq!(
                rewrite_download_url("https://github.com/o/r/x.tar.gz"),
                "https://mirror.corp.cn/https://github.com/o/r/x.tar.gz"
            );
            assert_eq!(
                git_clone_args("https://github.com/o/r.git"),
                vec![
                    "-c".to_string(),
                    "url.https://git.corp.cn/github.com.insteadOf=https://github.com/".to_string()
                ]
            );
        });
    }

    #[test]
    fn unknown_active_name_degrades_to_direct() {
        with_temp_mirrors(|path| {
            fs::write(path, "active = \"dead-mirror\"").unwrap();
            assert!(resolve_active_mirror().is_none());
        });
    }

    #[test]
    fn set_custom_prefix_rejects_non_urls() {
        with_temp_mirrors(|_path| {
            assert!(set_custom_prefix("not a url").is_err());
            assert!(set_custom_prefix("").is_err());
        });
    }
}
