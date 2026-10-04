//! Agent skill bundle installation (`niu skill install` / `niu skill status`,
//! niubash#188).
//!
//! The bundle itself lives in `skills/niubash/` at the repo root and is
//! embedded by the launcher (src/skill.rs, packaging-correct: the files are
//! inside the root package, not this crate's). This module owns the
//! target-directory contract and the install/diff mechanics so they are
//! unit-testable without the embedded content:
//!
//! - Named targets mirror the WinuxCmd-skill precedent: one directory per
//!   agent host, each carrying `<skills-root>/niubash/SKILL.md` plus the
//!   bundle's `references/` and `agents/` files (same structure as the
//!   released `niubash-skill-v*.zip`, so agents can consume either).
//! - `--target <dir>` installs into `<dir>/niubash/` — the generic escape
//!   hatch for hosts this product does not name.
//! - "Outdated" means drifted bytes, decided per file; there is no manifest
//!   file to get out of sync with the tree.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Directory name the bundle installs under, for every target.
pub const SKILL_DIR_NAME: &str = "niubash";

/// Named agent skill roots, in install/check order. Paths are relative to
/// the user home (the `dirs` home, consistent with path_utils).
pub const NAMED_TARGETS: &[(&str, &str)] = &[
    ("claude", ".claude/skills"),
    ("zcode", ".zcode/skills"),
    ("cursor", ".cursor/skills"),
];

/// One file of the bundle as the launcher embeds it.
pub struct SkillFile {
    /// Path relative to the bundle root (`SKILL.md`, `references/wpm.md`).
    pub rel: &'static str,
    /// Exact file content (LF endings; installs write it verbatim).
    pub content: &'static str,
}

/// Parsed `--target` value.
pub enum TargetSpec {
    /// A named host root from [`NAMED_TARGETS`].
    Named(&'static str),
    /// Every named host root.
    All,
    /// A generic skills root; installs `<dir>/niubash/`.
    Dir(PathBuf),
}

/// Parse a `--target` value: a named host, `all`, or a filesystem path.
pub fn resolve_target(spec: &str) -> anyhow::Result<TargetSpec> {
    if spec == "all" {
        return Ok(TargetSpec::All);
    }
    if let Some((name, _)) = NAMED_TARGETS.iter().find(|(n, _)| *n == spec) {
        return Ok(TargetSpec::Named(name));
    }
    if spec.is_empty() {
        anyhow::bail!(
            "--target requires a name ({}, all) or a directory",
            named_list()
        );
    }
    Ok(TargetSpec::Dir(PathBuf::from(spec)))
}

/// Comma-separated named targets for usage text.
pub fn named_list() -> String {
    NAMED_TARGETS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join("|")
}

/// The install directory for a named host: `<home>/<skills-root>/niubash/`.
/// `None` when the home directory cannot be resolved.
pub fn named_target_dir(name: &str) -> Option<PathBuf> {
    let (_, rel) = NAMED_TARGETS.iter().find(|(n, _)| *n == name)?;
    Some(
        crate::path_utils::shell_home_dir()?
            .join(rel)
            .join(SKILL_DIR_NAME),
    )
}

/// Write every bundle file under `target_dir`, creating parent directories.
/// Existing files are overwritten (the bundle is the source of truth).
/// Returns the number of files written.
pub fn install_files(files: &[SkillFile], target_dir: &Path) -> anyhow::Result<usize> {
    for file in files {
        let path = target_dir.join(file.rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, file.content)?;
    }
    Ok(files.len())
}

/// Per-target install state, decided by byte comparison per file.
pub struct TargetStatus {
    /// The `.../niubash` directory this status describes.
    pub dir: PathBuf,
    /// All bundle files exist.
    pub installed: bool,
    /// Installed and every file matches the embedded content byte-for-byte.
    pub up_to_date: bool,
    /// Bundle files absent from the target.
    pub missing: Vec<&'static str>,
    /// Present but differing files (drifted or hand-edited).
    pub differing: Vec<&'static str>,
}

/// Compare a target directory against the bundle content.
pub fn check_target(files: &[SkillFile], dir: &Path) -> TargetStatus {
    let mut missing = Vec::new();
    let mut differing = Vec::new();
    for file in files {
        match fs::read(dir.join(file.rel)) {
            Ok(bytes) => {
                if bytes.as_slice() != file.content.as_bytes() {
                    differing.push(file.rel);
                }
            }
            Err(_) => missing.push(file.rel),
        }
    }
    TargetStatus {
        dir: dir.to_path_buf(),
        installed: missing.is_empty(),
        up_to_date: missing.is_empty() && differing.is_empty(),
        missing,
        differing,
    }
}

/// sha256 of the bundle's SKILL.md content, for `skill status` display —
/// lets a script tell two bundles apart without byte-comparing trees.
pub fn skill_digest(files: &[SkillFile]) -> Option<String> {
    let content = files
        .iter()
        .find(|file| file.rel == "SKILL.md")
        .map(|file| file.content)?;
    let digest = Sha256::digest(content.as_bytes());
    Some(
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: &[SkillFile] = &[
        SkillFile {
            rel: "SKILL.md",
            content: "# bundle\n",
        },
        SkillFile {
            rel: "references/quickref.md",
            content: "quickref\n",
        },
    ];

    #[test]
    fn resolve_target_named_all_and_paths() {
        assert!(matches!(
            resolve_target("claude").unwrap(),
            TargetSpec::Named("claude")
        ));
        assert!(matches!(resolve_target("all").unwrap(), TargetSpec::All));
        assert!(matches!(
            resolve_target("C:/tmp/skills").unwrap(),
            TargetSpec::Dir(_)
        ));
        assert!(resolve_target("").is_err());
    }

    #[test]
    fn install_then_check_is_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills").join(SKILL_DIR_NAME);
        assert_eq!(install_files(FILES, &dir).unwrap(), 2);
        let status = check_target(FILES, &dir);
        assert!(status.installed);
        assert!(status.up_to_date);
        assert!(status.missing.is_empty());
        assert!(status.differing.is_empty());
        // Parents were created, content written verbatim.
        assert_eq!(
            fs::read_to_string(dir.join("references/quickref.md")).unwrap(),
            "quickref\n"
        );
    }

    #[test]
    fn check_reports_missing_and_drift() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(SKILL_DIR_NAME);
        let absent = check_target(FILES, &dir);
        assert!(!absent.installed);
        assert_eq!(absent.missing.len(), 2);

        install_files(FILES, &dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# edited\n").unwrap();
        let drifted = check_target(FILES, &dir);
        assert!(drifted.installed);
        assert!(!drifted.up_to_date);
        assert_eq!(drifted.differing, vec!["SKILL.md"]);
    }

    #[test]
    fn skill_digest_is_stable_hex() {
        let digest = skill_digest(FILES).unwrap();
        assert_eq!(digest.len(), 64);
        let again = skill_digest(FILES).unwrap();
        assert_eq!(digest, again);
    }
}
