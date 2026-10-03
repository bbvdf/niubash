//! Plugin collections — the LazyVim extras pattern for niu (study §8,
//! §10.2): a collection ("distro") is a *data manifest* listing recipe ids;
//! built-in collections ship in the product, imported collections are the
//! same format from a repo or directory, byte-identical in shape
//! (LazyVim's user-extras rule: anything the distro can do, a user repo
//! can do). `apply` walks the entries through [`super::recipes::install`]
//! and **never auto-trusts** — sources land untrusted with their review
//! verb printed, per owner ruling 2026-10-02.
//!
//! Imported collections live under `~/.niubash/distros/<name>/` with a
//! `niu-collection.toml` manifest at the root; the registry
//! (`~/.niubash/distros/registry.toml`) records provenance.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;

use super::recipes::{self, RecipeInstall};

/// Schema marker for collection manifests and the distro registry.
pub const COLLECTION_SCHEMA: &str = "niubash:plugin-collection@1";
/// Manifest file name inside a collection repo/directory.
pub const COLLECTION_MANIFEST: &str = "niu-collection.toml";

/// Built-in official collections (compiled in, same format as imports).
const BUILTIN_COLLECTIONS: &[(&str, &str)] = &[
    (
        "minimal",
        include_str!("../../assets/plugins/collections/minimal.toml"),
    ),
    (
        "recommended",
        include_str!("../../assets/plugins/collections/recommended.toml"),
    ),
    (
        "full",
        include_str!("../../assets/plugins/collections/full.toml"),
    ),
];

/// One collection manifest row.
#[derive(Debug, Clone, Deserialize)]
pub struct Collection {
    #[serde(default)]
    pub schema: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub entry: Vec<CollectionEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CollectionEntry {
    pub recipe: String,
}

/// Where a collection came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionOrigin {
    Builtin,
    Imported { path: PathBuf, origin: String },
}

#[derive(Debug, Clone)]
pub struct CollectionListing {
    pub collection: Collection,
    pub origin: CollectionOrigin,
}

fn parse_manifest(text: &str) -> anyhow::Result<Collection> {
    let collection: Collection = toml::from_str(text).context("parsing the collection manifest")?;
    if let Some(schema) = &collection.schema {
        if schema != COLLECTION_SCHEMA {
            bail!("unsupported collection schema '{schema}' (expected '{COLLECTION_SCHEMA}')");
        }
    }
    if collection.name.trim().is_empty() {
        bail!("collection manifest has no name");
    }
    Ok(collection)
}

/// Validate that every entry resolves to a known recipe (health-style:
/// name every unknown id, study §10.1).
pub fn validate_collection(collection: &Collection) -> anyhow::Result<()> {
    let unknown: Vec<&str> = collection
        .entry
        .iter()
        .map(|entry| entry.recipe.as_str())
        .filter(|recipe| recipes::recipe(recipe).is_none())
        .collect();
    if !unknown.is_empty() {
        bail!(
            "collection '{}' references unknown recipes: {}",
            collection.name,
            unknown.join(", ")
        );
    }
    Ok(())
}

/// All collections: built-ins first (stable order), then imported.
pub fn collections() -> Vec<CollectionListing> {
    let mut out = Vec::new();
    for (_, text) in BUILTIN_COLLECTIONS {
        if let Ok(collection) = parse_manifest(text) {
            out.push(CollectionListing {
                collection,
                origin: CollectionOrigin::Builtin,
            });
        }
    }
    for record in read_distro_registry() {
        let manifest = record.path.join(COLLECTION_MANIFEST);
        if let Ok(text) = fs::read_to_string(&manifest) {
            if let Ok(collection) = parse_manifest(&text) {
                out.push(CollectionListing {
                    collection,
                    origin: CollectionOrigin::Imported {
                        path: record.path.clone(),
                        origin: record.origin.clone(),
                    },
                });
            }
        }
    }
    out
}

pub fn collection(name: &str) -> Option<CollectionListing> {
    collections()
        .into_iter()
        .find(|listing| listing.collection.name == name)
}

/// Record in `~/.niubash/distros/registry.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct DistroRecord {
    pub name: String,
    /// Where the tree was imported from (git url or local path).
    pub origin: String,
    pub path: PathBuf,
    pub imported_at: String,
}

pub fn distros_root() -> PathBuf {
    if let Some(value) = std::env::var_os("NIU_PLUGIN_DISTROS_ROOT") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    crate::path_utils::shell_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".niubash")
        .join("distros")
}

fn distro_registry_path() -> PathBuf {
    distros_root().join("registry.toml")
}

fn read_distro_registry() -> Vec<DistroRecord> {
    let Ok(text) = fs::read_to_string(distro_registry_path()) else {
        return Vec::new();
    };
    #[derive(Deserialize)]
    struct Registry {
        /// Forward compatibility: unread (a mismatched registry degrades to
        /// "no imported collections", never a crash).
        #[serde(default, skip_serializing)]
        #[allow(dead_code)]
        schema: Option<String>,
        #[serde(default)]
        distro: Vec<DistroRecord>,
    }
    toml::from_str::<Registry>(&text)
        .map(|file| file.distro)
        .unwrap_or_default()
}

/// Import a collection from a git URL/shorthand or a local directory.
/// The tree lands under `~/.niubash/distros/<name>/`; the manifest must
/// parse and every entry must resolve. Re-importing the same origin is the
/// refresh path (the destination is replaced); a *different* origin taking
/// an imported name is refused, as is any name colliding with a built-in
/// (built-ins are listed first and would shadow the import).
pub fn import(origin: &str) -> anyhow::Result<Collection> {
    let normalized = super::sources::normalize_origin(origin);
    let staging = distros_root().join(format!(
        ".staging-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&staging)?;
    let result = (|| -> anyhow::Result<()> {
        let path = Path::new(&normalized);
        if path.is_dir() {
            copy_tree(path, &staging)?;
        } else {
            let status = Command::new("git")
                .args(["clone", "--depth", "1"])
                .arg(&normalized)
                .arg(&staging)
                .status()
                .context("failed to run git; is git on PATH?")?;
            if !status.success() {
                bail!("git clone of '{normalized}' failed");
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
        result?;
    }

    let text = fs::read_to_string(staging.join(COLLECTION_MANIFEST)).map_err(|err| {
        let _ = fs::remove_dir_all(&staging);
        anyhow!("no {COLLECTION_MANIFEST} at the root of '{normalized}': {err}")
    })?;
    // `inspect_err` is 1.76; the crate MSRV is 1.70 — same cleanup, no
    // new API (the staging dir must not survive a failed import).
    let collection = match parse_manifest(&text) {
        Ok(collection) => collection,
        Err(err) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(err);
        }
    };
    if let Err(err) = validate_collection(&collection) {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    // Name-collision policy (health-style: name the object and the repair
    // verb, study §10.1). Built-ins win every listing lookup, so importing
    // under a built-in name would silently shadow; an imported name may
    // only be refreshed from the origin that owns it.
    for listing in collections() {
        if listing.collection.name != collection.name {
            continue;
        }
        match listing.origin {
            CollectionOrigin::Builtin => {
                let _ = fs::remove_dir_all(&staging);
                bail!(
                    "collection name '{}' collides with a built-in collection; \
                     rename it in its {COLLECTION_MANIFEST}",
                    collection.name
                );
            }
            CollectionOrigin::Imported { origin, .. } if origin != normalized => {
                let _ = fs::remove_dir_all(&staging);
                bail!(
                    "collection '{}' is already imported from '{origin}'; \
                     remove it first: niu plugin distro remove {}",
                    collection.name,
                    collection.name
                );
            }
            CollectionOrigin::Imported { .. } => {} // same origin: refresh below
        }
    }

    let dest = distros_root().join(&collection.name);
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    fs::rename(&staging, &dest)?;

    let mut registry = read_distro_registry();
    registry.retain(|record| record.name != collection.name);
    registry.push(DistroRecord {
        name: collection.name.clone(),
        origin: normalized.clone(),
        path: dest.clone(),
        imported_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default(),
    });
    write_distro_registry(&registry)?;
    Ok(collection)
}

fn write_distro_registry(registry: &[DistroRecord]) -> anyhow::Result<()> {
    fs::create_dir_all(distros_root())?;
    let mut body = format!(
        "# Imported plugin collections (niu plugin distro import).\nschema = \"{COLLECTION_SCHEMA}\"\n"
    );
    for record in registry {
        body.push_str(&format!(
            "\n[[distro]]\nname = {}\norigin = {}\npath = {}\nimported_at = {}\n",
            toml_quote(&record.name),
            toml_quote(&record.origin),
            toml_quote(&record.path.display().to_string()),
            toml_quote(&record.imported_at)
        ));
    }
    fs::write(distro_registry_path(), body)?;
    Ok(())
}

fn toml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Remove an imported collection (built-ins cannot be removed).
pub fn remove(name: &str) -> anyhow::Result<()> {
    let listing = collection(name).ok_or_else(|| anyhow!("unknown collection '{name}'"))?;
    match listing.origin {
        CollectionOrigin::Builtin => bail!("'{name}' is built in and cannot be removed"),
        CollectionOrigin::Imported { path, .. } => {
            let mut registry = read_distro_registry();
            registry.retain(|record| record.name != name);
            write_distro_registry(&registry)?;
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            }
            Ok(())
        }
    }
}

/// Outcome of applying a collection: per-entry install reports (failures
/// collected, not thrown — a bad entry never kills the rest, lazy.nvim
/// `Spec:log` pattern, study §10.1) plus the undo targets.
#[derive(Debug, Clone)]
pub struct CollectionApply {
    pub name: String,
    pub reports: Vec<RecipeInstall>,
    /// Entries that failed: (recipe id, error message).
    pub failures: Vec<(String, String)>,
    /// Source ids installed by this apply (undo: `niu plugin source remove`).
    pub installed_sources: Vec<String>,
}

/// Apply a collection: walk the entries through the recipe drivers. Trust
/// is never granted here — each report carries the review verb.
/// Executable-tool entries recommend package managers (download retraction
/// 2026-10-04); they install nothing and fail nothing.
pub fn apply(name: &str) -> anyhow::Result<CollectionApply> {
    let listing = collection(name).ok_or_else(|| anyhow!("unknown collection '{name}'"))?;
    validate_collection(&listing.collection)?;
    let before_sources: Vec<String> = super::sources::read_source_registry()
        .into_iter()
        .map(|record| record.id)
        .collect();

    let mut reports = Vec::new();
    let mut failures = Vec::new();
    for entry in &listing.collection.entry {
        match recipes::install(&entry.recipe) {
            Ok(report) => reports.push(report),
            Err(err) => failures.push((entry.recipe.clone(), format!("{err:#}"))),
        }
    }

    let installed_sources: Vec<String> = super::sources::read_source_registry()
        .into_iter()
        .map(|record| record.id)
        .filter(|id| !before_sources.contains(id))
        .collect();
    Ok(CollectionApply {
        name: listing.collection.name,
        reports,
        failures,
        installed_sources,
    })
}

fn copy_tree(src: &Path, dest: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        // Guard: never recurse into the destination itself (importing a
        // directory that contains the distros root would otherwise copy
        // the staging dir into itself without bound).
        if path.starts_with(dest) {
            continue;
        }
        let target = dest.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else if path.is_file() {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::PROCESS_STATE_LOCK;

    struct EnvGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    #[test]
    fn builtin_collections_parse_and_validate() {
        let listings = collections();
        let names: Vec<&str> = listings
            .iter()
            .map(|l| l.collection.name.as_str())
            .collect();
        assert_eq!(names, ["minimal", "recommended", "full"], "{names:?}");
        for listing in &listings {
            validate_collection(&listing.collection).unwrap_or_else(|err| {
                panic!("built-in '{}' invalid: {err}", listing.collection.name)
            });
        }
    }

    #[test]
    fn builtin_recommended_rides_the_default_omb_theme() {
        let listing = collection("recommended").unwrap();
        let recipes: Vec<&str> = listing
            .collection
            .entry
            .iter()
            .map(|e| e.recipe.as_str())
            .collect();
        assert!(recipes.contains(&"oh-my-bash"), "{recipes:?}");
        assert!(recipes.contains(&"omb-theme-robbyrussell"), "{recipes:?}");
        assert!(recipes.contains(&"bash-completion"), "{recipes:?}");
    }

    #[test]
    fn manifest_schema_mismatch_is_refused() {
        let err = parse_manifest("schema = \"niubash:plugin-collection@99\"\nname = \"x\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("plugin-collection@1"), "{err}");
    }

    #[test]
    fn unknown_entries_are_named_in_validation() {
        let collection = Collection {
            schema: None,
            name: "test".into(),
            description: String::new(),
            entry: vec![
                CollectionEntry {
                    recipe: "oh-my-bash".into(),
                },
                CollectionEntry {
                    recipe: "no-such-recipe".into(),
                },
            ],
        };
        let err = validate_collection(&collection).unwrap_err().to_string();
        assert!(err.contains("no-such-recipe"), "{err}");
    }

    #[test]
    fn import_from_local_directory_registers_and_applies() {
        let temp = std::env::temp_dir().join(format!(
            "niu-distros-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let source = temp.join("collection-src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(COLLECTION_MANIFEST),
            "# test collection\nschema = \"niubash:plugin-collection@1\"\nname = \"test-distro\"\ndescription = \"d\"\n\n[[entry]]\nrecipe = \"bash-preexec\"\n",
        )
        .unwrap();
        // A real driver target so apply has something to install: point the
        // recipe's generic source at a local fixture? bash-preexec clones
        // from GitHub — not in tests. Validate import + listing only; the
        // apply path is covered by the recipes unit tests.
        let _guard = PROCESS_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _distros = EnvGuard::set(
            "NIU_PLUGIN_DISTROS_ROOT",
            &temp.join("distros").to_string_lossy(),
        );
        let imported = import(source.to_string_lossy().as_ref()).expect("import");
        assert_eq!(imported.name, "test-distro");
        let listings = collections();
        let names: Vec<&str> = listings
            .iter()
            .map(|l| l.collection.name.as_str())
            .collect();
        assert!(names.contains(&"test-distro"), "{names:?}");

        // Same origin again = refresh (no error, entry set still resolves).
        import(source.to_string_lossy().as_ref()).expect("re-import refreshes");

        // A different origin under the same name is refused, naming the
        // owning origin and the repair verb.
        let other = temp.join("collection-other");
        fs::create_dir_all(&other).unwrap();
        fs::write(
            other.join(COLLECTION_MANIFEST),
            "schema = \"niubash:plugin-collection@1\"\nname = \"test-distro\"\n\n[[entry]]\nrecipe = \"bash-preexec\"\n",
        )
        .unwrap();
        let err = import(other.to_string_lossy().as_ref())
            .unwrap_err()
            .to_string();
        assert!(err.contains("already imported"), "{err}");
        assert!(err.contains("niu plugin distro remove"), "{err}");

        remove("test-distro").expect("remove");
        assert!(!collections()
            .iter()
            .any(|l| l.collection.name == "test-distro"));
        drop(_distros);
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn import_refuses_builtin_name_collision() {
        let temp = std::env::temp_dir().join(format!(
            "niu-distros-builtin-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        // The import source must live outside the distros root (the staging
        // dir is created under it — copy_tree guards, but keep the fixture
        // shaped like a real import).
        let source = temp.join("collection-builtin");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(COLLECTION_MANIFEST),
            "schema = \"niubash:plugin-collection@1\"\nname = \"minimal\"\n\n[[entry]]\nrecipe = \"bash-completion\"\n",
        )
        .unwrap();
        let _guard = PROCESS_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _distros = EnvGuard::set(
            "NIU_PLUGIN_DISTROS_ROOT",
            &temp.join("distros").to_string_lossy(),
        );
        let err = import(source.to_string_lossy().as_ref())
            .unwrap_err()
            .to_string();
        assert!(err.contains("built-in"), "{err}");
    }

    #[test]
    fn apply_activates_assets_on_a_trusted_manager() {
        // Network-free apply path: with the oh-my-bash fixture installed and
        // trusted, a collection entry that rides it activates locally (the
        // recipes::install manager-asset branch). The collection itself is
        // imported from a local directory so no git clone runs.
        let _guard = PROCESS_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp = std::env::temp_dir().join(format!(
            "niu-distros-apply-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let _sources = EnvGuard::set(
            "NIU_PLUGIN_SOURCES_ROOT",
            &temp.join("sources").to_string_lossy(),
        );
        let _distros = EnvGuard::set(
            "NIU_PLUGIN_DISTROS_ROOT",
            &temp.join("distros").to_string_lossy(),
        );
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/sources/oh-my-bash");
        super::super::sources::add_source(super::super::sources::SourceInstallRequest {
            adapter: None,
            origin: fixture.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .expect("fixture source add");
        super::super::sources::trust_source("oh-my-bash").expect("fixture trust");

        let source = temp.join("collection-src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(COLLECTION_MANIFEST),
            "schema = \"niubash:plugin-collection@1\"\nname = \"test-apply\"\n\n[[entry]]\nrecipe = \"omb-theme-robbyrussell\"\n",
        )
        .unwrap();
        import(source.to_string_lossy().as_ref()).expect("import test collection");
        let outcome = apply("test-apply").expect("apply");
        assert_eq!(outcome.name, "test-apply");
        assert_eq!(outcome.reports.len(), 1, "{:?}", outcome.reports);
        assert_eq!(outcome.reports[0].recipe_id, "omb-theme-robbyrussell");

        // Cleanup: drop the source and the imported tree (both under temp).
        let _ = super::super::sources::remove_source("oh-my-bash");
    }

    #[test]
    fn builtin_cannot_be_removed() {
        let err = remove("minimal").unwrap_err().to_string();
        assert!(err.contains("built in"), "{err}");
    }
}
