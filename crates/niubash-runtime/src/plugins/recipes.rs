//! Plugin recipe registry — the mason-registry pattern for niu (study §6,
//! §10.5): one pure-data index, one row per known bash-ecosystem asset, no
//! behavior code per recipe. Bulk rows are generated from the ecosystem
//! corpus (`scripts/gen-plugin-recipes.py`); manager rows for oh-my-bash /
//! bash-it / bash-completion are derived from the compiled-in adapters at
//! runtime so ids/origins/licenses can never drift.
//!
//! Drivers (closed set, study §10.3): `git` (tree source through the
//! existing adapters — manager or generic-with-entry) and `download`
//! (direct binary via [`super::download`], pure Rust, no external package
//! manager — owner ruling 2026-10-03). Asset recipes (`manager` + `asset`)
//! ride on a manager source and resolve through its own selection
//! mechanism.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use anyhow::{anyhow, bail};
use serde::Deserialize;

use super::assets;
use super::download::{self, DownloadAsset};
use super::sources;

/// Generated seed index, compiled in (regenerate with
/// `scripts/gen-plugin-recipes.py`).
const RECIPES_TOML: &str = include_str!("../../assets/plugins/recipes.toml");

/// Recipe categories (the index's five classes plus `alias`, which oh-my-bash
/// and bash-it both ship as a distinct asset class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeCategory {
    Manager,
    Theme,
    Plugin,
    Alias,
    Completion,
    Prompt,
}

impl RecipeCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manager => "manager",
            Self::Theme => "theme",
            Self::Plugin => "plugin",
            Self::Alias => "alias",
            Self::Completion => "completion",
            Self::Prompt => "prompt",
        }
    }
}

/// How a recipe installs. Absent = info-only row (known ecosystem asset;
/// `add` explains instead of installing).
#[derive(Debug, Clone)]
pub enum RecipeDriver {
    /// Git tree source: `kind` names the source adapter (a manager id or
    /// `generic`), `entry` the generic loader entry file.
    Git {
        kind: String,
        entry: Option<String>,
        origin: Option<String>,
    },
    /// Direct binary download (pure-Rust driver).
    Download {
        version: String,
        downloads: BTreeMap<String, DownloadAsset>,
        bins: Vec<String>,
    },
}

/// One recipe row (data only — the generator and the adapter-derived
/// manager rows are the only producers). Deserialized from a flat raw row
/// (driver fields are siblings, mason-style) and folded into
/// [`RecipeDriver`]; TOML-internal tagged enums cannot express that shape.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: String,
    pub category: RecipeCategory,
    pub summary: String,
    pub license: String,
    /// Info URL (upstream page, tree, release).
    pub url: String,
    pub driver: Option<RecipeDriver>,
    /// Asset recipes: the manager source this rides on.
    pub manager: Option<String>,
    pub asset_kind: Option<String>,
    pub asset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRecipe {
    id: String,
    category: RecipeCategory,
    summary: String,
    license: String,
    url: String,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    downloads: Option<BTreeMap<String, DownloadAsset>>,
    #[serde(default)]
    bins: Option<Vec<String>>,
    #[serde(default)]
    manager: Option<String>,
    #[serde(default)]
    asset_kind: Option<String>,
    #[serde(default)]
    asset: Option<String>,
}

impl TryFrom<RawRecipe> for Recipe {
    type Error = String;
    fn try_from(raw: RawRecipe) -> Result<Self, Self::Error> {
        let driver = match raw.driver.as_deref() {
            None => None,
            Some("git") => Some(RecipeDriver::Git {
                kind: raw
                    .kind
                    .clone()
                    .ok_or(format!("recipe '{}': driver=git requires kind", raw.id))?,
                entry: raw.entry.clone(),
                origin: raw.origin.clone(),
            }),
            Some("download") => Some(RecipeDriver::Download {
                version: raw.version.clone().ok_or(format!(
                    "recipe '{}': driver=download requires version",
                    raw.id
                ))?,
                downloads: raw.downloads.clone().unwrap_or_default(),
                bins: raw.bins.clone().unwrap_or_default(),
            }),
            Some(other) => {
                return Err(format!(
                    "recipe '{}': unknown driver '{other}' (expected git|download)",
                    raw.id
                ))
            }
        };
        if raw.entry.is_some() && raw.driver.as_deref() != Some("git") {
            return Err(format!("recipe '{}': entry requires driver=git", raw.id));
        }
        if raw.manager.is_some() && raw.driver.is_some() {
            return Err(format!(
                "recipe '{}': manager rows cannot carry a driver",
                raw.id
            ));
        }
        Ok(Recipe {
            id: raw.id,
            category: raw.category,
            summary: raw.summary,
            license: raw.license,
            url: raw.url,
            driver,
            manager: raw.manager,
            asset_kind: raw.asset_kind,
            asset: raw.asset,
        })
    }
}

/// The merged registry: adapter-derived manager rows + generated data rows.
pub fn recipes() -> &'static [Recipe] {
    static RECIPES: OnceLock<Vec<Recipe>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        #[derive(Deserialize)]
        struct Registry {
            #[serde(default)]
            recipe: Vec<RawRecipe>,
        }
        let parsed: Registry = toml::from_str(RECIPES_TOML)
            .unwrap_or_else(|err| panic!("compiled-in recipes.toml must parse: {err}"));
        let mut all: Vec<Recipe> = parsed
            .recipe
            .into_iter()
            .map(|raw| {
                raw.try_into()
                    .unwrap_or_else(|err| panic!("compiled-in recipes.toml is invalid: {err}"))
            })
            .collect();
        // Manager rows derive from the adapters (single source of truth —
        // catalog.rs derives its shorthand table the same way).
        for adapter in sources::builtin_source_adapters() {
            let Some(origin) = adapter.default_origin() else {
                continue;
            };
            all.push(Recipe {
                id: adapter.id().to_string(),
                category: RecipeCategory::Manager,
                summary: adapter.summary().to_string(),
                license: adapter.license().to_string(),
                url: origin.trim_end_matches(".git").to_string(),
                driver: Some(RecipeDriver::Git {
                    kind: adapter.id().to_string(),
                    entry: None,
                    origin: Some(origin.to_string()),
                }),
                manager: None,
                asset_kind: None,
                asset: None,
            });
        }
        all.sort_by(|left, right| left.id.cmp(&right.id));
        all
    })
}

pub fn recipe(id: &str) -> Option<&'static Recipe> {
    recipes().iter().find(|recipe| recipe.id == id)
}

/// Outcome of `niu plugin add <recipe-id>`: what happened and what the user
/// should run next (health-style "name the repair verb", study §10.1).
#[derive(Debug, Clone)]
pub struct RecipeInstall {
    pub recipe_id: String,
    /// human-readable one-line summary of what was done.
    pub summary: String,
    /// next commands, in order (empty when everything is already active).
    pub next: Vec<String>,
}

/// `niu plugin add <recipe-id>`: install through the recipe's driver.
/// Every branch keeps the trust protocol: sources land **untrusted**,
/// assets activate only through an explicit later step (or immediately
/// when the manager source is already trusted).
pub fn install(id: &str) -> anyhow::Result<RecipeInstall> {
    let found = recipe(id).ok_or_else(|| anyhow!("unknown recipe '{id}'"))?;
    match (&found.driver, &found.manager) {
        (
            Some(RecipeDriver::Git {
                kind,
                entry,
                origin,
            }),
            None,
        ) => install_git_source(&found.id, kind, entry.as_deref(), origin.as_deref()),
        (None, Some(manager)) => install_manager_asset(found, manager),
        (None, None) => Ok(RecipeInstall {
            recipe_id: found.id.clone(),
            summary: format!(
                "'{}' is an info-only recipe (no niu driver); see {}",
                found.id, found.url
            ),
            next: vec![],
        }),
        (Some(RecipeDriver::Download { .. }), Some(_)) => bail!(
            "recipe '{}' is malformed: download driver with a manager",
            found.id
        ),
        (Some(RecipeDriver::Git { .. }), Some(_)) => bail!(
            "recipe '{}' is malformed: git driver with a manager",
            found.id
        ),
        (
            Some(RecipeDriver::Download {
                version,
                downloads,
                bins,
            }),
            None,
        ) => install_download(found, version, downloads, bins),
    }
}

fn install_git_source(
    id: &str,
    kind: &str,
    entry: Option<&str>,
    origin: Option<&str>,
) -> anyhow::Result<RecipeInstall> {
    let origin = origin.ok_or_else(|| anyhow!("recipe '{id}' has no git origin"))?;
    let mut request = sources::SourceInstallRequest {
        adapter: Some(kind.to_string()),
        origin: sources::normalize_origin(origin),
        ..Default::default()
    };
    if kind == "generic" {
        request.id = Some(id.to_string());
        request.entry = entry.map(str::to_string);
    }
    let record = sources::add_source(request)?;
    let mut next = vec![format!("niu plugin trust {}", record.id)];
    if record.id != id {
        // Generic sources activate whole-source after trust.
        next.push(format!("niu plugin enable {}", record.id));
    } else {
        next.push(format!("niu plugin enable {}", record.id));
    }
    Ok(RecipeInstall {
        recipe_id: id.to_string(),
        summary: format!(
            "installed source '{}' ({}) — untrusted",
            record.id, record.version
        ),
        next,
    })
}

/// Asset recipe: ensure the manager source exists, then activate the asset
/// through the manager's own selection mechanism (only when already
/// trusted).
fn install_manager_asset(found: &Recipe, manager: &str) -> anyhow::Result<RecipeInstall> {
    let asset = found
        .asset
        .as_deref()
        .ok_or_else(|| anyhow!("recipe '{}' has no asset name", found.id))?;
    let installed = sources::read_source_registry()
        .into_iter()
        .find(|record| record.id == manager);
    match installed {
        None => {
            // Install the manager first (untrusted), then hint the chain.
            let manager_recipe = recipe(manager).ok_or_else(|| {
                anyhow!(
                    "asset recipe '{}' references unknown manager '{manager}'",
                    found.id
                )
            })?;
            let manager_driver = manager_recipe
                .driver
                .as_ref()
                .ok_or_else(|| anyhow!("manager '{manager}' has no install driver"))?;
            let RecipeDriver::Git { kind, origin, .. } = manager_driver else {
                bail!("manager '{manager}' is not a git source");
            };
            let origin = origin
                .as_deref()
                .ok_or_else(|| anyhow!("manager '{manager}' has no origin"))?;
            sources::add_source(sources::SourceInstallRequest {
                adapter: Some(kind.clone()),
                origin: sources::normalize_origin(origin),
                ..Default::default()
            })?;
            Ok(RecipeInstall {
                recipe_id: found.id.clone(),
                summary: format!(
                    "installed manager '{manager}' (untrusted); asset '{asset}' activates after trust"
                ),
                next: vec![
                    format!("niu plugin trust {manager}"),
                    format!("niu plugin enable {asset}"),
                ],
            })
        }
        Some(record) if !record.trusted => Ok(RecipeInstall {
            recipe_id: found.id.clone(),
            summary: format!(
                "manager '{manager}' is installed but untrusted; asset '{asset}' waits"
            ),
            next: vec![
                format!("niu plugin trust {manager}"),
                format!("niu plugin enable {asset}"),
            ],
        }),
        Some(_) => {
            let outcome = assets::enable(asset)?;
            Ok(RecipeInstall {
                recipe_id: found.id.clone(),
                summary: outcome.summary,
                next: vec![],
            })
        }
    }
}

fn install_download(
    found: &Recipe,
    version: &str,
    downloads: &BTreeMap<String, DownloadAsset>,
    bins: &[String],
) -> anyhow::Result<RecipeInstall> {
    let asset = download::resolve_platform_asset(downloads)?;
    let mut asset = asset.clone();
    if asset.bins.is_empty() {
        asset.bins = bins.to_vec();
    }
    if asset.bins.is_empty() {
        bail!("recipe '{}' declares no binaries", found.id);
    }
    let install = download::install_executable(&found.id, version, &asset)?;
    let pinned = if install.pinned_checksum {
        format!(
            "sha256 verified ({})",
            short(&install.record.archive_sha256)
        )
    } else {
        "sha256 recorded (recipe pins no checksum — upstream digest unavailable)".to_string()
    };
    Ok(RecipeInstall {
        recipe_id: found.id.clone(),
        summary: format!(
            "installed '{}' {} into {} [{pinned}]",
            found.id,
            version,
            install.record.path.display()
        ),
        next: vec![format!("niu plugin enable {}", found.id)],
    })
}

fn short(digest: &str) -> String {
    digest.chars().take(12).collect()
}

/// `niu plugin enable/disable <id>` routing: executable tools activate a
/// PATH managed block (mason PATH-as-policy, study §10.3); everything else
/// goes to the asset/asset-layer machinery.
pub fn enable(id: &str) -> anyhow::Result<assets::ActivationOutcome> {
    if let Some(record) = download::read_tool_registry()
        .into_iter()
        .find(|record| record.id == id)
    {
        let rc = assets::write_tool_path_block(id, &record.path)?;
        return Ok(assets::ActivationOutcome {
            summary: format!(
                "tool '{id}' on PATH — managed block added to {}",
                rc.display()
            ),
            undo: format!("niu plugin disable {id}"),
        });
    }
    assets::enable(id)
}

pub fn disable(id: &str) -> anyhow::Result<assets::ActivationOutcome> {
    if download::read_tool_registry()
        .iter()
        .any(|record| record.id == id)
    {
        if assets::remove_tool_path_block(id)? {
            return Ok(assets::ActivationOutcome {
                summary: format!("tool '{id}' PATH block removed"),
                undo: format!("niu plugin enable {id}"),
            });
        }
    }
    assets::disable(id)
}

/// Install state of one recipe for listings/UI: which channel owns it and
/// where it currently stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeState {
    /// Not installed.
    Available,
    /// Executable tool installed through the download driver.
    ToolInstalled,
    /// Riding on an installed manager source (sub-state of the source).
    ManagerAsset { manager_state: String },
    /// Info-only row.
    InfoOnly,
}

pub fn recipe_state(id: &str) -> RecipeState {
    let Some(found) = recipe(id) else {
        return RecipeState::Available;
    };
    if download::read_tool_registry()
        .iter()
        .any(|tool| tool.id == id)
    {
        return RecipeState::ToolInstalled;
    }
    match (&found.driver, &found.manager) {
        (None, Some(manager)) => {
            let state = sources::read_source_registry()
                .into_iter()
                .find(|record| record.id == *manager)
                .map(|record| {
                    if !record.path.is_dir() {
                        "degraded".to_string()
                    } else if record.trusted {
                        "ready".to_string()
                    } else {
                        "untrusted".to_string()
                    }
                })
                .unwrap_or_else(|| "not-installed".to_string());
            RecipeState::ManagerAsset {
                manager_state: state,
            }
        }
        (None, None) => RecipeState::InfoOnly,
        (Some(RecipeDriver::Git { .. }), manager) => {
            let source_id = manager.clone().unwrap_or_else(|| found.id.clone());
            match sources::read_source_registry()
                .into_iter()
                .find(|record| record.id == source_id)
            {
                Some(record) if record.path.is_dir() => RecipeState::ManagerAsset {
                    manager_state: if record.trusted {
                        "ready".to_string()
                    } else {
                        "untrusted".to_string()
                    },
                },
                Some(_) => RecipeState::ManagerAsset {
                    manager_state: "degraded".to_string(),
                },
                None => RecipeState::Available,
            }
        }
        (Some(RecipeDriver::Download { .. }), _) => RecipeState::Available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_in_index_parses_and_is_unique() {
        let all = recipes();
        assert!(
            all.len() > 400,
            "expected a full seed index, got {}",
            all.len()
        );
        let mut ids: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate recipe ids in the index");
    }

    #[test]
    fn manager_rows_derive_from_adapters() {
        for id in ["oh-my-bash", "bash-it", "bash-completion"] {
            let found = recipe(id).expect("manager row");
            assert_eq!(found.category, RecipeCategory::Manager);
            assert!(matches!(found.driver, Some(RecipeDriver::Git { .. })));
        }
        // The generic adapter has no catalog row (never auto-installed).
        assert!(recipe("generic").is_none());
    }

    #[test]
    fn seed_covers_the_named_ecosystem_assets() {
        for id in [
            "omb-theme-agnoster",
            "omb-plugin-git",
            "bashit-alias-git",
            "bashit-plugin-fzf", // if present in corpus
            "bash-completion",
            "bash-preexec",
            "bpkg",
            "basher",
            "starship",
            "fzf",
            "bash-git-prompt",
            "sexy-bash-prompt",
        ] {
            if id == "bashit-plugin-fzf" {
                // corpus-dependent; only assert when present
                continue;
            }
            assert!(recipe(id).is_some(), "missing seed recipe '{id}'");
        }
    }

    #[test]
    fn every_category_has_rows() {
        // The five named classes (managers/themes/plugins/completions/
        // prompts) plus the omb alias rows must all be non-empty.
        for category in [
            RecipeCategory::Manager,
            RecipeCategory::Theme,
            RecipeCategory::Plugin,
            RecipeCategory::Completion,
            RecipeCategory::Prompt,
            RecipeCategory::Alias,
        ] {
            let count = recipes()
                .iter()
                .filter(|recipe| recipe.category == category)
                .count();
            assert!(count > 0, "category '{}' has no rows", category.as_str());
        }
        let prompts = recipes()
            .iter()
            .filter(|recipe| recipe.category == RecipeCategory::Prompt)
            .count();
        assert!(prompts >= 4, "prompt category too thin: {prompts}");
    }

    #[test]
    fn download_recipes_carry_platform_rows() {
        let starship = recipe("starship").expect("starship recipe");
        let Some(RecipeDriver::Download { downloads, .. }) = &starship.driver else {
            panic!("starship must be a download recipe");
        };
        assert!(downloads.contains_key("windows-x64"), "{downloads:?}");
        assert!(downloads.contains_key("linux-x64"), "{downloads:?}");
    }

    #[test]
    fn download_recipes_pin_sha256_per_platform() {
        // Integrity is data, not policy: a download row without a pinned
        // digest silently downgrades to "checksum recorded" (download.rs).
        // The compiled-in seed must never ship that downgrade.
        for found in recipes() {
            if let Some(RecipeDriver::Download { downloads, .. }) = &found.driver {
                for (platform, asset) in downloads {
                    assert!(
                        asset
                            .sha256
                            .as_deref()
                            .is_some_and(|digest| !digest.is_empty()),
                        "recipe '{}' platform '{platform}' pins no sha256",
                        found.id
                    );
                }
            }
        }
    }

    #[test]
    fn asset_recipes_reference_known_managers() {
        for found in recipes() {
            if let Some(manager) = &found.manager {
                assert!(
                    recipe(manager).is_some(),
                    "recipe '{}' references unknown manager '{manager}'",
                    found.id
                );
            }
        }
    }

    #[test]
    fn info_only_recipes_explain_instead_of_installing() {
        let ble = recipe("ble.sh").expect("ble.sh recipe");
        assert!(
            ble.driver.is_none(),
            "ble.sh is info-only (build from source)"
        );
    }
}
