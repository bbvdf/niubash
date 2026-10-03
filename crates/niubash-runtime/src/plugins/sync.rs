//! `niu plugin sync` — reconcile the declarative spec against the machine
//! (design §14.6.3; lazy.nvim `:Lazy sync` semantics).
//!
//! The spec (`plugins::spec`, `~/.niubash/plugins.toml`) declares *what
//! should exist*; the registry (`plugins::sources`) locks *what exists*
//! (commit + tree checksum). Sync is the only thing that moves the two
//! towards each other:
//!
//! 1. **declared, not installed** → fetch through the add pipeline
//!    (fetch gate only; the trust gate is never automatic — the new source
//!    lands untrusted and sync prints the exact `niu plugin trust <id>`);
//! 2. **declared, installed, trusted** → materialize the managed rc block
//!    / enabled tree *from the spec*, idempotently (`plugins::assets::
//!    materialize_spec_selection`, hand-added entries preserved);
//! 3. **installed, not declared** → suggest cleanup, never auto-delete
//!    (`--prune` removes them explicitly);
//! 4. **spec absent** → legacy imperative mode: nothing to reconcile; sync
//!    reports and suggests a starter spec.
//!
//! `niu plugin sync --bootstrap` is the same reconciliation in its quiet
//! startup form (clean machine → zero output), wired into the rc by the
//! setup wizard as a single bootstrap line.

use super::assets;
use super::sources::{
    self, normalize_origin, read_source_registry, SourceInstallRequest, SourceRecord,
};
use super::spec::{self, PluginSpec, SpecSource};

#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    /// Remove installed-but-undeclared sources (the explicit confirmation
    /// for the cleanup suggestion; never the default).
    pub prune: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncRow {
    pub id: String,
    /// installed | awaiting-trust | activated | unchanged | deactivated |
    /// degraded | drift | failed | removed | unsupported
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncReport {
    /// True when a spec file exists (false = legacy imperative mode).
    pub spec_present: bool,
    pub rows: Vec<SyncRow>,
    /// Installed sources the spec does not declare (cleanup suggestions).
    pub undeclared: Vec<String>,
    /// True when nothing needed doing (the bootstrap fast path).
    pub clean: bool,
}

impl SyncReport {
    /// True when the reconciler did (or would do) nothing: no installs, no
    /// materialization changes, no drift, no undeclared sources.
    fn compute_clean(&self) -> bool {
        self.rows.iter().all(|row| row.action == "unchanged") && self.undeclared.is_empty()
    }
}

/// Resolve a spec target the same way `niu plugin add` does: catalog ids
/// expand to their official origin, shorthand/URLs/paths normalize.
fn resolve_spec_origin(target: &str) -> anyhow::Result<(Option<String>, String)> {
    let target = target.trim();
    if target.is_empty() {
        anyhow::bail!("spec entry has an empty target");
    }
    // Catalog id (bare word naming a manager)?
    if !target.contains('/')
        && !target.contains('\\')
        && !target.contains("://")
        && !target.starts_with("./")
        && !target.starts_with("../")
        && !std::path::Path::new(target).is_absolute()
    {
        if let Some(entry) = super::catalog::catalog_entry(target) {
            return Ok((Some(entry.id.to_string()), entry.origin.to_string()));
        }
    }
    Ok((None, normalize_origin(target)))
}

/// The registry record a spec entry owns: explicit id, catalog id, or the
/// recorded origin — in that order.
fn record_for_entry<'a>(
    entry: &SpecSource,
    registry: &'a [SourceRecord],
) -> Option<&'a SourceRecord> {
    if let Some(id) = &entry.id {
        return registry.iter().find(|record| &record.id == id);
    }
    let resolved = resolve_spec_origin(&entry.target).ok();
    // Catalog targets resolve to the manager id.
    if let Some((Some(adapter_id), _)) = &resolved {
        if let Some(record) = registry.iter().find(|record| record.id == *adapter_id) {
            return Some(record);
        }
    }
    // Origin match (covers forks whose derived id differs from the manager
    // id the detection assigned).
    let (_, origin) = resolved?;
    registry
        .iter()
        .find(|record| record.url.trim() == origin.trim())
        .or_else(|| {
            registry
                .iter()
                .find(|record| record.url.trim() == entry.target.trim())
        })
}

/// Would this (target, id, origin) triple already be declared? Returns
/// the existing entry's identifying id/target for the duplicate message.
pub fn declared_entry(
    spec: &PluginSpec,
    target: &str,
    id: Option<&str>,
    origin: &str,
) -> Option<String> {
    if let Some(id) = id {
        if let Some(entry) = spec.entry_for_id(id) {
            return Some(entry.id.clone().unwrap_or_else(|| entry.target.clone()));
        }
    }
    for entry in &spec.sources {
        if entry.target == target {
            return Some(entry.id.clone().unwrap_or_else(|| entry.target.clone()));
        }
        if let Ok((_, entry_origin)) = resolve_spec_origin(&entry.target) {
            if entry_origin == origin || entry_origin == target {
                return Some(entry.id.clone().unwrap_or_else(|| entry.target.clone()));
            }
        }
    }
    None
}

/// Run the reconciliation. Never prompts (the CLI layer prints; `--prune`
/// is the explicit confirm). See the module docs for the algorithm.
pub fn sync_spec(options: SyncOptions) -> anyhow::Result<SyncReport> {
    if let Some(spec_file) = spec::load_spec()? {
        sync_with_spec(spec_file, options)
    } else {
        Ok(SyncReport {
            spec_present: false,
            rows: Vec::new(),
            undeclared: read_source_registry()
                .into_iter()
                .map(|record| record.id)
                .collect(),
            clean: false,
        })
    }
}

fn sync_with_spec(mut spec: PluginSpec, options: SyncOptions) -> anyhow::Result<SyncReport> {
    let mut registry = read_source_registry();
    let mut rows: Vec<SyncRow> = Vec::new();
    let mut spec_changed = false;
    let mut declared_ids: Vec<String> = Vec::new();

    for entry in spec.sources.iter_mut() {
        let (catalog_hint, origin) = match resolve_spec_origin(&entry.target) {
            Ok(resolved) => resolved,
            Err(err) => {
                rows.push(SyncRow {
                    id: entry.id.clone().unwrap_or_else(|| entry.target.clone()),
                    action: "failed".to_string(),
                    detail: err.to_string(),
                });
                continue;
            }
        };
        // An explicit `kind` pin (e.g. a tree adopted as `bpkg`) wins over
        // catalog resolution and over auto-detection: later syncs reinstall
        // through that adapter and refuse when the fingerprint stops
        // matching, instead of silently degrading to wild-file loading.
        let adapter_hint = entry.kind.clone().or(catalog_hint);
        let existing = record_for_entry(entry, &registry);
        let record = match existing {
            Some(record) => record.clone(),
            None => {
                // Declared but not installed: fetch (fetch gate only —
                // trust is never automatic).
                let request = SourceInstallRequest {
                    adapter: adapter_hint.clone(),
                    origin: origin.clone(),
                    ref_name: entry.ref_name.clone(),
                    commit: None,
                    expected_checksum: None,
                    id: entry.id.clone(),
                    entry: None,
                };
                match sources::add_source(request) {
                    Ok(record) => {
                        // Persist the derived id into the spec so later
                        // syncs match directly instead of re-deriving.
                        if entry.id.is_none() {
                            entry.id = Some(record.id.clone());
                            spec_changed = true;
                        }
                        rows.push(SyncRow {
                            id: record.id.clone(),
                            action: "awaiting-trust".to_string(),
                            detail: format!(
                                "installed {} (untrusted) — review, then `niu plugin trust {}`",
                                record.version, record.id
                            ),
                        });
                        registry = read_source_registry();
                        record
                    }
                    Err(err) => {
                        rows.push(SyncRow {
                            id: entry.id.clone().unwrap_or_else(|| entry.target.clone()),
                            action: "failed".to_string(),
                            detail: err.to_string(),
                        });
                        continue;
                    }
                }
            }
        };
        declared_ids.push(record.id.clone());

        if !record.trusted {
            if !rows.iter().any(|row| row.id == record.id) {
                rows.push(SyncRow {
                    id: record.id.clone(),
                    action: "awaiting-trust".to_string(),
                    detail: format!(
                        "installed but untrusted — review, then `niu plugin trust {}`",
                        record.id
                    ),
                });
            }
            continue;
        }
        if !record.path.is_dir() {
            rows.push(SyncRow {
                id: record.id.clone(),
                action: "degraded".to_string(),
                detail: format!(
                    "tree missing — repair with `niu plugin restore {}` (native fallback active)",
                    record.id
                ),
            });
            continue;
        }

        // Materialize the spec selection (idempotent; hand-added entries
        // preserved) and persist the materialized state into the lock.
        match assets::materialize_spec_selection(&record, entry) {
            Ok(materialized) => {
                let mut updated = record.clone();
                updated.spec_enabled = Some(materialized.spec_enabled.unwrap_or_default());
                updated.spec_theme = materialized.spec_theme;
                if updated.spec_enabled != record.spec_enabled
                    || updated.spec_theme != record.spec_theme
                {
                    write_record(&mut registry, updated);
                }
                let action = materialized.action;
                rows.push(SyncRow {
                    id: record.id.clone(),
                    action,
                    detail: materialized.detail,
                });
            }
            Err(err) => rows.push(SyncRow {
                id: record.id.clone(),
                action: "failed".to_string(),
                detail: err.to_string(),
            }),
        }
    }
    if spec_changed {
        spec::save_spec(&spec)?;
    }

    // Installed but not declared: suggest (or, with --prune, remove).
    let mut undeclared: Vec<String> = Vec::new();
    for record in read_source_registry() {
        if declared_ids.contains(&record.id) {
            continue;
        }
        undeclared.push(record.id.clone());
        if record.spec_enabled.is_some() {
            // The spec owned this source and no longer declares it: drop
            // its managed block (spec is the truth) but keep the tree.
            let _ = remove_activation_block(&record);
            let mut registry_now = read_source_registry();
            if let Some(stored) = registry_now
                .iter_mut()
                .find(|candidate| candidate.id == record.id)
            {
                stored.spec_enabled = None;
                stored.spec_theme = None;
                sources::write_source_registry(&registry_now)?;
            }
            rows.push(SyncRow {
                id: record.id.clone(),
                action: "deactivated".to_string(),
                detail: format!(
                    "no longer declared — activation dropped; tree kept \
                     (`niu plugin source remove {id}` to delete)",
                    id = record.id
                ),
            });
        }
    }
    let mut pruned: Vec<SyncRow> = Vec::new();
    if options.prune {
        for id in &undeclared {
            match sources::remove_source(id) {
                Ok(path) => pruned.push(SyncRow {
                    id: id.clone(),
                    action: "removed".to_string(),
                    detail: format!("pruned (tree {} deleted)", path.display()),
                }),
                Err(err) => pruned.push(SyncRow {
                    id: id.clone(),
                    action: "failed".to_string(),
                    detail: err.to_string(),
                }),
            }
        }
        undeclared.clear();
    }
    rows.extend(pruned);

    let mut report = SyncReport {
        spec_present: true,
        rows,
        undeclared,
        clean: false,
    };
    report.clean = report.compute_clean();
    Ok(report)
}

fn write_record(registry: &mut Vec<SourceRecord>, updated: SourceRecord) {
    match registry.iter_mut().find(|record| record.id == updated.id) {
        Some(slot) => *slot = updated,
        None => registry.push(updated),
    }
    if let Err(err) = sources::write_source_registry(registry) {
        log::warn!("failed to persist spec state: {err}");
    }
}

/// Drop a source's managed rc activation (and bash-it enabled/ entries)
/// without touching the rest of the tree.
fn remove_activation_block(record: &SourceRecord) -> anyhow::Result<()> {
    assets::deactivate_block(record);
    Ok(())
}

/// `niu plugin update` (no id): the vim-plug `:PlugUpdate` move — refetch
/// every git-origin source at its recorded ref's tip (the pre-14.6
/// meaning of bare `niu plugin sync`).
pub fn update_all_to_ref_tip() -> Vec<sources::SourceSyncOutcome> {
    sources::sync_sources()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::PROCESS_STATE_LOCK;
    use std::fs;
    use std::path::{Path, PathBuf};

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

    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niu-sync-{}-{}-{}",
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

    struct Sandbox {
        _home: EnvGuard,
        _userprofile: EnvGuard,
        _sources: EnvGuard,
        _spec: EnvGuard,
        temp: PathBuf,
    }

    fn sandbox(label: &str) -> Sandbox {
        let temp = unique_temp_dir(label);
        let home = temp.join("home");
        fs::create_dir_all(&home).unwrap();
        Sandbox {
            _home: EnvGuard::set("HOME", &home.to_string_lossy()),
            _userprofile: EnvGuard::set("USERPROFILE", &home.to_string_lossy()),
            _sources: EnvGuard::set(
                "NIU_PLUGIN_SOURCES_ROOT",
                &temp.join("sources").to_string_lossy(),
            ),
            _spec: EnvGuard::set(
                "NIU_PLUGIN_SPEC",
                &home.join(".niubash/plugins.toml").to_string_lossy(),
            ),
            temp,
        }
    }

    fn write_omb_fixture(root: &Path) {
        fs::create_dir_all(root.join("themes/agnoster")).unwrap();
        fs::create_dir_all(root.join("plugins/git")).unwrap();
        fs::create_dir_all(root.join("plugins/npm")).unwrap();
        fs::write(
            root.join("oh-my-bash.sh"),
            "#!/usr/bin/env bash\ncase $- in *i*) ;; *) return;; esac\n",
        )
        .unwrap();
        fs::write(
            root.join("themes/agnoster/agnoster.theme.sh"),
            "PS1='agnoster> '\n",
        )
        .unwrap();
        fs::write(
            root.join("plugins/git/git.plugin.sh"),
            "alias gg='git status'\n",
        )
        .unwrap();
        fs::write(
            root.join("plugins/npm/npm.plugin.sh"),
            "alias ni='npm install'\n",
        )
        .unwrap();
    }

    fn write_wild_fixture(root: &Path) {
        fs::write(root.join("pre.sh"), "pre_fun() { echo pre; }\n").unwrap();
    }

    fn rc_text() -> String {
        fs::read_to_string(assets::rc_file()).unwrap_or_default()
    }

    fn spec_sources() -> Vec<SpecSource> {
        spec::load_spec().unwrap().unwrap_or_default().sources
    }

    #[test]
    fn sync_installs_declared_sources_and_materializes_idempotently() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let holder = unique_temp_dir("omb-holder");
        let origin = holder.join("oh-my-fixture");
        fs::create_dir_all(&origin).unwrap();
        write_omb_fixture(&origin);
        let box_ = sandbox("omb-idempotent");

        // Hand-written spec: declare the local tree with two plugins and a
        // theme. Target is a path (offline), ref left out.
        spec::save_spec(&PluginSpec {
            schema: Some(spec::PLUGIN_SPEC_SCHEMA.to_string()),
            sources: vec![SpecSource {
                target: origin.to_string_lossy().into_owned(),
                id: Some("oh-my-bash".to_string()),
                kind: None,
                ref_name: None,
                theme: Some("agnoster".to_string()),
                enable: vec!["git".to_string(), "npm".to_string()],
            }],
        })
        .unwrap();

        // Sync 1: installs (fetch gate), lands untrusted.
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert_eq!(report.rows.len(), 1, "{:?}", report.rows);
        assert_eq!(report.rows[0].action, "awaiting-trust", "{:?}", report.rows);
        assert!(
            report.rows[0].detail.contains("niu plugin trust"),
            "{:?}",
            report.rows
        );
        assert!(rc_text().is_empty(), "untrusted sources activate nothing");

        sources::trust_source("oh-my-bash").unwrap();

        // Sync 2: materializes the spec into the managed block.
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert_eq!(report.rows[0].action, "activated", "{:?}", report.rows);
        let rc = rc_text();
        assert!(rc.contains("OSH_THEME='agnoster'"), "{rc}");
        assert!(rc.contains("plugins=('git' 'npm')"), "{rc}");
        assert!(!report.clean, "{:?}", report);

        // Sync 3 (idempotency, pinned by integration too): byte-identical
        // rc, unchanged rows, clean report.
        let before = rc_text();
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert_eq!(report.rows[0].action, "unchanged", "{:?}", report.rows);
        assert_eq!(rc_text(), before, "rc must be byte-identical");
        assert!(report.clean, "{:?}", report);

        // Removing an entry from the spec drops it from the rc block, and
        // hand-added entries survive: simulate a hand edit by adding 'g2'
        // directly to the rc array, then syncing after removing npm.
        let hand = before.replace("plugins=('git' 'npm')", "plugins=('git' 'npm' 'g2')");
        fs::write(assets::rc_file(), &hand).unwrap();
        let mut next_spec = spec::load_spec().unwrap().unwrap();
        next_spec.sources[0].enable.retain(|name| name != "npm");
        spec::save_spec(&next_spec).unwrap();
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert_eq!(report.rows[0].action, "activated", "{:?}", report.rows);
        let rc = rc_text();
        assert!(
            rc.contains("plugins=('git' 'g2')"),
            "spec-removed npm is dropped, hand-added g2 survives: {rc}"
        );
        assert!(rc.contains("OSH_THEME='agnoster'"), "{rc}");

        let _ = fs::remove_dir_all(&holder);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn sync_undeclared_sources_are_suggested_then_pruned() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let holder = unique_temp_dir("prune-holder");
        let origin = holder.join("wildy");
        fs::create_dir_all(&origin).unwrap();
        write_wild_fixture(&origin);
        let box_ = sandbox("prune");

        // Imperative install (no spec): legacy mode reports undeclared.
        sources::add_source(SourceInstallRequest {
            adapter: None,
            origin: origin.to_string_lossy().into_owned(),
            ref_name: None,
            commit: None,
            expected_checksum: None,
            id: None,
            entry: None,
        })
        .unwrap();
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert!(!report.spec_present, "{:?}", report);
        assert_eq!(report.undeclared, ["wildy"], "{:?}", report);

        // An empty spec exists → same suggestion, no auto-delete.
        spec::save_spec(&PluginSpec::default()).unwrap();
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert!(report.spec_present, "{:?}", report);
        assert_eq!(report.undeclared, ["wildy"], "{:?}", report);
        assert!(
            box_.temp.join("sources/wildy/pre.sh").is_file(),
            "undeclared trees are never auto-deleted"
        );

        // --prune is the explicit confirm.
        let report = sync_spec(SyncOptions { prune: true }).unwrap();
        assert!(report.undeclared.is_empty(), "{:?}", report);
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.id == "wildy" && row.action == "removed"),
            "{:?}",
            report.rows
        );
        assert!(!box_.temp.join("sources/wildy").exists());
        assert!(read_source_registry().is_empty());

        let _ = fs::remove_dir_all(&holder);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn spec_declared_source_removal_drops_the_block_and_keeps_the_tree() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let holder = unique_temp_dir("drop-holder");
        let origin = holder.join("oh-my-fixture");
        fs::create_dir_all(&origin).unwrap();
        write_omb_fixture(&origin);
        let box_ = sandbox("drop");

        spec::save_spec(&PluginSpec {
            schema: None,
            sources: vec![SpecSource {
                target: origin.to_string_lossy().into_owned(),
                id: Some("oh-my-bash".to_string()),
                kind: None,
                ref_name: None,
                theme: None,
                enable: vec!["git".to_string()],
            }],
        })
        .unwrap();
        sync_spec(SyncOptions::default()).unwrap();
        sources::trust_source("oh-my-bash").unwrap();
        sync_spec(SyncOptions::default()).unwrap();
        assert!(rc_text().contains("plugins=('git')"), "{}", rc_text());

        // Remove the declaration → sync drops the activation, keeps tree.
        spec::save_spec(&PluginSpec::default()).unwrap();
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.id == "oh-my-bash" && row.action == "deactivated"),
            "{:?}",
            report.rows
        );
        assert!(!rc_text().contains("plugins="), "{}", rc_text());
        assert!(
            box_.temp.join("sources/oh-my-bash/oh-my-bash.sh").is_file(),
            "tree kept"
        );

        let _ = fs::remove_dir_all(&holder);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn sync_binds_derived_ids_back_into_the_spec() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let holder = unique_temp_dir("bind-holder");
        let origin = holder.join("prelib");
        fs::create_dir_all(&origin).unwrap();
        write_wild_fixture(&origin);
        let box_ = sandbox("bind");

        // No explicit id: the install derives one; sync persists it.
        spec::save_spec(&PluginSpec {
            schema: None,
            sources: vec![SpecSource {
                target: origin.to_string_lossy().into_owned(),
                id: None,
                kind: None,
                ref_name: None,
                theme: None,
                enable: vec!["pre.sh".to_string()],
            }],
        })
        .unwrap();
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert_eq!(report.rows[0].id, "prelib", "{:?}", report.rows);
        let sources = spec_sources();
        assert_eq!(sources[0].id.as_deref(), Some("prelib"), "{sources:?}");

        // A second sync matches the bound id (no reinstall attempt).
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert!(
            report.rows.iter().all(|row| row.action != "failed"),
            "{:?}",
            report.rows
        );
        assert_eq!(read_source_registry().len(), 1);

        let _ = fs::remove_dir_all(&holder);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn missing_spec_reports_legacy_mode() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let box_ = sandbox("legacy");
        let report = sync_spec(SyncOptions::default()).unwrap();
        assert!(!report.spec_present);
        assert!(report.rows.is_empty());
        let _ = fs::remove_dir_all(&box_.temp);
    }
}
