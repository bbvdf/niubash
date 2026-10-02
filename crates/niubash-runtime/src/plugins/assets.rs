//! Asset-level activation for trusted external sources — `niu plugin
//! list/enable/disable` operating on the *real* manager assets
//! (oh-my-bash themes/plugins/aliases/completions, bash-it components,
//! bash-completion as a whole), each through the manager's own selection
//! mechanism (§11: preserve the native layout; owner ruling 2026-10-02:
//! the external ecosystem is the first-class content).
//!
//! Activation state lives where the manager itself looks for it:
//!
//! * oh-my-bash — a managed block in `~/.niubashrc` defining
//!   `OSH_THEME`/`plugins=(…)`/`aliases=(…)`/`completions=(…)` right before
//!   the guarded loader snippet (exactly the arrays `oh-my-bash.sh`
//!   consumes; declarative, lazy.nvim-spec-style).
//! * bash-it — `enabled/<priority>---<file>` entries in the source tree
//!   (the mechanism `scripts/reloader.bash` reads); the theme variable and
//!   the guarded loader live in the managed rc block.
//! * bash-completion — one managed loader block; activation is
//!   whole-source.
//!
//! Every block keeps the adapter's existence guard, so a missing tree is a
//! silent no-op at startup (iron law 2: the fallback chain never breaks).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context as _};

use super::sources::{
    adapter_for, list_sources, read_source_registry, SelectionModel, SourceAsset, SourceAssetKind,
    SourceRecord, SourceStatus,
};
use crate::path_utils::shell_home_dir;

/// Marker pair wrapping one managed block in `~/.niubashrc`. Managed lines
/// are rewritten by niu; everything outside the markers is the user's.
fn begin_marker(id: &str) -> String {
    format!("# >>> niu source {id} (managed by `niu plugin enable/disable`) >>>")
}

fn end_marker(id: &str) -> String {
    format!("# <<< niu source {id} <<<")
}

/// The primary interactive rc file asset activation edits.
pub fn rc_file() -> PathBuf {
    shell_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".niubashrc")
}

/// One asset row in the overview, with its activation state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetRow {
    pub asset: SourceAsset,
    pub enabled: bool,
}

/// Per-source section of `niu plugin list`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceReport {
    pub status: SourceStatus,
    /// Assets with activation state (empty for degraded/untrusted rows).
    pub assets: Vec<AssetRow>,
    /// True when the managed rc block (or bash-it enabled/ entries) is in
    /// place, i.e. the source participates in startup.
    pub activated: bool,
}

/// Result of an enable/disable call: what happened and the exact undo.
#[derive(Debug, Clone)]
pub struct ActivationOutcome {
    pub summary: String,
    pub undo: String,
}

// ── Managed rc block ─────────────────────────────────────────────────────────

/// Parsed activation state inside one managed block.
#[derive(Debug, Clone, Default)]
struct BlockState {
    theme: Option<String>,
    arrays: BTreeMap<&'static str, Vec<String>>,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

/// Parse a block body back into state. Only understands the lines this
/// module writes (single-quoted words); anything unrecognized is ignored
/// and will be re-rendered canonically on the next write.
fn parse_block(body: &str, model: &SelectionModel) -> BlockState {
    const NO_ARRAYS: &[(&str, SourceAssetKind)] = &[];
    let (theme_var, arrays) = match model {
        SelectionModel::LoaderArrays { arrays, theme_var } => (Some(*theme_var), *arrays),
        SelectionModel::EnabledDir { theme_var, .. } => (Some(*theme_var), NO_ARRAYS),
        SelectionModel::WholeSource => (None, NO_ARRAYS),
    };
    let mut state = BlockState::default();
    for raw in body.lines() {
        let line = raw.trim();
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if let Some(inner) = value
            .strip_prefix('(')
            .and_then(|v| v.strip_suffix(')'))
            .map(str::trim)
        {
            // Array assignment: plugins=('a' 'b')
            let items = split_quoted_words(inner);
            if let Some((var, _)) = arrays.iter().find(|(var, _)| *var == name) {
                state.arrays.entry(var).or_default().extend(items);
            }
        } else if Some(name) == theme_var {
            // Theme assignment: OSH_THEME='name'
            let trimmed = value.trim_matches('\'').trim_matches('"');
            state.theme = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
    }
    state
}

/// Split `'a' 'b' 'c'` into words (the exact form this module writes,
/// including `'\''`-escaped quotes inside a word).
fn split_quoted_words(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        // Skip separators between words.
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        if chars[index] != '\'' {
            // Not a form we write (hand-edited); skip the token entirely.
            while index < chars.len() && !chars[index].is_whitespace() {
                index += 1;
            }
            continue;
        }
        index += 1; // opening quote
        let mut word = String::new();
        let mut closed = false;
        while index < chars.len() {
            let ch = chars[index];
            if ch == '\'' {
                // `'\''` keeps a literal quote inside the word.
                if index + 2 < chars.len()
                    && chars[index + 1] == '\\'
                    && chars[index + 2] == '\''
                    && index + 3 < chars.len()
                    && chars[index + 3] == '\''
                {
                    word.push('\'');
                    index += 4;
                    continue;
                }
                index += 1; // closing quote
                closed = true;
                break;
            }
            word.push(ch);
            index += 1;
        }
        if closed {
            words.push(word);
        } else {
            break; // unterminated word (hand-edited); stop parsing
        }
    }
    words
}

/// Render a managed block: theme line, selection arrays, then the
/// adapter's guarded loader snippet, wrapped in markers.
fn render_block(record: &SourceRecord, model: &SelectionModel, state: &BlockState) -> String {
    let adapter = adapter_for(&record.adapter).expect("adapter for a rendered block");
    let mut body = String::new();
    let theme_var = match model {
        SelectionModel::LoaderArrays { theme_var, .. } => Some(*theme_var),
        SelectionModel::EnabledDir { theme_var, .. } => Some(*theme_var),
        SelectionModel::WholeSource => None,
    };
    if let (Some(var), Some(theme)) = (theme_var, &state.theme) {
        body.push_str(&format!("{}={}\n", var, shell_quote(theme)));
    }
    if let SelectionModel::LoaderArrays { arrays, .. } = model {
        for (var, _) in arrays.iter() {
            if let Some(items) = state.arrays.get(var) {
                if !items.is_empty() {
                    let quoted: Vec<String> = items.iter().map(|i| shell_quote(i)).collect();
                    body.push_str(&format!("{}=({})\n", var, quoted.join(" ")));
                }
            }
        }
    }
    body.push_str(&adapter.loader_snippet(record));
    format!(
        "{}\n{}{}\n",
        begin_marker(&record.id),
        body,
        end_marker(&record.id)
    )
}

/// Read one managed block body from the rc (markers excluded).
fn read_managed_block(id: &str) -> Option<String> {
    let text = fs::read_to_string(rc_file()).ok()?;
    extract_block(&text, id)
}

fn extract_block(text: &str, id: &str) -> Option<String> {
    let begin = begin_marker(id);
    let end = end_marker(id);
    let start = text.lines().position(|line| line.trim() == begin)?;
    let stop = text
        .lines()
        .skip(start + 1)
        .position(|line| line.trim() == end)?
        + start
        + 1;
    let body: Vec<&str> = text
        .lines()
        .skip(start + 1)
        .take(stop - start - 1)
        .collect();
    Some(body.join("\n"))
}

/// Insert or replace a managed block in the rc file, creating the rc when
/// absent. Returns the rc path (for the outcome message).
fn write_managed_block(id: &str, block: &str) -> anyhow::Result<PathBuf> {
    let path = rc_file();
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .map(|text| text.lines().map(str::to_string).collect())
        .unwrap_or_else(|_| {
            vec!["# Niubash interactive rc - edited by you and `niu plugin`.".to_string()]
        });
    let begin = begin_marker(id);
    let end = end_marker(id);
    let block_lines: Vec<String> = block.lines().map(str::to_string).collect();
    match lines.iter().position(|line| line.trim() == begin) {
        Some(start) => {
            let stop = lines
                .iter()
                .skip(start + 1)
                .position(|line| line.trim() == end)
                .map(|offset| offset + start + 1)
                .ok_or_else(|| {
                    anyhow!("rc block for '{id}' has a begin marker but no end marker")
                })?;
            lines.splice(start..=stop, block_lines);
        }
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.extend(block_lines);
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, text)?;
    Ok(path)
}

/// Remove a managed block; returns false when there was none.
fn remove_managed_block(id: &str) -> anyhow::Result<bool> {
    let path = rc_file();
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(false);
    };
    let begin = begin_marker(id);
    let end = end_marker(id);
    let Some(start) = text.lines().position(|line| line.trim() == begin) else {
        return Ok(false);
    };
    let Some(stop) = text
        .lines()
        .skip(start + 1)
        .position(|line| line.trim() == end)
        .map(|offset| offset + start + 1)
    else {
        bail!("rc block for '{id}' has a begin marker but no end marker");
    };
    let kept: Vec<&str> = text
        .lines()
        .enumerate()
        .filter(|(index, _)| *index < start || *index > stop)
        .map(|(_, line)| line)
        .collect();
    let mut rewritten = kept.join("\n");
    rewritten.push('\n');
    fs::write(&path, rewritten)?;
    Ok(true)
}

/// Build the managed activation block for a source with a theme selected
/// (used by the setup wizard and by `niu plugin enable <theme>`).
pub fn build_theme_block(source_id: &str, theme: &str) -> Option<String> {
    let record = read_source_registry()
        .into_iter()
        .find(|record| record.id == source_id)?;
    let adapter = adapter_for(&record.adapter)?;
    let model = adapter.selection_model();
    let state = BlockState {
        theme: Some(theme.to_string()),
        arrays: BTreeMap::new(),
    };
    Some(render_block(&record, &model, &state))
}

// ── bash-it enabled/ directory (tree-side state) ─────────────────────────────

const BASH_IT_LOAD_PRIORITY_SEPARATOR: &str = "---";
const DEFAULT_LOAD_PRIORITY: u32 = 500;

/// Load priority declared by a bash-it component header
/// (`# BASH_IT_LOAD_PRIORITY: 350`), defaulting to bash-it's own 500.
fn load_priority(file: &Path) -> u32 {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let rest = line.trim().strip_prefix("# BASH_IT_LOAD_PRIORITY:")?;
                rest.trim().parse::<u32>().ok()
            })
        })
        .unwrap_or(DEFAULT_LOAD_PRIORITY)
}

fn enabled_entries(record: &SourceRecord, asset: &SourceAsset) -> Vec<PathBuf> {
    let enabled_dir = record.path.join("enabled");
    let file_name = asset
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let suffix = format!("{BASH_IT_LOAD_PRIORITY_SEPARATOR}{file_name}");
    let Ok(entries) = fs::read_dir(&enabled_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(&suffix))
        })
        .collect()
}

fn tree_enable(record: &SourceRecord, asset: &SourceAsset) -> anyhow::Result<()> {
    let enabled_dir = record.path.join("enabled");
    fs::create_dir_all(&enabled_dir)?;
    if !enabled_entries(record, asset).is_empty() {
        return Ok(()); // already enabled
    }
    let file_name = asset
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("asset path has no file name"))?;
    let target = enabled_dir.join(format!(
        "{}{}{}",
        load_priority(&asset.path),
        BASH_IT_LOAD_PRIORITY_SEPARATOR,
        file_name
    ));
    // Copy (not symlink): Windows symlinks need privileges, and the
    // reloader only globs `enabled/*.bash` either way.
    fs::copy(&asset.path, &target)
        .with_context(|| format!("failed to enable {}", asset.path.display()))?;
    Ok(())
}

fn tree_disable(record: &SourceRecord, asset: &SourceAsset) -> anyhow::Result<()> {
    for entry in enabled_entries(record, asset) {
        fs::remove_file(&entry)
            .with_context(|| format!("failed to disable {}", entry.display()))?;
    }
    Ok(())
}

// ── Overview ─────────────────────────────────────────────────────────────────

/// Compute `niu plugin list` rows: every registered source with its assets
/// and activation state (untrusted/degraded sources list with state only).
pub fn asset_overview() -> Vec<SourceReport> {
    let mut out = Vec::new();
    for status in list_sources() {
        let Some(adapter) = adapter_for(&status.record.adapter) else {
            out.push(SourceReport {
                status,
                assets: Vec::new(),
                activated: false,
            });
            continue;
        };
        if status.degraded || !status.record.trusted {
            out.push(SourceReport {
                status,
                assets: Vec::new(),
                activated: false,
            });
            continue;
        }
        let record = &status.record;
        let model = adapter.selection_model();
        let block = read_managed_block(&record.id);
        let state = block
            .as_deref()
            .map(|body| parse_block(body, &model))
            .unwrap_or_default();
        let assets = adapter
            .list_assets(&record.path)
            .into_iter()
            .map(|asset| AssetRow {
                enabled: asset_enabled(&model, record, &state, &asset),
                asset,
            })
            .collect();
        out.push(SourceReport {
            status,
            assets,
            activated: block.is_some(),
        });
    }
    out
}

fn asset_enabled(
    model: &SelectionModel,
    record: &SourceRecord,
    state: &BlockState,
    asset: &SourceAsset,
) -> bool {
    match (model, asset.kind) {
        (SelectionModel::WholeSource, _) => false, // informational; whole-source activation
        (_, SourceAssetKind::Theme) => state.theme.as_deref() == Some(asset.name.as_str()),
        (SelectionModel::EnabledDir { .. }, _) => !enabled_entries(record, asset).is_empty(),
        (SelectionModel::LoaderArrays { arrays, .. }, kind) => {
            arrays.iter().any(|(var, array_kind)| {
                *array_kind == kind
                    && state
                        .arrays
                        .get(var)
                        .is_some_and(|items| items.iter().any(|item| item == &asset.name))
            })
        }
    }
}

// ── Enable / disable ─────────────────────────────────────────────────────────

/// Resolve an enable/disable target: a source id (`oh-my-bash`), a
/// qualified asset (`oh-my-bash/git`), or a bare asset name that must be
/// unambiguous across trusted sources.
enum Target {
    Source(SourceRecord),
    Asset {
        record: SourceRecord,
        asset: SourceAsset,
    },
}

fn resolve_target(name: &str) -> anyhow::Result<Target> {
    let records = read_source_registry();
    // Qualified form: <source-id>/<asset-name>.
    if let Some((source_id, asset_name)) = name.split_once('/') {
        if let Some(record) = records.iter().find(|record| record.id == source_id) {
            if let Some(asset) = find_asset(record, asset_name) {
                let target = Target::Asset {
                    record: record.clone(),
                    asset,
                };
                return check_gate(target, record);
            }
        }
    }
    // Source id.
    if let Some(record) = records.iter().find(|record| record.id == name) {
        return check_gate(Target::Source(record.clone()), record);
    }
    // Bare asset name across gated (trusted, non-degraded) sources.
    let mut matches = Vec::new();
    for record in &records {
        if let Some(asset) = find_asset(record, name) {
            matches.push((record.clone(), asset));
        }
    }
    match matches.len() {
        0 => {
            let installed: Vec<String> = records.iter().map(|r| r.id.clone()).collect();
            bail!(
                "no source or asset named '{name}'; installed sources: {}",
                if installed.is_empty() {
                    "(none — start with `niu plugin discover`)".to_string()
                } else {
                    installed.join(", ")
                }
            )
        }
        1 => check_gate(
            Target::Asset {
                record: matches[0].0.clone(),
                asset: matches[0].1.clone(),
            },
            &matches[0].0,
        ),
        _ => {
            let options: Vec<String> = matches
                .iter()
                .map(|(record, asset)| format!("{}/{}", record.id, asset.name))
                .collect();
            bail!(
                "'{name}' exists in multiple sources — pick one: {}",
                options.join(", ")
            )
        }
    }
}

fn find_asset(record: &SourceRecord, name: &str) -> Option<SourceAsset> {
    let adapter = adapter_for(&record.adapter)?;
    adapter
        .list_assets(&record.path)
        .into_iter()
        .find(|asset| asset.name == name)
}

/// The execution gate applies to activation too: untrusted or degraded
/// sources contribute nothing (§12.2/§11.4).
fn check_gate(target: Target, record: &SourceRecord) -> anyhow::Result<Target> {
    if !record.path.is_dir() {
        bail!(
            "source '{}' is degraded (tree missing); restore it first: `niu plugin restore {}`",
            record.id,
            record.id
        );
    }
    if !record.trusted {
        bail!(
            "source '{}' is installed but untrusted; review and trust it first: \
             `niu plugin trust {}`",
            record.id,
            record.id
        );
    }
    Ok(target)
}

/// `niu plugin enable <target>`: activate a source (managed loader block)
/// or a single asset through its manager's own selection mechanism.
pub fn enable(name: &str) -> anyhow::Result<ActivationOutcome> {
    let target = resolve_target(name)?;
    match target {
        Target::Source(record) => {
            let adapter = adapter_for(&record.adapter)
                .ok_or_else(|| anyhow!("unknown adapter '{}'", record.adapter))?;
            let model = adapter.selection_model();
            // Preserve any existing selection (theme/arrays) in the block.
            let state = read_managed_block(&record.id)
                .as_deref()
                .map(|body| parse_block(body, &model))
                .unwrap_or_default();
            let block = render_block(&record, &model, &state);
            let rc = write_managed_block(&record.id, &block)?;
            Ok(ActivationOutcome {
                summary: format!(
                    "source '{}' activated — guarded loader added to {}",
                    record.id,
                    rc.display()
                ),
                undo: format!("niu plugin disable {}", record.id),
            })
        }
        Target::Asset { record, asset } => {
            let adapter = adapter_for(&record.adapter)
                .ok_or_else(|| anyhow!("unknown adapter '{}'", record.adapter))?;
            let model = adapter.selection_model();
            match (&model, asset.kind) {
                (SelectionModel::WholeSource, _) => bail!(
                    "'{}' activates as a whole source; run `niu plugin enable {}` instead",
                    record.id,
                    record.id
                ),
                (model, SourceAssetKind::Theme) => {
                    let theme_var = match model {
                        SelectionModel::LoaderArrays { theme_var, .. }
                        | SelectionModel::EnabledDir { theme_var, .. } => *theme_var,
                        SelectionModel::WholeSource => unreachable!(),
                    };
                    let state = current_state(&record, &model);
                    let mut state = state;
                    state.theme = Some(asset.name.clone());
                    let block = render_block(&record, &model, &state);
                    let rc = write_managed_block(&record.id, &block)?;
                    Ok(ActivationOutcome {
                        summary: format!(
                            "theme '{}' ({}) enabled via {} in {}",
                            asset.name,
                            record.id,
                            theme_var,
                            rc.display()
                        ),
                        undo: format!("niu plugin disable {}", asset.name),
                    })
                }
                (SelectionModel::EnabledDir { .. }, _) => {
                    tree_enable(&record, &asset)?;
                    // The loader block must exist for the entry to load.
                    ensure_block(&record, &model)?;
                    Ok(ActivationOutcome {
                        summary: format!(
                            "'{}' ({}) enabled — {} now loads it",
                            asset.name, record.id, record.id
                        ),
                        undo: format!("niu plugin disable {}", asset.name),
                    })
                }
                (SelectionModel::LoaderArrays { arrays, .. }, kind) => {
                    let var = arrays
                        .iter()
                        .find(|(_, array_kind)| *array_kind == kind)
                        .map(|(var, _)| *var)
                        .ok_or_else(|| {
                            anyhow!(
                                "adapter '{}' has no selection array for {kind:?}",
                                record.adapter
                            )
                        })?;
                    let mut state = current_state(&record, &model);
                    let items = state.arrays.entry(var).or_default();
                    if !items.iter().any(|item| item == &asset.name) {
                        items.push(asset.name.clone());
                    }
                    let block = render_block(&record, &model, &state);
                    let rc = write_managed_block(&record.id, &block)?;
                    Ok(ActivationOutcome {
                        summary: format!(
                            "'{}' ({}) added to {}=() in {}",
                            asset.name,
                            record.id,
                            var,
                            rc.display()
                        ),
                        undo: format!("niu plugin disable {}", asset.name),
                    })
                }
            }
        }
    }
}

/// `niu plugin disable <target>`: remove the source's loader block, or
/// drop one asset from the selection (theme line, rc array, or bash-it
/// enabled/ entry).
pub fn disable(name: &str) -> anyhow::Result<ActivationOutcome> {
    let target = resolve_target(name)?;
    match target {
        Target::Source(record) => {
            if remove_managed_block(&record.id)? {
                Ok(ActivationOutcome {
                    summary: format!(
                        "source '{}' deactivated — loader block removed from {} \
                         (the installed tree is untouched)",
                        record.id,
                        rc_file().display()
                    ),
                    undo: format!("niu plugin enable {}", record.id),
                })
            } else {
                Ok(ActivationOutcome {
                    summary: format!(
                        "source '{}' was not activated (no block to remove)",
                        record.id
                    ),
                    undo: format!("niu plugin enable {}", record.id),
                })
            }
        }
        Target::Asset { record, asset } => {
            let adapter = adapter_for(&record.adapter)
                .ok_or_else(|| anyhow!("unknown adapter '{}'", record.adapter))?;
            let model = adapter.selection_model();
            match (&model, asset.kind) {
                (SelectionModel::WholeSource, _) => bail!(
                    "'{}' activates as a whole source; run `niu plugin disable {}` instead",
                    record.id,
                    record.id
                ),
                (_, SourceAssetKind::Theme) => {
                    let mut state = current_state(&record, &model);
                    state.theme = None;
                    let block = render_block(&record, &model, &state);
                    let rc = write_managed_block(&record.id, &block)?;
                    Ok(ActivationOutcome {
                        summary: format!(
                            "theme '{}' disabled — prompt falls back to the niubash default ({})",
                            asset.name,
                            rc.display()
                        ),
                        undo: format!("niu plugin enable {}", asset.name),
                    })
                }
                (SelectionModel::EnabledDir { .. }, _) => {
                    tree_disable(&record, &asset)?;
                    Ok(ActivationOutcome {
                        summary: format!("'{}' ({}) disabled", asset.name, record.id),
                        undo: format!("niu plugin enable {}", asset.name),
                    })
                }
                (SelectionModel::LoaderArrays { arrays, .. }, kind) => {
                    let var = arrays
                        .iter()
                        .find(|(_, array_kind)| *array_kind == kind)
                        .map(|(var, _)| *var)
                        .ok_or_else(|| {
                            anyhow!(
                                "adapter '{}' has no selection array for {kind:?}",
                                record.adapter
                            )
                        })?;
                    let mut state = current_state(&record, &model);
                    if let Some(items) = state.arrays.get_mut(var) {
                        items.retain(|item| item != &asset.name);
                    }
                    let block = render_block(&record, &model, &state);
                    let rc = write_managed_block(&record.id, &block)?;
                    Ok(ActivationOutcome {
                        summary: format!(
                            "'{}' ({}) removed from {}=() in {}",
                            asset.name,
                            record.id,
                            var,
                            rc.display()
                        ),
                        undo: format!("niu plugin enable {}", asset.name),
                    })
                }
            }
        }
    }
}

fn current_state(record: &SourceRecord, model: &SelectionModel) -> BlockState {
    read_managed_block(&record.id)
        .as_deref()
        .map(|body| parse_block(body, model))
        .unwrap_or_default()
}

fn ensure_block(record: &SourceRecord, model: &SelectionModel) -> anyhow::Result<()> {
    if read_managed_block(&record.id).is_none() {
        let state = BlockState::default();
        let block = render_block(record, model, &state);
        write_managed_block(&record.id, &block)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::PROCESS_STATE_LOCK;
    use std::fs;

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
            "niu-assets-{}-{}-{}",
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

    fn write_omb_fixture(root: &Path) {
        fs::create_dir_all(root.join("themes/agnoster")).unwrap();
        fs::create_dir_all(root.join("plugins/git")).unwrap();
        fs::create_dir_all(root.join("aliases")).unwrap();
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
            root.join("aliases/cargo.aliases.sh"),
            "alias cb='cargo build'\n",
        )
        .unwrap();
    }

    fn write_bash_it_fixture(root: &Path) {
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("plugins/available")).unwrap();
        fs::create_dir_all(root.join("completion/available")).unwrap();
        fs::create_dir_all(root.join("themes/demox")).unwrap();
        fs::write(
            root.join("bash_it.sh"),
            "#!/usr/bin/env bash\nfor _f in \"$BASH_IT/enabled\"/*.bash; do [ -r \"$_f\" ] && . \"$_f\"; done\nunset _f\n",
        )
        .unwrap();
        fs::write(root.join("lib/composure.bash"), "# composure\n").unwrap();
        fs::write(
            root.join("plugins/available/base.plugin.bash"),
            "# BASH_IT_LOAD_PRIORITY: 350\n_base_fn() { :; }\n",
        )
        .unwrap();
        fs::write(
            root.join("completion/available/docker.completion.bash"),
            "# docker completion\n",
        )
        .unwrap();
        fs::write(
            root.join("themes/demox/demox.theme.bash"),
            "PS1='demox> '\n",
        )
        .unwrap();
    }

    fn write_bash_completion_fixture(root: &Path) {
        fs::create_dir_all(root.join("completions")).unwrap();
        fs::write(
            root.join("bash_completion"),
            "# stub\nBASH_COMPLETION_STUB=1\n",
        )
        .unwrap();
        fs::write(root.join("completions/git.bash"), "# git completion\n").unwrap();
    }

    struct Sandbox {
        _home: EnvGuard,
        _userprofile: EnvGuard,
        _sources: EnvGuard,
        temp: PathBuf,
    }

    /// Isolated HOME + sources root so enable/disable never touch the real
    /// `~/.niubashrc` or `~/.niubash/sources`.
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
            temp,
        }
    }

    fn install(origin: &Path) {
        super::super::sources::add_source(super::super::sources::SourceInstallRequest {
            adapter: None,
            origin: origin.to_string_lossy().into_owned(),
            ref_name: None,
            commit: None,
            expected_checksum: None,
        })
        .expect("fixture add");
    }

    fn rc_text() -> String {
        fs::read_to_string(rc_file()).unwrap_or_default()
    }

    #[test]
    fn omb_enable_disable_round_trip_edits_rc_arrays() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let origin = unique_temp_dir("omb-origin");
        write_omb_fixture(&origin);
        let box_ = sandbox("omb-roundtrip");
        install(&origin);

        // Gate: untrusted sources cannot activate.
        let err = enable("oh-my-bash").expect_err("untrusted enable must fail");
        assert!(err.to_string().contains("untrusted"), "{err}");
        assert!(rc_text().is_empty(), "no rc written on refusal");
        super::super::sources::trust_source("oh-my-bash").unwrap();

        // Source-level enable writes the guarded loader block.
        enable("oh-my-bash").expect("source enable");
        let rc = rc_text();
        assert!(rc.contains(">>> niu source oh-my-bash"), "{rc}");
        assert!(rc.contains("if [ -r "), "{rc}");
        assert!(rc.contains(". \"$OSH/oh-my-bash.sh\""), "{rc}");
        assert!(!rc.contains("OSH_THEME"), "no theme forced: {rc}");

        // Asset-level: plugin/alias/theme through the manager's own knobs.
        enable("git").expect("plugin enable");
        enable("cargo").expect("alias enable");
        enable("agnoster").expect("theme enable");
        let rc = rc_text();
        assert!(rc.contains("OSH_THEME='agnoster'"), "{rc}");
        assert!(rc.contains("plugins=('git')"), "{rc}");
        assert!(rc.contains("aliases=('cargo')"), "{rc}");

        // Overview reflects the state.
        let overview = asset_overview();
        let report = overview
            .iter()
            .find(|r| r.status.record.id == "oh-my-bash")
            .unwrap();
        assert!(report.activated);
        let is_on = |name: &str| {
            report
                .assets
                .iter()
                .find(|row| row.asset.name == name)
                .unwrap()
                .enabled
        };
        assert!(is_on("git"));
        assert!(is_on("cargo"));
        assert!(is_on("agnoster"));

        // Disables reverse each piece; unknown names explain.
        disable("git").expect("plugin disable");
        let rc = rc_text();
        assert!(!rc.contains("plugins=('git')"), "{rc}");
        disable("agnoster").expect("theme disable");
        assert!(!rc_text().contains("OSH_THEME="), "{}", rc_text());

        let err = enable("nope").expect_err("unknown target");
        assert!(
            err.to_string().contains("no source or asset named 'nope'"),
            "{err}"
        );

        // Source disable removes the block, keeps the tree.
        disable("oh-my-bash").expect("source disable");
        let rc = rc_text();
        assert!(!rc.contains(">>> niu source oh-my-bash"), "{rc}");
        assert!(
            box_.temp.join("sources/oh-my-bash/oh-my-bash.sh").is_file(),
            "tree untouched"
        );
        let _ = fs::remove_dir_all(&origin);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn rc_lines_outside_the_managed_block_survive_edits() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let origin = unique_temp_dir("omb-user-rc");
        write_omb_fixture(&origin);
        let box_ = sandbox("omb-user-rc");
        install(&origin);
        super::super::sources::trust_source("oh-my-bash").unwrap();

        // A user-written rc keeps its content through enable/disable.
        fs::write(
            rc_file(),
            "# my stuff\nexport MY_VAR=1\nalias ll='ls -la'\n",
        )
        .unwrap();
        enable("oh-my-bash").unwrap();
        enable("git").unwrap();
        let rc = rc_text();
        assert!(rc.contains("# my stuff"), "{rc}");
        assert!(rc.contains("export MY_VAR=1"), "{rc}");
        assert!(rc.contains("alias ll='ls -la'"), "{rc}");
        disable("oh-my-bash").unwrap();
        let rc = rc_text();
        assert!(rc.contains("# my stuff"), "{rc}");
        assert!(rc.contains("export MY_VAR=1"), "{rc}");
        assert!(!rc.contains("plugins="), "{rc}");
        let _ = fs::remove_dir_all(&origin);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn bash_it_enable_uses_enabled_dir_and_theme_var() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let origin = unique_temp_dir("bit-origin");
        write_bash_it_fixture(&origin);
        let box_ = sandbox("bash-it");
        install(&origin);
        super::super::sources::trust_source("bash-it").unwrap();

        enable("base").expect("plugin enable");
        let entry = box_
            .temp
            .join("sources/bash-it/enabled/350---base.plugin.bash");
        assert!(entry.is_file(), "enabled/ entry with declared priority");
        let rc = rc_text();
        assert!(rc.contains("BASH_IT="), "{rc}");
        assert!(rc.contains(". \"$BASH_IT/bash_it.sh\""), "{rc}");

        enable("demox").expect("theme enable");
        assert!(rc_text().contains("BASH_IT_THEME='demox'"), "{}", rc_text());

        // Completion from the completion/available dir.
        enable("docker").expect("completion enable");
        assert!(
            box_.temp
                .join("sources/bash-it/enabled")
                .read_dir()
                .unwrap()
                .count()
                >= 2,
            "docker enabled entry exists"
        );

        disable("base").expect("plugin disable");
        assert!(!entry.exists(), "enabled/ entry removed");

        let _ = fs::remove_dir_all(&origin);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn bash_completion_is_whole_source_activation() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let origin = unique_temp_dir("bc-origin");
        write_bash_completion_fixture(&origin);
        let box_ = sandbox("bash-completion");
        install(&origin);
        super::super::sources::trust_source("bash-completion").unwrap();

        enable("bash-completion").expect("whole-source enable");
        let rc = rc_text();
        assert!(rc.contains(". \"${NIU_PLUGIN_SOURCES_ROOT:-$HOME/.niubash/sources}/bash-completion/bash_completion\""), "{rc}");
        // Individual completions are informational only.
        let err = enable("git").expect_err("asset enable must explain");
        assert!(
            err.to_string().contains("activates as a whole source"),
            "{err}"
        );
        let _ = fs::remove_dir_all(&origin);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn ambiguous_bare_names_require_the_qualified_form() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let omb = unique_temp_dir("amb-omb");
        let bc = unique_temp_dir("amb-bc");
        write_omb_fixture(&omb);
        write_bash_completion_fixture(&bc);
        let box_ = sandbox("ambiguous");
        install(&omb);
        install(&bc);
        super::super::sources::trust_source("oh-my-bash").unwrap();
        super::super::sources::trust_source("bash-completion").unwrap();

        // "git" exists as an OMB plugin and a bash-completion completion.
        let err = enable("git").expect_err("ambiguity must be reported");
        assert!(err.to_string().contains("multiple sources"), "{err}");
        assert!(err.to_string().contains("oh-my-bash/git"), "{err}");

        enable("oh-my-bash/git").expect("qualified enable");
        assert!(rc_text().contains("plugins=('git')"), "{}", rc_text());
        let _ = fs::remove_dir_all(&omb);
        let _ = fs::remove_dir_all(&bc);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn degraded_source_refuses_activation_with_restore_hint() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let origin = unique_temp_dir("deg-origin");
        write_omb_fixture(&origin);
        let box_ = sandbox("degraded");
        install(&origin);
        super::super::sources::trust_source("oh-my-bash").unwrap();
        fs::remove_dir_all(box_.temp.join("sources/oh-my-bash")).unwrap();

        let err = enable("oh-my-bash").expect_err("degraded enable must fail");
        assert!(err.to_string().contains("degraded"), "{err}");
        assert!(
            err.to_string().contains("niu plugin restore oh-my-bash"),
            "{err}"
        );
        let _ = fs::remove_dir_all(&origin);
        let _ = fs::remove_dir_all(&box_.temp);
    }

    #[test]
    fn quoted_word_parser_handles_escaped_quotes() {
        assert_eq!(split_quoted_words("'a' 'b'"), vec!["a", "b"]);
        // An empty quoted word is one empty element (shell semantics).
        assert_eq!(split_quoted_words("''"), vec![""]);
        assert_eq!(split_quoted_words("'it'\\''s'"), vec!["it's"]);
        // Hand-edited junk tokens are skipped, not fatal.
        assert_eq!(split_quoted_words("'a' bare 'b'"), vec!["a", "b"]);
    }
}
