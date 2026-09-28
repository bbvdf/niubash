//! First-run setup wizard.
//!
//! zsh-style minimal flow (owner decision 2026-09-27, see
//! docs/planning/wizard-redesign.md): one theme pick from the §0-layered
//! gallery (external oh-my-bash themes first, built-in themes last behind a
//! separator, "keep current" always an equal option) plus at most two plain
//! opt-in questions — extra tab completions and the niu-git hint. Every
//! question defaults to "no change", nothing is installed without an
//! explicit pick, and skipped questions leave the previous rc semantics
//! untouched. Presets stay available non-interactively via
//! `niu setup --preset <name>`; the old multi-page survey (fonts, starship,
//! wpm tool bundles, WT profile) is retired from the interactive flow.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::interactive_menu::{self, pad_display, Selection};
use crate::path_utils::shell_home_dir;
use crate::theme;

const PRIMARY_RC_FILE: &str = ".niubashrc";
const COMPAT_RC_FILE: &str = ".winuxshrc";
const SETUP_DONE_FILE: &str = ".setup-done";

/// Schema marker for the wizard answers file (`~/.niubash/wizard-answers.toml`).
const WIZARD_ANSWERS_SCHEMA: &str = "niubash:wizard-answers@0.1.0";

/// niu-git wpm package name and install command, taken read-only from the
/// niu-git repo docs (`D:/repo/niu-git` README "WPM package" section and
/// `wpm/niugit.json`, the official-index entry). The wizard only ever shows
/// this command; it runs it solely on an explicit "Install" pick.
const NIUGIT_WPM_PACKAGE: &str = "niugit";
/// Windows-only (owner directive): the wpm install form and the `wpm` string
/// itself must never appear on other platforms — gate the command const and
/// the whole niu-git machinery at compile time.
#[cfg(windows)]
const NIUGIT_INSTALL_COMMAND: &str = "wpm install niugit";

/// Packs the wizard can add to `NIU_PLUGINS` when the user opts into extra
/// tab completions; each is added only when its binary is on PATH.
const COMPLETION_PACK_CANDIDATES: &[(&str, &str)] = &[
    ("git", "git"),
    ("docker", "docker"),
    ("kubectl", "kubectl"),
    ("npm", "npm"),
];

/// Tools probed on PATH during preflight; drives the environment summary and
/// the completion-pack candidates.
const PROBED_TOOLS: &[&str] = &[
    "git", "fzf", "eza", "bat", "starship", "zoxide", "fd", "rg", "dust", "duf", "erd", "direnv",
    "kubectl", "docker", "npm", "thefuck",
];

/// Windows-only probe additions: wpm exists only on Windows — the name must
/// never surface (probe lists included) on other platforms.
#[cfg(windows)]
const PLATFORM_PROBED_TOOLS: &[&str] = &["wpm"];
#[cfg(not(windows))]
const PLATFORM_PROBED_TOOLS: &[&str] = &[];

// ── Wizard language ─────────────────────────────────────────────────────────
//
// The wizard is the one niubash surface every new user reads, so it carries a
// built-in Chinese string table alongside English — no external catalogs, no
// framework. Detection order: an explicit `NIU_LANG` override wins, then
// POSIX-style `LC_ALL`/`LANG`, then the Windows UI language. Any string
// without a translation falls back to English, and the generated rc file
// itself always stays English (it is bash code plus comments).

/// UI language for the setup wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    fn detect() -> Lang {
        if let Some(value) = std::env::var_os("NIU_LANG") {
            let value = value.to_string_lossy().to_lowercase();
            if value.starts_with("zh") {
                return Lang::Zh;
            }
            if !value.is_empty() {
                return Lang::En;
            }
        }
        for name in ["LC_ALL", "LANG"] {
            if let Some(value) = std::env::var_os(name) {
                let value = value.to_string_lossy().to_lowercase();
                if value.starts_with("zh") {
                    return Lang::Zh;
                }
                if !value.is_empty() {
                    return Lang::En;
                }
            }
        }
        if ui_language_is_chinese() {
            return Lang::Zh;
        }
        Lang::En
    }

    /// Translate a wizard string (English literal as the key). Untranslated
    /// keys fall back to the English text, so partial translations stay safe.
    fn tr<'a>(&self, en: &'a str) -> &'a str {
        match self {
            Lang::En => en,
            Lang::Zh => zh(en).unwrap_or(en),
        }
    }
}

/// True when the wizard language resolves to Chinese. Shared with the
/// interactive menu hint so the one line it owns matches the wizard around
/// it; the i18n itself stays embedded in this module.
pub(crate) fn wizard_lang_is_chinese() -> bool {
    Lang::detect() == Lang::Zh
}

/// True when the Windows user UI language is Chinese (any sublanguage).
#[cfg(windows)]
fn ui_language_is_chinese() -> bool {
    // LANG_CHINESE is the primary-language id 0x04; the low 10 bits of a
    // LANGID carry the primary language.
    const LANG_CHINESE: u32 = 0x04;
    let langid = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } as u32;
    (langid & 0x3FF) == LANG_CHINESE
}

#[cfg(not(windows))]
fn ui_language_is_chinese() -> bool {
    false
}

/// How the `{git}` prompt segment is produced — or whether starship owns the
/// whole prompt outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitBackend {
    /// Host-provided git snapshot rendered by the niubash prompt.
    #[default]
    Native,
    /// `starship module git_branch/git_status` renders just the `{git}`
    /// segment; the niubash prompt template and theme still apply.
    StarshipSegment,
    /// The bundle `starship` plugin runs `starship init bash`, which owns
    /// PS1/PROMPT_COMMAND — niubash prompt template and internal git status
    /// are disabled.
    StarshipFull,
}

/// Everything the wizard can write into `~/.niubashrc`. Presets fill this in
/// one shot (`niu setup --preset`, non-interactive); the interactive flow
/// fills only what the user explicitly picked.
///
/// Empty-string fields mean "no override": the generated rc omits the line
/// entirely, so the product defaults (and any previous rc semantics) stay in
/// charge. That is what makes wizard "skip" paths side-effect free.
#[derive(Debug, Clone, Default)]
pub struct WizardConfig {
    pub theme: String,
    pub prompt_style: String,
    pub right_prompt: String,
    pub symbol: String,
    pub cwd_style: String,
    pub prompt_enabled: bool,
    pub git_enabled: bool,
    pub git_backend: GitBackend,
    pub segment_preset: Option<String>,
    pub completion_style: String,
    pub plugins: Vec<String>,
    /// Extra `alias name='cmd'` lines appended to the generated rc.
    pub aliases: Vec<(String, String)>,
    /// When true, write `NIU_DISABLE_DEFAULT_PLUGINS=1` so exactly
    /// `plugins` loads (preset semantics). The interactive flow never sets
    /// it: opting into packs only *adds* to the inventory defaults.
    pub disable_default_plugins: bool,
    /// Source id (e.g. "oh-my-bash") when `theme` is an external-source
    /// theme; the rc activates it through the guarded source loader and the
    /// bash-compatible PS1 channel instead of the native TOML theme pair.
    pub theme_source_id: Option<String>,
}

/// The theme question's outcome. `Keep` writes no theme lines, mirroring
/// whatever is already active (product defaults on a fresh install).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ThemePick {
    Keep,
    Native { name: String },
    External { name: String, source_id: String },
}

/// One theme in the wizard gallery, tagged with its §0 layer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ThemeGalleryEntry {
    name: String,
    tier: ThemeGalleryTier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ThemeGalleryTier {
    /// `~/.niubash/themes/<name>.toml` — the user's own, always highest.
    User,
    /// Trusted external plugin-manager source (oh-my-bash) — primary body.
    External { source_id: String, adapter: String },
    /// Bundle-native TOML theme — fallback layer, retired to the end of the
    /// gallery behind the separator with the honest label.
    Builtin,
}

/// The §0-layered theme gallery: user and external themes first, built-in
/// themes last. `builtin_start` is the index into `entries` where the
/// built-in fallback tier begins (the separator position).
#[derive(Debug, Clone)]
struct ThemeGallery {
    entries: Vec<ThemeGalleryEntry>,
    builtin_start: usize,
}

/// External oh-my-bash themes from trusted sources lead the gallery; the
/// corpus (252/252 + PS1 byte parity, rubash target-ecosys) proved them, so
/// they are the default body. Built-in bundle themes follow behind the
/// separator. Same-name collisions resolve user > external > built-in.
fn theme_gallery() -> ThemeGallery {
    let mut user = Vec::new();
    let mut external = Vec::new();
    let mut builtin = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in theme::user_theme_entries() {
        if seen.insert(entry.name.to_ascii_lowercase()) {
            user.push(ThemeGalleryEntry {
                name: entry.name,
                tier: ThemeGalleryTier::User,
            });
        }
    }
    for entry in crate::plugins::sources::source_theme_entries() {
        if seen.insert(entry.name.to_ascii_lowercase()) {
            external.push(ThemeGalleryEntry {
                name: entry.name,
                tier: ThemeGalleryTier::External {
                    source_id: entry.source_id,
                    adapter: entry.adapter_display,
                },
            });
        }
    }
    for entry in crate::plugins::plugin_theme_catalog() {
        if entry.source != "bundle" {
            continue;
        }
        if seen.insert(entry.name.to_ascii_lowercase()) {
            builtin.push(ThemeGalleryEntry {
                name: entry.name,
                tier: ThemeGalleryTier::Builtin,
            });
        }
    }
    user.sort_by(|a, b| a.name.cmp(&b.name));
    external.sort_by(|a, b| a.name.cmp(&b.name));
    builtin.sort_by(|a, b| a.name.cmp(&b.name));
    let builtin_start = user.len() + external.len();
    let mut entries = user;
    entries.extend(external);
    entries.extend(builtin);
    ThemeGallery {
        entries,
        builtin_start,
    }
}

/// A curated setup preset. Built-ins ship in the binary; the oh-my-niu bundle
/// may drop additional `presets/*.toml` files that extend or override them.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub summary: String,
    pub requires_nerd_font: bool,
    pub theme: String,
    pub prompt_symbol: String,
    pub prompt_style: String,
    pub right_prompt: String,
    pub cwd_style: String,
    pub completion_style: String,
    pub packs: Vec<String>,
    /// Aliases always written into the rc.
    pub aliases: BTreeMap<String, String>,
    /// Pack name -> binary that must exist on PATH for the pack to enable.
    pub conditional_packs: BTreeMap<String, String>,
    /// Binary -> aliases written only when the binary is on PATH.
    pub conditional_aliases: BTreeMap<String, BTreeMap<String, String>>,
    /// Pre-selected answer for the starship question when starship is found.
    pub starship_default: bool,
}

impl Default for Preset {
    fn default() -> Self {
        Preset {
            name: String::new(),
            summary: String::new(),
            requires_nerd_font: false,
            theme: "classic".into(),
            prompt_symbol: ">".into(),
            prompt_style: "minimal".into(),
            right_prompt: "off".into(),
            cwd_style: "home".into(),
            completion_style: "ide".into(),
            packs: Vec::new(),
            aliases: BTreeMap::new(),
            conditional_packs: BTreeMap::new(),
            conditional_aliases: BTreeMap::new(),
            starship_default: false,
        }
    }
}

/// Environment facts collected once, before any question is asked.
struct EnvProbe {
    windows_terminal: bool,
    mintty_hint: bool,
    nerd_font: bool,
    command_links: bool,
    /// Names from `PROBED_TOOLS` that resolved on PATH.
    tools: BTreeSet<String>,
}

impl EnvProbe {
    fn collect() -> Self {
        let mut tools: BTreeSet<String> = PROBED_TOOLS
            .iter()
            .chain(PLATFORM_PROBED_TOOLS.iter())
            .filter(|tool| on_path(tool))
            .map(|tool| tool.to_string())
            .collect();
        // Windows-only: wpm is also usable through `winuxcmd.exe wpm`
        // without a command link. The name must never surface elsewhere.
        #[cfg(windows)]
        if wpm_available() {
            tools.insert("wpm".to_string());
        }
        EnvProbe {
            windows_terminal: std::env::var_os("WT_SESSION").is_some(),
            mintty_hint: std::env::var_os("MSYSTEM").is_some()
                || std::env::var("TERM_PROGRAM")
                    .map(|v| v.eq_ignore_ascii_case("mintty"))
                    .unwrap_or(false),
            nerd_font: crate::fonts::nerd_font_installed(),
            command_links: crate::winuxcmd::command_links_ready(),
            tools,
        }
    }

    fn on_path(&self, tool: &str) -> bool {
        self.tools.contains(tool)
    }

    fn print_summary(&self, lang: Lang) {
        let terminal = if self.windows_terminal {
            "Windows Terminal"
        } else {
            lang.tr("classic console")
        };
        let tools = if self.tools.is_empty() {
            lang.tr("none detected").to_string()
        } else {
            self.tools.iter().cloned().collect::<Vec<_>>().join(" ")
        };
        let label = |en: &str| pad_display(lang.tr(en), 14);
        println!();
        println!("  \u{1f50d}  {}", lang.tr("Environment"));
        println!("  \u{2502}  {} {}", label("terminal"), terminal);
        println!(
            "  \u{2502}  {} {}",
            label("nerd font"),
            if self.nerd_font {
                lang.tr("detected")
            } else {
                lang.tr("not found")
            }
        );
        println!(
            "  \u{2502}  {} {}",
            label("command links"),
            if self.command_links {
                lang.tr("ready")
            } else {
                lang.tr("missing")
            }
        );
        println!("  \u{2502}  {} {}", label("tools"), tools);
    }
}

/// Interactive question driver with fast-forward and abort handling.
///
/// Esc on any question fast-forwards: every remaining question silently takes
/// its default and the flow lands on the summary. Ctrl-C aborts the wizard.
struct WizardIo {
    interactive: bool,
    fast_forward: bool,
}

impl WizardIo {
    fn new(interactive: bool) -> Self {
        WizardIo {
            interactive,
            fast_forward: false,
        }
    }

    fn choice(
        &mut self,
        label: &str,
        default_idx: usize,
        options: &[&str],
        help: &str,
    ) -> Option<usize> {
        self.choice_inner(label, default_idx, options, help, None)
    }

    fn choice_preview(
        &mut self,
        label: &str,
        default_idx: usize,
        options: &[&str],
        help: &str,
        preview: &dyn Fn(usize) -> Vec<String>,
    ) -> Option<usize> {
        self.choice_inner(label, default_idx, options, help, Some(preview))
    }

    fn choice_inner(
        &mut self,
        label: &str,
        default_idx: usize,
        options: &[&str],
        help: &str,
        preview: Option<&dyn Fn(usize) -> Vec<String>>,
    ) -> Option<usize> {
        if !self.interactive || self.fast_forward || options.is_empty() {
            return Some(default_idx.min(options.len().saturating_sub(1)));
        }
        let selection = match preview {
            Some(pv) => {
                interactive_menu::interactive_choice_ex(label, options, default_idx, help, Some(pv))
            }
            None => interactive_menu::interactive_choice(label, options, default_idx, help),
        };
        match selection {
            Selection::Confirmed(idx) => Some(idx),
            Selection::UseDefault => {
                self.fast_forward = true;
                println!(
                    "  \x1b[2m{} → {}\x1b[0m",
                    label.trim(),
                    options[default_idx]
                );
                Some(default_idx.min(options.len().saturating_sub(1)))
            }
            Selection::Abort => None,
        }
    }

    /// The final Apply/Cancel gate. Unlike ordinary questions this never
    /// honours fast-forward: Esc fast-forward still stops here for an
    /// explicit answer, and Esc *on* this prompt means Cancel.
    fn confirm(&mut self, label: &str, options: &[&str]) -> Option<usize> {
        if !self.interactive || options.is_empty() {
            return Some(0);
        }
        match interactive_menu::interactive_choice(label, options, 0, "") {
            Selection::Confirmed(idx) => Some(idx),
            Selection::UseDefault => Some(options.len() - 1),
            Selection::Abort => None,
        }
    }
}

/// Localized Ctrl-C abort note for the wizard. A free function (not a
/// method) because the `ask!` macro's hygiene rules hide caller locals.
fn print_cancelled_note() {
    println!();
    println!(
        "  \u{1f6d1}  {}",
        Lang::detect().tr("Setup cancelled \u{2014} nothing was written.")
    );
}

/// `format!` for translated templates: substitutes `{}` placeholders left to
/// right. `format!` requires a literal format string, so dynamic translated
/// templates go through this helper instead.
fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = template.to_string();
    for arg in args {
        match out.find("{}") {
            Some(pos) => out.replace_range(pos..pos + 2, &arg.to_string()),
            None => break,
        }
    }
    out
}

/// Unwrap an `Option` answer; `None` means Ctrl-C — stop the wizard cleanly.
macro_rules! ask {
    ($call:expr) => {
        match $call {
            Some(value) => value,
            None => {
                print_cancelled_note();
                return Ok(());
            }
        }
    };
}

/// Returns `true` if the user has never run the setup wizard before
/// (i.e. no primary/compat rc file and no setup marker). Legacy/managed TOML
/// metadata no longer blocks first-run rc onboarding.
pub fn is_first_run() -> bool {
    let home = setup_home_dir();
    for name in [PRIMARY_RC_FILE, COMPAT_RC_FILE] {
        if home.join(name).is_file() {
            return false;
        }
    }
    !home.join(".niubash").join(SETUP_DONE_FILE).is_file()
}

/// Run the interactive setup wizard.
///
/// Prints a welcome banner, asks the user a few questions with defaults,
/// writes `~/.niubashrc`, and creates the `.setup-done` marker.
pub fn run_wizard() -> anyhow::Result<()> {
    run_wizard_inner(false)
}

/// Re-run the setup wizard even if the user already has a startup rc.
pub fn rerun_wizard() -> anyhow::Result<()> {
    run_wizard_inner(true)
}

/// Render the logo to lines and print it alongside welcome text.
fn display_welcome_side_by_side(reconfigure: bool, lang: Lang) {
    let width = crate::interactive_menu::term_width();
    let logo_cols = if width >= 100 { 48 } else { 32 };
    let logo_str = crate::logo::render_logo_to_string(logo_cols);
    let logo_lines: Vec<String> = logo_str.lines().map(String::from).collect();

    let mut content = Vec::new();
    content.push(String::new());
    content.push(format!(
        " {}  {} {}",
        "\u{1f389}",
        lang.tr("Welcome to Niubash"),
        format!("v{}!", env!("CARGO_PKG_VERSION"))
    ));
    content.push(format!(
        " {}  {}",
        "\u{2728}",
        lang.tr("A native Rust implementation of bash for Windows \u{2014} \
             no WSL, no MSYS2, no emulation layer.")
    ));
    content.push(String::new());
    if reconfigure {
        content.push(
            lang.tr("Reconfigure your interactive prompt/plugins. Existing rc will be backed up.")
                .to_string(),
        );
    } else {
        content.push(lang.tr("Let\u{2019}s get you set up.").to_string());
    }
    content.push(String::new());

    crate::interactive_menu::print_side_by_side(&logo_lines, &content, 100);
}

fn run_wizard_inner(reconfigure: bool) -> anyhow::Result<()> {
    let home = setup_home_dir();
    let lang = Lang::detect();
    let t = lang;

    display_welcome_side_by_side(reconfigure, lang);

    let probe = EnvProbe::collect();
    let mut io = WizardIo::new(crate::terminal::stdio_is_interactive());

    if io.interactive {
        probe.print_summary(lang);
    } else if probe.mintty_hint {
        println!();
        println!(
            "  \u{2139}\u{fe0f}  {}",
            t.tr("This terminal can't host interactive menus (Git Bash/MinTTY).")
        );
        println!(
            "  \u{2502}  {}",
            t.tr("Applying the 'minimal' preset. Re-run `niu setup` inside")
        );
        println!(
            "  \u{2502}  {}",
            t.tr("Windows Terminal, cmd, or PowerShell for the full wizard.")
        );
    }

    if cfg!(windows) && io.interactive && !probe.command_links {
        println!();
        println!(
            "  \u{26a0}\u{fe0f}  {}",
            t.tr("WinuxCmd command links look missing (ls/cat/grep/ln).")
        );
        println!(
            "  \u{2502}  {}",
            t.tr("They are created automatically on startup; if Unix commands still")
        );
        println!(
            "  \u{2502}  {}",
            t.tr("fail after setup, restart niu or run `wpm links rebuild`.")
        );
    }

    // Non-interactive runs stay deterministic: apply the 'minimal' preset.
    if !io.interactive {
        let preset = Preset::builtin("minimal");
        let cfg = preset.to_config(&probe, probe.nerd_font, &mut Vec::new(), lang);
        write_rc_and_mark_done(&home, &cfg, lang)?;
        return Ok(());
    }

    // --- Q1: theme gallery ---
    // External oh-my-bash themes lead (corpus-proven primary layer), the
    // built-in bundle themes retire to the end behind the separator with the
    // honest fallback label, and "keep current" is an always-equal first
    // option. No font step, no preset survey: one question.
    let gallery = theme_gallery();
    let current = current_theme_pick(&home);
    let mut theme_pick = ThemePick::Keep;
    if gallery.entries.is_empty() {
        println!();
        println!(
            "  {}",
            t.tr("No themes installed yet — keeping the built-in default look.")
        );
        println!(
            "  {}",
            t.tr("Browse the ecosystem any time with `niu plugin discover` (read-only).")
        );
    } else {
        let mut options = vec![match &current {
            ThemePick::Keep => t.tr("Skip — keep my current theme").to_string(),
            pick @ (ThemePick::Native { .. } | ThemePick::External { .. }) => format!(
                "{} ({})",
                t.tr("Skip — keep my current theme"),
                describe_theme_pick(pick, t)
            ),
        }];
        for entry in &gallery.entries {
            let label = match &entry.tier {
                ThemeGalleryTier::User | ThemeGalleryTier::External { .. } => entry.name.clone(),
                ThemeGalleryTier::Builtin => {
                    format!("{}{}", entry.name, t.tr("  · built-in fallback"))
                }
            };
            options.push(label);
        }
        let theme_refs: Vec<&str> = options.iter().map(String::as_str).collect();
        let nf_capable = probe.nerd_font;
        let preview = |i: usize| -> Vec<String> {
            if i == 0 {
                return match &current {
                    ThemePick::Keep => vec![t.tr("current look unchanged").to_string()],
                    pick => vec![format!("{} {}", t.tr("keep"), describe_theme_pick(pick, t))],
                };
            }
            let entry = &gallery.entries[i - 1];
            match &entry.tier {
                ThemeGalleryTier::User => {
                    let mut lines = theme_preview_line(&entry.name, "\u{276f}")
                        .lines()
                        .map(String::from)
                        .collect::<Vec<_>>();
                    lines.push(t.tr("your theme (~/.niubash/themes)").to_string());
                    lines
                }
                ThemeGalleryTier::External { adapter, .. } => vec![
                    format!("{} — {} {}", entry.name, adapter, t.tr("theme (external source, primary)")),
                    t.tr("renders via the bash-compatible PS1 channel; built-in themes stay as fallback").to_string(),
                ],
                ThemeGalleryTier::Builtin => {
                    let mut lines = theme_preview_line(&entry.name, "\u{276f}")
                        .lines()
                        .map(String::from)
                        .collect::<Vec<_>>();
                    if nerd_font_theme(&entry.name) && !nf_capable {
                        lines.push(
                            t.tr("needs a Nerd Font — `niu font` installs one (optional)")
                                .to_string(),
                        );
                    }
                    lines
                }
            }
        };
        let builtin_hint = if gallery.builtin_start < gallery.entries.len() {
            t.tr(
                "  │  external themes come first; entries marked 'built-in fallback' are the safe built-ins\n  │  Skip changes nothing",
            )
        } else {
            t.tr("  │  external themes from your trusted plugin sources; Skip changes nothing")
        };
        let idx = ask!(io.choice_preview(
            t.tr("  \u{1f3a8}  Pick a theme"),
            0,
            &theme_refs,
            builtin_hint,
            &preview,
        ));
        if idx > 0 {
            let entry = &gallery.entries[idx - 1];
            theme_pick = match &entry.tier {
                ThemeGalleryTier::User | ThemeGalleryTier::Builtin => ThemePick::Native {
                    name: entry.name.clone(),
                },
                ThemeGalleryTier::External { source_id, .. } => ThemePick::External {
                    name: entry.name.clone(),
                    source_id: source_id.clone(),
                },
            };
        }
    }

    // --- Q2: extra tab completions (opt-in, default off) ---
    let completion_candidates: Vec<&'static str> = COMPLETION_PACK_CANDIDATES
        .iter()
        .filter(|(_, bin)| probe.on_path(bin))
        .map(|(pack, _)| *pack)
        .collect();
    let mut completions_enabled = false;
    if !completion_candidates.is_empty() {
        let options = [
            format!(
                "{}  {}",
                pad_display(t.tr("Skip"), 8),
                t.tr("default — nothing changes")
            ),
            format!(
                "{}  {} {}",
                pad_display(t.tr("Enable"), 8),
                t.tr("add"),
                completion_candidates.join(" ")
            ),
        ];
        let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
        let idx = ask!(io.choice(
            t.tr("  \u{2328}\u{fe0f}  Extra tab completions for tools on PATH?"),
            0,
            &option_refs,
            t.tr("  \u{2502}  adds the completion packs found on PATH; skip keeps the defaults"),
        ));
        completions_enabled = idx == 1;
    }

    // --- Q3: niu-git, offered once (never auto-installed, never nagged) ---
    // Windows-only question: the wpm install form exists only on Windows,
    // and Linux/macOS users already have native git — the topic (and the
    // `wpm` string itself) must never appear on other platforms.
    let niu_git = match ask_niu_git(&mut io, &t, &home, &probe) {
        Some(choice) => choice,
        None => return Ok(()), // cancelled at the niu-git question
    };

    // --- Summary + explicit Apply gate ---
    let cfg = build_config(&theme_pick, completions_enabled, &completion_candidates);
    print_config_summary(&cfg, &theme_pick, completions_enabled, niu_git, lang);
    let confirm_options = [t.tr("Apply"), t.tr("Cancel")];
    let confirm = ask!(io.confirm(
        t.tr("  \u{2705}  Apply this configuration?"),
        &confirm_options,
    ));
    if confirm == 1 {
        println!("  {}", t.tr("Nothing was written."));
        return Ok(());
    }

    let backup_path = write_rc_and_mark_done(&home, &cfg, lang)?;

    if niu_git == NiuGitChoice::Install {
        install_niu_git(&home, lang);
    } else if niu_git == NiuGitChoice::NeverShow {
        write_niu_git_answer(&home, "never");
    }

    print_finish_screen(backup_path.as_deref(), lang);

    Ok(())
}

/// Short human description of a theme pick ("classic", "robbyrussell ·
/// oh-my-bash").
fn describe_theme_pick(pick: &ThemePick, t: Lang) -> String {
    match pick {
        ThemePick::Keep => t.tr("default").to_string(),
        ThemePick::Native { name } => name.clone(),
        ThemePick::External { name, .. } => format!("{} · oh-my-bash", name),
    }
}

/// The theme that is active right now: the `NIU_THEME` the shell was started
/// with, else the assignment in the existing rc (`OSH_THEME` +
/// `NIU_THEME_SOURCE=omb` marks an external oh-my-bash pick). Used so
/// "Skip — keep my current theme" is honest and side-effect free.
fn current_theme_pick(home: &std::path::Path) -> ThemePick {
    if let Ok(value) = std::env::var("NIU_THEME") {
        let value = value.trim().trim_matches('\'').trim_matches('"');
        if !value.is_empty() {
            return ThemePick::Native {
                name: value.to_string(),
            };
        }
    }
    let text = std::fs::read_to_string(home.join(PRIMARY_RC_FILE))
        .ok()
        .or_else(|| std::fs::read_to_string(home.join(COMPAT_RC_FILE)).ok());
    let Some(text) = text else {
        return ThemePick::Keep;
    };
    let mut native: Option<String> = None;
    let mut osh: Option<String> = None;
    let mut omb_channel = false;
    for raw in text.lines() {
        let line = raw.trim().strip_prefix("export ").unwrap_or(raw).trim();
        let assigned = |name: &str| -> Option<String> {
            let rest = line.strip_prefix(name)?.strip_prefix('=')?;
            let value = rest.trim().trim_matches('\'').trim_matches('"');
            (!value.is_empty()).then(|| value.to_string())
        };
        if let Some(value) = assigned("NIU_THEME") {
            native = Some(value);
        } else if let Some(value) = assigned("OSH_THEME") {
            osh = Some(value);
        } else if let Some(value) = assigned("NIU_THEME_SOURCE") {
            omb_channel = value == "omb";
        }
    }
    if omb_channel {
        if let Some(name) = osh {
            return ThemePick::External {
                name,
                source_id: "oh-my-bash".to_string(),
            };
        }
    }
    if let Some(name) = native {
        return ThemePick::Native { name };
    }
    if let Some(name) = osh {
        return ThemePick::External {
            name,
            source_id: "oh-my-bash".to_string(),
        };
    }
    ThemePick::Keep
}

/// Build the rc configuration from the wizard answers. Skip paths mirror the
/// previous rc verbatim (theme lines omitted, plugin selection re-emitted),
/// and opting into completions only ever *adds* packs.
fn build_config(
    theme_pick: &ThemePick,
    completions_enabled: bool,
    completion_candidates: &[&str],
) -> WizardConfig {
    let previous = crate::plugins::configured_plugins();
    let mut cfg = WizardConfig {
        prompt_enabled: true,
        ..WizardConfig::default()
    };
    match theme_pick {
        ThemePick::Keep => {}
        ThemePick::Native { name } => cfg.theme = name.clone(),
        ThemePick::External { name, source_id } => {
            cfg.theme = name.clone();
            cfg.theme_source_id = Some(source_id.clone());
        }
    }
    if completions_enabled {
        let mut names = previous.load;
        for pack in completion_candidates {
            if !names.iter().any(|name| name == pack) {
                names.push(pack.to_string());
            }
        }
        cfg.plugins = names;
        cfg.disable_default_plugins = previous.disable_defaults;
    } else {
        cfg.plugins = previous.load;
        cfg.disable_default_plugins = previous.disable_defaults;
    }
    cfg
}

/// The user's answer to the niu-git question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NiuGitChoice {
    Skip,
    Install,
    NeverShow,
}

// --- niu-git ask: Windows-only at compile time (owner directive) ---
// The wpm install form and every `wpm` string exist only on Windows builds;
// other platforms get a Skip stub and contain none of it.
#[cfg(windows)]
fn ask_niu_git(
    io: &mut WizardIo,
    t: &Lang,
    home: &std::path::Path,
    probe: &EnvProbe,
) -> Option<NiuGitChoice> {
    let mut niu_git = NiuGitChoice::Skip;
    if read_niu_git_answer(home).is_none() {
        let options = [
            format!(
                "{}  {}",
                pad_display(t.tr("Skip"), 14),
                t.tr("default — nothing is installed")
            ),
            format!(
                "{}  {}",
                pad_display(t.tr("Install"), 14),
                NIUGIT_INSTALL_COMMAND
            ),
            format!(
                "{}  {}",
                pad_display(t.tr("Don't ask again"), 14),
                t.tr("remember this and stop offering")
            ),
        ];
        let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
        let note = if probe.on_path("git") {
            t.tr("  │  a separate GPLv2 project; your current git keeps working either way")
        } else {
            t.tr("  │  a separate GPLv2 project — native Windows git without MSYS")
        };
        let idx = match io.choice(
            t.tr("  🧩  niu-git — Windows-native git experience?"),
            0,
            &option_refs,
            note,
        ) {
            Some(value) => value,
            None => return None, // cancelled
        };
        niu_git = match idx {
            1 => NiuGitChoice::Install,
            2 => NiuGitChoice::NeverShow,
            _ => NiuGitChoice::Skip,
        };
    }
    Some(niu_git)
}

#[cfg(not(windows))]
fn ask_niu_git(
    _io: &mut WizardIo,
    _t: &Lang,
    _home: &std::path::Path,
    _probe: &EnvProbe,
) -> Option<NiuGitChoice> {
    Some(NiuGitChoice::Skip)
}

fn wizard_answers_path(home: &std::path::Path) -> PathBuf {
    home.join(".niubash").join("wizard-answers.toml")
}

/// The recorded niu-git answer, when the user gave a lasting one ("never",
/// or "installed" after a successful pick). While `None` the wizard may
/// offer the choice again on the next explicit `niu setup` run — a wizard
/// re-run is user-initiated, never a nag.
fn read_niu_git_answer(home: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(wizard_answers_path(home)).ok()?;
    for raw in text.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix("niu_git") else {
            continue;
        };
        let value = rest.trim().strip_prefix('=')?.trim();
        let value = value.trim_matches('"').trim_matches('\'');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn write_niu_git_answer(home: &std::path::Path, value: &str) {
    let path = wizard_answers_path(home);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = format!(
        "# Answers the user gave explicitly during `niu setup`.\n\
         schema = \"{}\"\n\
         niu_git = \"{}\"\n",
        WIZARD_ANSWERS_SCHEMA, value
    );
    if let Err(err) = std::fs::write(&path, body) {
        println!(
            "  \u{26a0}\u{fe0f}  {} {err}",
            Lang::detect().tr("could not write")
        );
    }
}

/// Run the niu-git install the user explicitly picked — the only path that
/// ever invokes wpm here. The "installed" marker is only recorded after a
/// successful install, so a failed install stays retryable.
#[cfg(windows)]
fn install_niu_git(home: &std::path::Path, lang: Lang) {
    let Some(mut wpm) = wpm_command() else {
        println!(
            "  \u{26a0}\u{fe0f}  {} {}",
            lang.tr("wpm not available — install later with:"),
            NIUGIT_INSTALL_COMMAND
        );
        return;
    };
    println!("  \u{1f4e6}  {}", NIUGIT_INSTALL_COMMAND);
    let status = wpm
        .arg("install")
        .arg(NIUGIT_WPM_PACKAGE)
        .stdin(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => {
            write_niu_git_answer(home, "installed");
            println!("  \u{2705}  {}", lang.tr("niu-git installed"));
        }
        Ok(status) => println!(
            "  \u{26a0}\u{fe0f}  {} (exit {})",
            lang.tr("niu-git install failed — no other changes were made"),
            status.code().unwrap_or(1)
        ),
        Err(err) => println!(
            "  \u{26a0}\u{fe0f}  {}: {err}",
            lang.tr("niu-git install failed — no other changes were made")
        ),
    }
}

/// The final "how to change things later" block — one compact screen, in the
/// spirit of oh-my-zsh's post-install hints. Nothing here installs anything.
fn print_finish_screen(backup_path: Option<&std::path::Path>, lang: Lang) {
    let t = lang;
    println!();
    if let Some(path) = backup_path {
        println!(
            "  \u{1f4e6}  {}",
            fill(t.tr("Previous rc backed up to {}"), &[&path.display()])
        );
    }
    println!();
    println!("  \u{1f504}  {}", t.tr("Change things later:"));
    println!(
        "  \u{2502}    {}",
        t.tr("theme      `niu plugin themes`  →  NIU_THEME=… in ~/.niubashrc")
    );
    println!(
        "  \u{2502}    {}",
        t.tr("plugins    `niu plugin list`  ·  `niu plugin enable <name>`")
    );
    println!(
        "  \u{2502}    {}",
        t.tr("ecosystem  `niu plugin discover`  (sources & themes, read-only)")
    );
    println!(
        "  \u{2502}    {}",
        t.tr("font / WT  `niu font`  ·  `niu --install-wt-profile`")
    );
    println!("  \u{2502}    {}", t.tr("this guide `niu setup`"));
    println!();
}

/// Apply a named preset non-interactively (`niu setup --preset <name>`).
pub fn apply_preset(name: &str) -> anyhow::Result<()> {
    let lang = Lang::detect();
    let t = lang;
    let presets = load_presets();
    let preset = presets.iter().find(|p| p.name == name).ok_or_else(|| {
        let names = presets
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::anyhow!(
            "{} '{name}' \u{2014} {} {names}",
            t.tr("unknown preset"),
            t.tr("available:")
        )
    })?;
    let home = setup_home_dir();
    let probe = EnvProbe::collect();
    let mut notes = Vec::new();
    let cfg = preset.to_config(&probe, probe.nerd_font, &mut notes, lang);
    for note in &notes {
        println!("  \u{2502}  {}", note);
    }
    let backup_path = write_rc_and_mark_done(&home, &cfg, lang)?;
    println!(
        "  \u{2705}  {}",
        format!("{} '{}'.", t.tr("Preset applied:"), preset.name)
    );
    if let Some(path) = backup_path {
        println!(
            "  \u{1f4e6}  {}",
            fill(t.tr("Previous rc backed up to {}"), &[&path.display()])
        );
    }
    Ok(())
}

/// Render the final confirmation table before anything is written. Only the
/// explicitly picked rows appear; everything else is called out as untouched.
fn print_config_summary(
    cfg: &WizardConfig,
    theme_pick: &ThemePick,
    completions_enabled: bool,
    niu_git: NiuGitChoice,
    lang: Lang,
) {
    let t = lang;
    let row = |en: &str, value: String| {
        println!("  \u{2502}  {} {}", pad_display(t.tr(en), 13), value);
    };
    println!();
    println!("  \u{1f4cb}  {}", t.tr("Summary"));
    row(
        "theme",
        match theme_pick {
            ThemePick::Keep => t.tr("unchanged").to_string(),
            ThemePick::Native { name } => name.clone(),
            ThemePick::External { name, .. } => format!("{} ({})", name, t.tr("oh-my-bash source")),
        },
    );
    row(
        "completions",
        if completions_enabled && !cfg.plugins.is_empty() {
            format!("{} {}", t.tr("add"), cfg.plugins.join(" "))
        } else {
            t.tr("unchanged").to_string()
        },
    );
    row(
        "niu-git",
        match niu_git {
            NiuGitChoice::Install => NIUGIT_INSTALL_COMMAND.to_string(),
            NiuGitChoice::NeverShow => t.tr("don't ask again").to_string(),
            NiuGitChoice::Skip => t.tr("skipped").to_string(),
        },
    );
    println!("  \u{2502}  {}", t.tr("everything else stays untouched"));
    println!();
}

/// Write `~/.niubashrc` from `cfg` (backing up any existing file) and create
/// the `.setup-done` marker. Returns the backup path when one was made.
fn write_rc_and_mark_done(
    home: &std::path::Path,
    cfg: &WizardConfig,
    lang: Lang,
) -> anyhow::Result<Option<PathBuf>> {
    let rc_content = generate_rc(cfg);
    let rc_path = home.join(PRIMARY_RC_FILE);
    let backup_path = write_primary_rc(home, &rc_content)?;

    let niubash_dir = home.join(".niubash");
    let _ = std::fs::create_dir_all(&niubash_dir);
    let _ = std::fs::write(niubash_dir.join(SETUP_DONE_FILE), b"");

    println!();
    println!(
        "  \u{2705}  {}",
        fill(lang.tr("Shell rc written to {}"), &[&rc_path.display()])
    );
    Ok(backup_path)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// `wpm` when the command link is on PATH, else `winuxcmd.exe wpm`.
fn wpm_command() -> Option<Command> {
    if on_path("wpm") {
        return Some(Command::new("wpm"));
    }
    crate::winuxcmd::find_winuxcmd().map(|exe| {
        let mut cmd = Command::new(exe);
        cmd.arg("wpm");
        cmd
    })
}

fn wpm_available() -> bool {
    wpm_command().is_some()
}

// ── Presets ──────────────────────────────────────────────────────────────────

impl Preset {
    /// Look up a built-in preset by name; panics only on a programmer error.
    fn builtin(name: &str) -> Preset {
        builtin_presets()
            .into_iter()
            .find(|p| p.name == name)
            .expect("built-in preset exists")
    }

    /// Expand this preset into a `WizardConfig` for the probed environment.
    /// Human-readable notes about skipped packs/fonts go to `notes`.
    fn to_config(
        &self,
        probe: &EnvProbe,
        nf_capable: bool,
        notes: &mut Vec<String>,
        lang: Lang,
    ) -> WizardConfig {
        let available = available_pack_names();
        let mut plugins: Vec<String> = Vec::new();
        for name in &self.packs {
            push_pack(&mut plugins, name, available.as_ref(), notes, lang);
        }
        for (pack, bin) in &self.conditional_packs {
            if probe.on_path(bin) {
                push_pack(&mut plugins, pack, available.as_ref(), notes, lang);
            } else {
                notes.push(fill(
                    lang.tr("pack '{}' skipped ('{}' not found on PATH)"),
                    &[pack, bin],
                ));
            }
        }

        let mut theme = self.theme.clone();
        let mut symbol = self.prompt_symbol.clone();
        if self.requires_nerd_font && !nf_capable {
            notes.push(fill(
                lang.tr("theme '{}' needs a Nerd Font \u{2014} using 'classic'"),
                &[&self.theme],
            ));
            theme = "classic".into();
            symbol = ">".into();
        }

        let mut aliases: Vec<(String, String)> = self
            .aliases
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (bin, map) in &self.conditional_aliases {
            if probe.on_path(bin) {
                aliases.extend(map.iter().map(|(k, v)| (k.clone(), v.clone())));
            } else {
                notes.push(fill(
                    lang.tr("aliases for '{}' skipped (not found on PATH)"),
                    &[bin],
                ));
            }
        }

        WizardConfig {
            theme,
            symbol,
            prompt_style: self.prompt_style.clone(),
            right_prompt: self.right_prompt.clone(),
            cwd_style: self.cwd_style.clone(),
            prompt_enabled: true,
            git_enabled: plugins.iter().any(|p| p == "git"),
            // wpm can fetch starship during the tools step, so the preset
            // defaults to the starship segment even when the binary is not
            // installed yet.
            git_backend: if self.starship_default
                && (probe.on_path("starship") || probe.on_path("wpm"))
            {
                GitBackend::StarshipSegment
            } else {
                GitBackend::Native
            },
            segment_preset: None,
            completion_style: self.completion_style.clone(),
            plugins,
            aliases,
            // Presets are an exact selection: the listed packs replace the
            // inventory defaults (same rc semantics presets always had).
            disable_default_plugins: true,
            theme_source_id: None,
        }
    }
}

fn push_pack(
    plugins: &mut Vec<String>,
    name: &str,
    available: Option<&BTreeSet<String>>,
    notes: &mut Vec<String>,
    lang: Lang,
) {
    if let Some(set) = available {
        if !set.contains(name) {
            notes.push(fill(
                lang.tr("pack '{}' not in the bundle \u{2014} skipped"),
                &[&name],
            ));
            return;
        }
    }
    if !plugins.iter().any(|p| p == name) {
        plugins.push(name.to_string());
    }
}

/// The three presets compiled into niubash. Bundle `presets/*.toml` files may
/// extend or override them; keep these safe on a bare install.
fn builtin_presets() -> Vec<Preset> {
    let recommended_aliases: BTreeMap<String, String> = [
        ("ll", "ls -la"),
        ("la", "ls -a"),
        ("l", "ls -F"),
        ("..", "cd .."),
        ("...", "cd ../.."),
        ("cls", "clear"),
        ("apt", "wpm"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let mut recommended_cond_aliases: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let eza: BTreeMap<String, String> = [
        ("ls", "eza --icons --git --group-directories-first"),
        ("ll", "eza -lh --icons --git --group-directories-first"),
        ("la", "eza -la --icons --git --group-directories-first"),
        ("lt", "eza --tree --level=2 --icons"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    recommended_cond_aliases.insert("eza".to_string(), eza);
    for (bin, alias, cmd) in [
        ("bat", "cat", "bat -pp"),
        ("dust", "du", "dust"),
        ("duf", "df", "duf"),
        ("fd", "files", "fd"),
        ("erd", "tree", "erd --icons"),
    ] {
        recommended_cond_aliases.insert(
            bin.to_string(),
            [(alias.to_string(), cmd.to_string())].into_iter().collect(),
        );
    }

    vec![
        Preset {
            name: "recommended".into(),
            summary: "curated daily driver: spaceship theme, git, smart aliases".into(),
            requires_nerd_font: true,
            theme: "spaceship".into(),
            prompt_symbol: "\u{276f}".into(),
            prompt_style: "minimal".into(),
            right_prompt: "time".into(),
            cwd_style: "home".into(),
            completion_style: "column".into(),
            packs: [
                "prompt-core",
                "prompts",
                "themes",
                "git",
                "keybindings",
                "common-aliases",
                "command-not-found",
                "last-working-dir",
                "dotenv",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            aliases: recommended_aliases.clone(),
            conditional_packs: [("fzf", "fzf"), ("zoxide", "zoxide")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            conditional_aliases: recommended_cond_aliases.clone(),
            starship_default: false,
        },
        Preset {
            name: "poweruser".into(),
            summary: "everything above plus starship, direnv, and tool-specific packs".into(),
            requires_nerd_font: true,
            theme: "spaceship".into(),
            prompt_symbol: "\u{276f}".into(),
            prompt_style: "minimal".into(),
            right_prompt: "full".into(),
            cwd_style: "home".into(),
            completion_style: "list".into(),
            packs: [
                "prompt-core",
                "prompts",
                "themes",
                "git",
                "keybindings",
                "common-aliases",
                "command-not-found",
                "last-working-dir",
                "dotenv",
                "extract",
                "path-tools",
                "env-sync",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            aliases: recommended_aliases,
            conditional_packs: [
                ("fzf", "fzf"),
                ("zoxide", "zoxide"),
                ("direnv", "direnv"),
                ("thefuck", "thefuck"),
                ("kubectl", "kubectl"),
                ("docker", "docker"),
                ("npm", "npm"),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
            conditional_aliases: recommended_cond_aliases,
            starship_default: true,
        },
        Preset {
            name: "minimal".into(),
            summary: "safe everywhere: classic theme, git prompt, no extra tooling".into(),
            requires_nerd_font: false,
            theme: "classic".into(),
            prompt_symbol: ">".into(),
            prompt_style: "minimal".into(),
            right_prompt: "off".into(),
            cwd_style: "home".into(),
            completion_style: "column".into(),
            packs: ["prompt-core", "git"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            aliases: BTreeMap::new(),
            conditional_packs: BTreeMap::new(),
            conditional_aliases: BTreeMap::new(),
            starship_default: false,
        },
    ]
}

/// Built-in presets plus any `presets/*.toml` shipped by the active bundle;
/// bundle presets override built-ins of the same name.
fn load_presets() -> Vec<Preset> {
    let mut presets = builtin_presets();
    let Some(dir) = bundle_presets_dir() else {
        return presets;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return presets;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let parsed = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str::<Preset>(&text).ok());
        match parsed {
            Some(preset) if !preset.name.is_empty() => {
                if let Some(existing) = presets.iter_mut().find(|p| p.name == preset.name) {
                    *existing = preset;
                } else {
                    presets.push(preset);
                }
            }
            _ => log::warn!("ignoring unparsable preset {}", path.display()),
        }
    }
    presets
}

/// Candidate bundle roots, mirroring both the registry inventory and the
/// rc-side `NIUBASH` search list so presets work in dev builds too.
fn bundle_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = crate::plugins::active_plugin_inventory().path {
        dirs.push(path);
    }
    if let Some(path) = std::env::var_os("NIU_APP_BUNDLE_PATH") {
        dirs.push(PathBuf::from(path));
    }
    let home = setup_home_dir();
    dirs.push(home.join(".oh-my-niu"));
    dirs.push(home.join(".niubash").join("oh-my-niu"));
    if let Ok(entries) = std::fs::read_dir(home.join(".niubash").join("bundles")) {
        dirs.extend(
            entries.flatten().map(|e| e.path()).filter(|p| {
                p.join("oh-my-niu.winux").is_file() || p.join("oh-my-niu.niu").is_file()
            }),
        );
    }
    dirs
}

fn bundle_presets_dir() -> Option<PathBuf> {
    bundle_dirs()
        .into_iter()
        .map(|d| d.join("presets"))
        .find(|d| d.is_dir())
}

/// Plugin names the oh-my-niu framework loader can resolve:
/// `<bundle>/plugins/<name>/<name>.plugin.niu` plus the user custom dir.
/// `None` when no bundle resolved — preset pack names are trusted as-is.
fn available_pack_names() -> Option<BTreeSet<String>> {
    let mut roots: Vec<PathBuf> = bundle_dirs()
        .into_iter()
        .map(|d| d.join("plugins"))
        .collect();
    roots.push(
        setup_home_dir()
            .join(".niubash")
            .join("custom")
            .join("plugins"),
    );
    if roots.is_empty() {
        return None;
    }
    let mut names = BTreeSet::new();
    let mut found_any = false;
    for root in roots {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.path().join(format!("{name}.plugin.niu")).is_file() {
                    names.insert(name);
                    found_any = true;
                }
            }
        }
    }
    found_any.then_some(names)
}

// ── Environment probing ─────────────────────────────────────────────────────

/// True when `tool` resolves to a file on PATH (`.exe`/`.bat`/`.cmd`/`.com`
/// or an extension-less name).
fn on_path(tool: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    const EXTS: &[&str] = &["", ".exe", ".bat", ".cmd", ".com"];
    std::env::split_paths(&path).any(|dir| {
        EXTS.iter()
            .any(|ext| dir.join(format!("{tool}{ext}")).is_file())
    })
}

fn setup_home_dir() -> PathBuf {
    shell_home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Guarded activation block for an external oh-my-bash theme (§3.3/§11.4):
/// set `OSH_THEME` first, then reuse the adapter's canonical loader snippet
/// (its `${OSH_THEME:-…}` default defers to ours). The existence guard keeps
/// the native fallback layer alive when the source tree is missing.
fn external_theme_activation(cfg: &WizardConfig, theme: &str) -> Option<String> {
    let source_id = cfg.theme_source_id.as_deref()?;
    if theme.is_empty() {
        return None;
    }
    let record = crate::plugins::sources::read_source_registry()
        .into_iter()
        .find(|record| record.id == source_id)?;
    let adapter = crate::plugins::sources::adapter_for(&record.adapter)?;
    let mut block = format!(
        "# Theme '{theme}' — external source '{source_id}' (primary; native themes stay as fallback)\n\
         OSH_THEME={}\n\
         export OSH_THEME\n",
        shell_quote(theme)
    );
    block.push_str(&adapter.loader_snippet(&record));
    Some(block)
}

fn generate_rc(cfg: &WizardConfig) -> String {
    let theme = if cfg.prompt_enabled {
        cfg.theme.as_str()
    } else {
        ""
    };
    let symbol = if cfg.prompt_enabled {
        cfg.symbol.as_str()
    } else {
        ">"
    };
    let external = external_theme_activation(cfg, theme);
    let (prompt_template, right_template) = if cfg.prompt_style.is_empty() {
        // No explicit prompt choice: leave the bundle's prompt defaults in
        // charge instead of overriding them with a template call.
        (String::new(), String::new())
    } else if cfg.prompt_style == "segments" {
        match cfg.segment_preset.as_deref().unwrap_or("classic") {
            "pure" => (
                "{cwd} {git} {command_execution_time}{newline}{prompt_char} ".to_string(),
                String::new(),
            ),
            "robbyrussell" => (
                "{cwd} {git}{newline}{prompt_char} ".to_string(),
                String::new(),
            ),
            "lean" => (
                "{cwd} {git}{newline}{prompt_char} ".to_string(),
                String::new(),
            ),
            "rainbow" | "classic" => (
                "{cwd} {git}{newline}{prompt_char} ".to_string(),
                "{status}{time} ".to_string(),
            ),
            _ => (
                "{cwd} {git}{newline}{prompt_char} ".to_string(),
                "{status}{time} ".to_string(),
            ),
        }
    } else {
        match (cfg.prompt_style.as_str(), cfg.right_prompt.as_str()) {
            ("powerline", "time") => ("{cwd} {git} ".to_string(), "{time} ".to_string()),
            ("powerline", "full") => ("{cwd} {git} ".to_string(), "{time} {git} ".to_string()),
            ("powerline", _) => ("{cwd} {git} ".to_string(), String::new()),
            ("multiline", "time") => (
                "{user}@{host} {time}\n{cwd} {git} ".to_string(),
                String::new(),
            ),
            ("multiline", "full") => (
                "{user}@{host} {time}\n{cwd} {git} ".to_string(),
                "{git} ".to_string(),
            ),
            ("multiline", _) => ("{user}@{host}\n{cwd} {git} ".to_string(), String::new()),
            ("classic", "time") => (
                "{user}@{host} {cwd} {git} ".to_string(),
                "{time} ".to_string(),
            ),
            ("classic", "full") => (
                "{user}@{host} {cwd} {git} ".to_string(),
                "{time} {git} ".to_string(),
            ),
            ("classic", _) => ("{user}@{host} {cwd} {git} ".to_string(), String::new()),
            ("minimal", "time") => ("{cwd} ".to_string(), "{time} ".to_string()),
            ("minimal", "full") => ("{cwd} ".to_string(), "{time} {git_branch} ".to_string()),
            _ => ("{cwd} ".to_string(), String::new()),
        }
    };
    let prompt_template = if cfg.git_enabled {
        prompt_template
    } else {
        strip_git_prompt_tokens(&prompt_template)
    };
    let right_template = if cfg.git_enabled {
        right_template
    } else {
        strip_git_prompt_tokens(&right_template)
    };
    // Empty fields mean "no override" — the line is omitted entirely, so the
    // wizard's skip paths change nothing.
    let mut header = String::new();
    if external.is_none() {
        // §3.2: an external oh-my-bash pick does not write the native theme
        // pair; the source loader owns the prompt channel.
        if !theme.is_empty() {
            header.push_str(&format!("NIU_THEME={}\n", shell_quote(theme)));
            header.push_str(&format!(
                "NIU_THEME_PLUGIN={}\n",
                shell_quote(&theme_plugin_name(theme))
            ));
        }
    }
    if !symbol.is_empty() {
        header.push_str(&format!("NIU_PROMPT_SYMBOL={}\n", shell_quote(symbol)));
    }
    if !cfg.cwd_style.is_empty() {
        header.push_str(&format!(
            "NIU_PROMPT_CWD_STYLE={}\n",
            shell_quote(cfg.cwd_style.as_str())
        ));
    }
    if !cfg.completion_style.is_empty() {
        header.push_str(&format!(
            "NIU_COMPLETION_STYLE={}\n",
            shell_quote(cfg.completion_style.as_str())
        ));
    }
    if cfg.disable_default_plugins {
        header.push_str("NIU_DISABLE_DEFAULT_PLUGINS=1\n");
    }
    // Plugin lines are written only when there is an explicit selection:
    // a preset's exact list, the user's preserved previous list, or the
    // completion packs they opted into. An empty skip-all config writes
    // nothing, leaving the inventory defaults active.
    let plugins_block = if cfg.disable_default_plugins || !cfg.plugins.is_empty() {
        format!("NIU_PLUGINS=({})\n", cfg.plugins.join(" "))
    } else {
        String::new()
    };
    // Must be exported before the bundle loads: prompt-core reads
    // NIU_PROMPT_GIT_BACKEND when it initializes.
    let starship_segment_setup = if cfg.git_backend == GitBackend::StarshipSegment {
        "NIU_PROMPT_GIT_BACKEND=starship\nexport NIU_PROMPT_GIT_BACKEND\n".to_string()
    } else {
        String::new()
    };
    let segment_note = cfg
        .segment_preset
        .as_deref()
        .map(|preset| format!("# Segment preset selected during setup: {preset}\n"))
        .unwrap_or_default();
    let alias_block = if cfg.aliases.is_empty() {
        String::new()
    } else {
        let mut block = String::from("# Aliases\n");
        for (name, cmd) in &cfg.aliases {
            block.push_str(&format!("alias {}={}\n", name, shell_quote(cmd)));
        }
        block
    };
    let prompt_call = if !cfg.prompt_enabled {
        "# Prompt/theme plugins disabled by setup.\n".to_string()
    } else if external.is_some() {
        format!(
            "# Prompt owned by the oh-my-bash theme '{theme}'; PS1 renders via the bash-compatible channel.\n"
        )
    } else if cfg.git_backend == GitBackend::StarshipFull {
        "# Prompt owned by Starship (the starship plugin runs `starship init bash`).\n".to_string()
    } else if cfg.prompt_style.is_empty() {
        String::new()
    } else {
        format!(
            "niubash_prompt_use_template {} {} 2>/dev/null || true\n",
            shell_quote(&prompt_template),
            shell_quote(&right_template)
        )
    };
    format!(
        r#"# Niubash interactive rc — generated by the setup wizard.
# Edit this file with normal Niubash/bash syntax.
# Structured TOML manifests are not user startup configuration; new interactive setup
# should live here.

{header}export NIU_THEME NIU_THEME_PLUGIN NIU_PROMPT_SYMBOL
export NIU_PROMPT_CWD_STYLE NIU_COMPLETION_STYLE NIU_DISABLE_DEFAULT_PLUGINS

{plugins_block}{starship_segment_setup}{segment_note}if [ -z "${{HOME:-}}" ] && [ -n "${{USERPROFILE:-}}" ]; then
  case "$USERPROFILE" in
    /[A-Za-z]/*)
      __niubash_home_drive="${{USERPROFILE#/}}"
      __niubash_home_drive="${{__niubash_home_drive%%/*}}"
      __niubash_home_rest="${{USERPROFILE#/$__niubash_home_drive/}}"
      HOME="$__niubash_home_drive:/$__niubash_home_rest"
      ;;
    *)
      HOME="${{USERPROFILE//\\}}"
      ;;
  esac
  export HOME
fi

if [ -z "${{NIUBASH:-}}" ]; then
  for __niubash_bundle in "$HOME/.oh-my-niu" "$HOME/.niubash/oh-my-niu" "$HOME/.niubash/bundles/oh-my-niu"/* "$NIU_APP_BUNDLE_PATH"; do
    if [ -f "$__niubash_bundle/oh-my-niu.niu" ] || [ -f "$__niubash_bundle/oh-my-niu.winux" ]; then
      NIUBASH="$__niubash_bundle"
      export NIUBASH
      break
    fi
  done
fi

if [ -f "$NIUBASH/oh-my-niu.niu" ]; then
  . "$NIUBASH/oh-my-niu.niu"
elif [ -f "$NIUBASH/oh-my-niu.winux" ]; then
  . "$NIUBASH/oh-my-niu.winux"
fi

{external_block}{alias_block}{prompt_call}unset __niubash_bundle __niubash_home_drive __niubash_home_rest

# Change things later (nothing here runs automatically):
#   niu plugin themes            list themes (external first, built-in fallback)
#   niu plugin discover          see sources & themes without installing
#   niu plugin enable <name>     turn a pack on
#   niu setup                    re-run this guide
"#,
        header = header,
        plugins_block = plugins_block,
        starship_segment_setup = starship_segment_setup,
        segment_note = segment_note,
        external_block = external.unwrap_or_default(),
        alias_block = alias_block,
        prompt_call = prompt_call,
    )
}

fn theme_plugin_name(theme: &str) -> String {
    if theme.starts_with("theme-") {
        theme.to_string()
    } else {
        format!("theme-{}", theme)
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn strip_git_prompt_tokens(value: &str) -> String {
    value
        .replace("{git_prompt}", "")
        .replace("{git}", "")
        .replace("{git_branch}", "")
        .replace("{git_status}", "")
        .replace("  ", " ")
}

fn write_primary_rc(home: &std::path::Path, rc_content: &str) -> anyhow::Result<Option<PathBuf>> {
    let rc_path = home.join(PRIMARY_RC_FILE);
    let niubash_dir = home.join(".niubash");
    std::fs::create_dir_all(&niubash_dir)?;
    let stamp = timestamp_id();
    let tmp_path = niubash_dir.join(format!(".niubashrc.tmp-{stamp}"));
    std::fs::write(&tmp_path, rc_content)?;

    let backup_path = if rc_path.is_file() {
        let backup_dir = niubash_dir.join("backups");
        std::fs::create_dir_all(&backup_dir)?;
        let backup = backup_dir.join(format!(".niubashrc.{stamp}.bak"));
        std::fs::copy(&rc_path, &backup)?;
        Some(backup)
    } else {
        None
    };

    if rc_path.exists() {
        std::fs::remove_file(&rc_path)?;
    }
    match std::fs::rename(&tmp_path, &rc_path) {
        Ok(()) => {}
        Err(_) => {
            std::fs::copy(&tmp_path, &rc_path)?;
            let _ = std::fs::remove_file(&tmp_path);
        }
    }
    Ok(backup_path)
}

fn timestamp_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}", now.as_secs(), now.subsec_millis())
}

fn theme_preview_line(theme_name: &str, symbol: &str) -> String {
    let theme = theme::by_name(theme_name);
    let dir = theme.prompt_dir.paint("~/repo/niubash").to_string();
    let git = theme.git_dirty.paint("codex/theme-api ✚ ? *").to_string();
    let prompt = theme.prompt_symbol.paint(symbol).to_string();
    let note = if nerd_font_theme(theme_name) {
        " [Nerd Font]"
    } else {
        ""
    };
    format!("{theme_name:<22} {dir} {git}\n  \u{2502}                           {prompt} {note}")
}

fn nerd_font_theme(theme_name: &str) -> bool {
    matches!(
        theme_name,
        "agnoster"
            | "dracula"
            | "catppuccin-mocha"
            | "gruvbox"
            | "spaceship"
            | "tokyonight"
            | "p10-classic"
            | "p10-lean"
            | "p10-rainbow"
            | "p10-pure"
    )
}

/// Built-in Chinese (Simplified) translations for the setup wizard. Keys are
/// the exact English literals passed to `Lang::tr`; anything missing falls
/// back to English. Keep compound lines as one template with the same
/// placeholder order so `format!` args line up in both languages.
fn zh(en: &str) -> Option<&'static str> {
    Some(match en {
        // Welcome
        "Welcome to Niubash" => "欢迎来到 Niubash",
        "A native Rust implementation of bash for Windows \u{2014} no WSL, no MSYS2, no emulation layer." =>
            "Windows 原生 Rust 实现的 bash —— 无需 WSL、MSYS2，没有模拟层。",
        "Reconfigure your interactive prompt/plugins. Existing rc will be backed up." =>
            "重新配置交互提示符/插件。现有 rc 文件会先备份。",
        "Let\u{2019}s get you set up." => "我们开始配置吧。",

        // Environment summary
        "Environment" => "环境",
        "classic console" => "传统控制台",
        "none detected" => "未检测到",
        "terminal" => "终端",
        "nerd font" => "Nerd 字体",
        "command links" => "命令链接",
        "tools" => "工具",
        "detected" => "已安装",
        "not found" => "未检测到",
        "ready" => "就绪",
        "missing" => "缺失",

        // MinTTY note
        "This terminal can't host interactive menus (Git Bash/MinTTY)." =>
            "当前终端（Git Bash/MinTTY）不支持交互菜单。",
        "Applying the 'minimal' preset. Re-run `niu setup` inside" =>
            "已应用 'minimal' 预设。请在 Windows Terminal、cmd 或",
        "Windows Terminal, cmd, or PowerShell for the full wizard." =>
            "PowerShell 中重新运行 `niu setup` 进入完整向导。",

        // Command-link warning
        "WinuxCmd command links look missing (ls/cat/grep/ln)." =>
            "WinuxCmd 命令链接似乎缺失（ls/cat/grep/ln）。",
        "They are created automatically on startup; if Unix commands still" =>
            "它们会在启动时自动创建；若设置完成后 Unix 命令仍",
        "fail after setup, restart niu or run `wpm links rebuild`." =>
            "无法使用，请重启 niu 或运行 `wpm links rebuild`。",

        // Theme gallery step
        "Skip — keep my current theme" => "跳过 —— 保留当前主题",
        "  · built-in fallback" => "  · 内置保底",
        "  \u{1f3a8}  Pick a theme" => "  \u{1f3a8}  选择主题",
        "  │  external themes come first; entries marked 'built-in fallback' are the safe built-ins\n  \u{2502}  Skip changes nothing" =>
            "  \u{2502}  外部主题排在前面；标注“内置保底”的是安全内置项\n  \u{2502}  跳过则不做任何改动",
        "  │  external themes from your trusted plugin sources; Skip changes nothing" =>
            "  \u{2502}  来自你已信任插件源的外部主题；跳过则不做任何改动",
        "No themes installed yet — keeping the built-in default look." =>
            "尚未安装任何主题 —— 保持内置默认外观。",
        "Browse the ecosystem any time with `niu plugin discover` (read-only)." =>
            "随时用 `niu plugin discover` 浏览生态（只读，不安装）。",
        "current look unchanged" => "当前外观保持不变",
        "keep" => "保留",
        "default" => "默认",
        "your theme (~/.niubash/themes)" => "你的主题（~/.niubash/themes）",
        "theme (external source, primary)" => "主题（外部源，主选）",
        "renders via the bash-compatible PS1 channel; built-in themes stay as fallback" =>
            "经 bash 兼容 PS1 通道渲染；内置主题作为保底保留",
        "needs a Nerd Font — `niu font` installs one (optional)" =>
            "需要 Nerd Font —— 可用 `niu font` 安装（可选）",

        // Completion opt-in
        "Skip" => "跳过",
        "Enable" => "启用",
        "add" => "添加",
        "default — nothing changes" => "默认 —— 不做任何改动",
        "  \u{2328}\u{fe0f}  Extra tab completions for tools on PATH?" =>
            "  \u{2328}\u{fe0f}  为 PATH 上的工具启用更多 Tab 补全？",
        "  \u{2502}  adds the completion packs found on PATH; skip keeps the defaults" =>
            "  \u{2502}  添加在 PATH 上找到的工具的补全包；跳过则保持默认",

        // niu-git single-choice
        "Install" => "安装",
        "Don't ask again" => "不再询问",
        "remember this and stop offering" => "记住这个选择，之后不再提供",
        "default — nothing is installed" => "默认 —— 不安装任何东西",
        "  \u{1f9e9}  niu-git — Windows-native git experience?" =>
            "  \u{1f9e9}  niu-git —— Windows 原生 git 体验？",
        "  \u{2502}  a separate GPLv2 project; your current git keeps working either way" =>
            "  \u{2502}  独立的 GPLv2 项目；无论选不选，现有 git 照常工作",
        "  \u{2502}  a separate GPLv2 project — native Windows git without MSYS" =>
            "  \u{2502}  独立的 GPLv2 项目 —— 无 MSYS 的 Windows 原生 git",
        "wpm not available — install later with:" =>
            "wpm 不可用 —— 之后可用以下命令安装：",
        "niu-git installed" => "niu-git 已安装",
        "niu-git install failed — no other changes were made" =>
            "niu-git 安装失败 —— 其他内容未做任何改动",

        // Summary (labels shared with the environment summary above)
        "Summary" => "配置摘要",
        "theme" => "主题",
        "completions" => "补全",
        "niu-git" => "niu-git",
        "oh-my-bash source" => "oh-my-bash 源",
        "unchanged" => "保持不变",
        "skipped" => "已跳过",
        "don't ask again" => "不再询问",
        "everything else stays untouched" => "其余一切保持原样",

        // Confirm + final messages
        "  \u{2705}  Apply this configuration?" => "  \u{2705}  应用此配置？",
        "Apply" => "应用",
        "Cancel" => "取消",
        "Nothing was written." => "未写入任何内容。",
        "Setup cancelled \u{2014} nothing was written." =>
            "设置已取消 —— 未写入任何内容。",
        "Shell rc written to {}" => "Shell 配置已写入 {}",
        "Previous rc backed up to {}" => "原 rc 已备份至 {}",
        "could not write" => "无法写入",

        // Finish screen
        "Change things later:" => "之后想调整：",
        "theme      `niu plugin themes`  →  NIU_THEME=… in ~/.niubashrc" =>
            "主题       `niu plugin themes`  →  在 ~/.niubashrc 设 NIU_THEME=…",
        "plugins    `niu plugin list`  ·  `niu plugin enable <name>`" =>
            "插件       `niu plugin list`  ·  `niu plugin enable <名称>`",
        "ecosystem  `niu plugin discover`  (sources & themes, read-only)" =>
            "生态       `niu plugin discover`（插件源与主题，只读）",
        "font / WT  `niu font`  ·  `niu --install-wt-profile`" =>
            "字体/终端  `niu font`  ·  `niu --install-wt-profile`",
        "this guide `niu setup`" => "本向导     `niu setup`",

        // apply_preset
        "unknown preset" => "未知预设",
        "available:" => "可用：",
        "Preset applied:" => "预设已应用：",

        // Preset-expansion notes
        "pack '{}' skipped ('{}' not found on PATH)" =>
            "插件包 '{}' 已跳过（PATH 中未找到 '{}'）",
        "theme '{}' needs a Nerd Font \u{2014} using 'classic'" =>
            "主题 '{}' 需要 Nerd Font —— 改用 'classic'",
        "aliases for '{}' skipped (not found on PATH)" =>
            "'{}' 相关别名已跳过（PATH 中未找到）",
        "pack '{}' not in the bundle \u{2014} skipped" =>
            "插件包 '{}' 不在 bundle 中 —— 已跳过",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::PROCESS_STATE_LOCK;

    #[test]
    fn display_welcome_side_by_side_renders_without_panic() {
        display_welcome_side_by_side(false, Lang::En);
        display_welcome_side_by_side(true, Lang::Zh);
    }

    #[test]
    fn setup_home_dir_accepts_shell_style_home_env() {
        let _process_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let home = unique_temp_dir("niubash-setup-home").join("home");
        let _home = EnvGuard::set("HOME", &host_to_shell_style_path(&home));
        let _userprofile = EnvGuard::unset("USERPROFILE");

        let resolved = setup_home_dir();
        if cfg!(windows) {
            assert_eq!(display_path(&resolved), display_path(&home));
        } else {
            assert_eq!(display_path(&resolved), display_path(&home));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn test_cfg(
        theme: &str,
        style: &str,
        right: &str,
        symbol: &str,
        cwd: &str,
        prompt_enabled: bool,
        git: bool,
        git_backend: GitBackend,
        segment: Option<&str>,
        completion: &str,
    ) -> WizardConfig {
        let mut plugins = Vec::new();
        if prompt_enabled {
            plugins.push("prompt-core".to_string());
        }
        if git {
            plugins.push("git".to_string());
        }
        if git_backend == GitBackend::StarshipFull {
            plugins.push("starship".to_string());
        }
        WizardConfig {
            theme: theme.to_string(),
            prompt_style: style.to_string(),
            right_prompt: right.to_string(),
            symbol: symbol.to_string(),
            cwd_style: cwd.to_string(),
            prompt_enabled,
            git_enabled: git,
            git_backend,
            segment_preset: segment.map(String::from),
            completion_style: completion.to_string(),
            plugins,
            aliases: Vec::new(),
            // Test configs model the preset flow (exact selection).
            disable_default_plugins: true,
            theme_source_id: None,
        }
    }

    #[test]
    fn generated_rc_uses_shell_entrypoint_not_toml_sections() {
        let rc = generate_rc(&test_cfg(
            "minimal",
            "minimal",
            "time",
            ">",
            "home",
            true,
            true,
            GitBackend::Native,
            None,
            "column",
        ));

        assert!(rc.contains("NIU_THEME_PLUGIN='theme-minimal'"));
        assert!(rc.contains("NIU_PROMPT_CWD_STYLE='home'"));
        assert!(rc.contains("NIU_DISABLE_DEFAULT_PLUGINS=1"));
        assert!(rc.contains("NIU_PLUGINS=(prompt-core git)"));
        assert!(rc.contains("\"$NIU_APP_BUNDLE_PATH\""));
        assert!(rc.contains(". \"$NIUBASH/oh-my-niu.niu\""));
        assert!(rc.contains("niubash_prompt_use_template '{cwd} ' '{time} '"));
        assert!(!rc.contains("[plugins]"));
        assert!(!rc.contains("[shell]"));
        assert!(!rc.contains("prompt_format ="));
    }

    #[test]
    fn generated_rc_can_disable_git_prompt_tokens() {
        let rc = generate_rc(&test_cfg(
            "minimal",
            "classic",
            "full",
            "$",
            "full",
            true,
            false,
            GitBackend::Native,
            None,
            "column",
        ));

        assert!(rc.contains("NIU_THEME_PLUGIN='theme-minimal'"));
        assert!(rc.contains("NIU_PROMPT_CWD_STYLE='full'"));
        assert!(rc.contains("NIU_PLUGINS=(prompt-core)"));
        assert!(!rc.contains(" git "));
        assert!(!rc.contains("{git_prompt}"));
        assert!(!rc.contains("{git_branch}"));
    }

    #[test]
    fn generated_rc_can_disable_prompt_theme_plugins() {
        let rc = generate_rc(&test_cfg(
            "",
            "off",
            "off",
            ">",
            "basename",
            false,
            true,
            GitBackend::Native,
            None,
            "column",
        ));

        // "No theme" now means the theme override lines are omitted entirely
        // rather than written empty.
        assert!(!rc.contains("NIU_THEME="));
        assert!(!rc.contains("NIU_THEME_PLUGIN="));
        assert!(rc.contains("NIU_PROMPT_CWD_STYLE='basename'"));
        assert!(rc.contains("NIU_DISABLE_DEFAULT_PLUGINS=1"));
        assert!(rc.contains("NIU_PLUGINS=(git)"));
        assert!(rc.contains("# Prompt/theme plugins disabled by setup."));
        assert!(!rc.contains("niubash_prompt_use_template"));
        assert!(!rc.contains("prompt_format ="));
    }

    #[test]
    fn generated_rc_can_delegate_git_segment_to_starship() {
        let rc = generate_rc(&test_cfg(
            "spaceship",
            "multiline",
            "off",
            "%",
            "home",
            true,
            true,
            GitBackend::StarshipSegment,
            None,
            "column",
        ));

        assert!(rc.contains("NIU_THEME_PLUGIN='theme-spaceship'"));
        assert!(rc.contains("NIU_PLUGINS=(prompt-core git)"));
        // The backend env var must be exported before the bundle loads.
        let backend_pos = rc.find("NIU_PROMPT_GIT_BACKEND=starship").unwrap();
        let bundle_pos = rc.find("oh-my-niu.niu\"").unwrap();
        assert!(backend_pos < bundle_pos);
        assert!(rc.contains("niubash_prompt_use_template"));
        assert!(rc.contains("{git}"));
        assert!(!rc.contains("NIU_STARSHIP_SEGMENTS"));
        assert!(!rc.contains("STARSHIP_CONFIG"));
    }

    #[test]
    fn generated_rc_full_starship_owns_the_prompt() {
        let rc = generate_rc(&test_cfg(
            "spaceship",
            "multiline",
            "off",
            "%",
            "home",
            true,
            true,
            GitBackend::StarshipFull,
            None,
            "column",
        ));

        assert!(rc.contains("NIU_PLUGINS=(prompt-core git starship)"));
        assert!(rc.contains("# Prompt owned by Starship"));
        assert!(!rc.contains("niubash_prompt_use_template"));
        assert!(!rc.contains("NIU_PROMPT_GIT_BACKEND"));
    }

    #[test]
    fn generated_rc_includes_completion_style() {
        let rc = generate_rc(&test_cfg(
            "minimal",
            "minimal",
            "off",
            ">",
            "home",
            true,
            true,
            GitBackend::Native,
            None,
            "list",
        ));
        assert!(rc.contains("NIU_COMPLETION_STYLE='list'"));
        assert!(rc.contains("NIU_COMPLETION_STYLE"));

        let rc = generate_rc(&test_cfg(
            "minimal",
            "minimal",
            "off",
            ">",
            "home",
            true,
            true,
            GitBackend::Native,
            None,
            "inline",
        ));
        assert!(rc.contains("NIU_COMPLETION_STYLE='inline'"));
    }

    #[test]
    fn generated_rc_writes_preset_aliases() {
        let mut cfg = test_cfg(
            "minimal",
            "minimal",
            "off",
            ">",
            "home",
            true,
            true,
            GitBackend::Native,
            None,
            "column",
        );
        cfg.aliases = vec![("ll".to_string(), "ls -la".to_string())];
        let rc = generate_rc(&cfg);
        assert!(rc.contains("alias ll='ls -la'"));
    }

    #[test]
    fn builtin_presets_expand_to_configs() {
        let probe = EnvProbe {
            windows_terminal: false,
            mintty_hint: false,
            nerd_font: false,
            command_links: true,
            tools: BTreeSet::new(),
        };
        let presets = builtin_presets();
        assert!(presets.len() >= 3);
        assert!(presets.iter().any(|p| p.name == "recommended"));
        for preset in &presets {
            let mut notes = Vec::new();
            let cfg = preset.to_config(&probe, false, &mut notes, Lang::En);
            assert!(!cfg.completion_style.is_empty());
        }
        // Without a Nerd Font, presets that need one fall back to 'classic'.
        let recommended = presets.iter().find(|p| p.name == "recommended").unwrap();
        let mut notes = Vec::new();
        let cfg = recommended.to_config(&probe, false, &mut notes, Lang::En);
        assert_eq!(cfg.theme, "classic");
        assert!(notes.iter().any(|n| n.contains("Nerd Font")));
        // With a Nerd Font the preset theme survives.
        let cfg = recommended.to_config(&probe, true, &mut Vec::new(), Lang::En);
        assert_eq!(cfg.theme, "spaceship");
    }

    #[test]
    fn zh_translations_cover_the_wizard_vocab() {
        // Every key here is asserted to translate so a renamed English literal
        // fails loudly instead of silently falling back to English.
        for key in [
            "Welcome to Niubash",
            "  \u{1f3a8}  Pick a theme",
            "Skip — keep my current theme",
            "  · built-in fallback",
            "  \u{2328}\u{fe0f}  Extra tab completions for tools on PATH?",
            "  \u{1f9e9}  niu-git — Windows-native git experience?",
            "Apply",
            "Cancel",
            "Change things later:",
        ] {
            assert!(zh(key).is_some(), "missing zh translation for {key:?}");
        }
        // Unknown keys fall back to English verbatim.
        assert_eq!(Lang::Zh.tr("untranslated literal"), "untranslated literal");
    }

    #[test]
    fn preset_toml_round_trips() {
        let text = r#"
name = "myteam"
summary = "team preset"
requires_nerd_font = false
theme = "minimal"
packs = ["prompt-core", "git"]

[aliases]
ll = "ls -la"

[conditional_packs]
fzf = "fzf"

[conditional_aliases.eza]
ls = "eza --icons"
"#;
        let preset: Preset = toml::from_str(text).unwrap();
        assert_eq!(preset.name, "myteam");
        assert_eq!(preset.aliases["ll"], "ls -la");
        assert_eq!(preset.conditional_packs["fzf"], "fzf");
        assert_eq!(preset.conditional_aliases["eza"]["ls"], "eza --icons");
    }

    /// Minimal oh-my-niu-shaped bundle with native TOML themes (same shape
    /// as tests/plugin_inventory.rs fixtures).
    fn write_theme_bundle(path: &std::path::Path, themes: &[&str]) {
        std::fs::create_dir_all(path.join("packs").join("themes")).unwrap();
        std::fs::create_dir_all(path.join("themes")).unwrap();
        std::fs::write(
            path.join("bundle.toml"),
            r#"name = "oh-my-niu"
version = "9.9.10"
api = "niubash:plugin-bundle@0.1.0"
min_niubash = "0.8.3"
[packs]
default = ["themes"]
available = ["themes"]
[layout]
packs_dir = "packs"
themes_dir = "themes"
"#,
        )
        .unwrap();
        let list = themes
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        std::fs::write(
            path.join("packs").join("themes").join("plugin.toml"),
            format!(
                r#"name = "themes"
bundle = "oh-my-niu"
version = "9.9.10"
kind = "builtin"
api = "niubash:plugin@0.1.0"
category = "ux"
summary = "Wizard gallery fixture."
default = true
permissions = []
required_binaries = []
[exports]
aliases = false
completions = []
prompt_segments = []
hooks = []
commands = []
keybindings = []
themes = [{list}]
"#
            ),
        )
        .unwrap();
        for name in themes {
            std::fs::write(
                path.join("themes").join(format!("{name}.toml")),
                "[prompt_user]\nfg = \"green\"\n",
            )
            .unwrap();
        }
    }

    /// The vendored oh-my-bash fixture tree shipped with the repo tests.
    fn omb_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sources/oh-my-bash")
    }

    fn install_trusted_omb_fixture(root: &std::path::Path) {
        crate::plugins::sources::add_source(crate::plugins::sources::SourceInstallRequest {
            adapter: None,
            origin: omb_fixture_path().to_string_lossy().into_owned(),
            ref_name: None,
            expected_checksum: None,
        })
        .expect("fixture source add must succeed");
        crate::plugins::sources::trust_source("oh-my-bash").expect("fixture trust must succeed");
        let _ = root;
    }

    #[test]
    fn theme_gallery_external_first_builtins_after_separator() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("wizard-gallery");
        let root = temp.join("sources");
        let bundle = temp.join("bundle");
        // The bundle's `agnoster` collides by name with the fixture's
        // external `agnoster` — external must win, and appear exactly once.
        write_theme_bundle(&bundle, &["agnoster", "native-only", "p10-classic"]);
        let _sources = EnvGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root.to_string_lossy());
        let _bundle = EnvGuard::set("NIU_PLUGIN_BUNDLE_PATH", &bundle.to_string_lossy());
        install_trusted_omb_fixture(&root);

        let gallery = theme_gallery();
        let position = |name: &str| {
            gallery
                .entries
                .iter()
                .position(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("{name} missing from {gallery:?}"))
        };
        let agnoster = position("agnoster");
        let robbyrussell = position("robbyrussell");
        let native_only = position("native-only");
        let p10 = position("p10-classic");
        // External themes lead; everything from the separator on is the
        // built-in fallback tier.
        assert!(agnoster < gallery.builtin_start, "{gallery:?}");
        assert!(robbyrussell < gallery.builtin_start, "{gallery:?}");
        assert!(native_only >= gallery.builtin_start, "{gallery:?}");
        assert!(p10 >= gallery.builtin_start, "{gallery:?}");
        assert!(matches!(
            gallery.entries[agnoster].tier,
            ThemeGalleryTier::External { .. }
        ));
        assert!(matches!(
            gallery.entries[native_only].tier,
            ThemeGalleryTier::Builtin
        ));
        assert_eq!(
            gallery
                .entries
                .iter()
                .filter(|entry| entry.name == "agnoster")
                .count(),
            1,
            "same-name collision must resolve once (external wins): {gallery:?}"
        );
        for (index, entry) in gallery.entries.iter().enumerate() {
            let is_builtin = matches!(entry.tier, ThemeGalleryTier::Builtin);
            assert_eq!(
                is_builtin,
                index >= gallery.builtin_start,
                "tier ordering violated at {index}: {gallery:?}"
            );
        }

        crate::plugins::sources::remove_source("oh-my-bash").unwrap();
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn wizard_skip_answers_leave_zero_rc_overrides() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("wizard-skip");
        let home = temp.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let _home = EnvGuard::set("HOME", &host_to_shell_style_path(&home));
        let _userprofile = EnvGuard::unset("USERPROFILE");

        // Skipped every question, first run (no previous rc): the rc the
        // wizard writes must not override anything.
        let cfg = build_config(&ThemePick::Keep, false, &[]);
        let rc = generate_rc(&cfg);
        assert!(!rc.contains("NIU_PLUGINS="), "{rc}");
        assert!(!rc.contains("NIU_DISABLE_DEFAULT_PLUGINS=1"), "{rc}");
        assert!(!rc.contains("NIU_THEME="), "{rc}");
        assert!(!rc.contains("OSH_THEME="), "{rc}");
        assert!(!rc.contains("niubash_prompt_use_template"), "{rc}");
        assert!(!rc.contains("alias "), "{rc}");
        assert!(!rc.contains("wpm"), "{rc}");
        // The how-to-change-later hints are still there.
        assert!(rc.contains("niu plugin discover"), "{rc}");
        assert!(rc.contains("niu setup"), "{rc}");
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn wizard_skip_preserves_previous_plugin_selection() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("wizard-skip-keep");
        let home = temp.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(PRIMARY_RC_FILE),
            "NIU_DISABLE_DEFAULT_PLUGINS=1\nNIU_PLUGINS=(prompt-core git golang)\n",
        )
        .unwrap();
        let _home = EnvGuard::set("HOME", &host_to_shell_style_path(&home));
        let _userprofile = EnvGuard::unset("USERPROFILE");

        // Reconfigure with every question skipped: the previous explicit
        // selection is carried over verbatim — a skip never disables packs.
        let cfg = build_config(&ThemePick::Keep, false, &[]);
        let rc = generate_rc(&cfg);
        assert!(rc.contains("NIU_PLUGINS=(prompt-core git golang)"), "{rc}");
        assert!(rc.contains("NIU_DISABLE_DEFAULT_PLUGINS=1"), "{rc}");
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn wizard_completion_optin_only_adds_packs() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("wizard-completions");
        let home = temp.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let _home = EnvGuard::set("HOME", &host_to_shell_style_path(&home));
        let _userprofile = EnvGuard::unset("USERPROFILE");

        // Fresh home: opting in adds packs on top of the defaults; the
        // wizard must not write the disable-defaults switch.
        let cfg = build_config(&ThemePick::Keep, true, &["git", "docker"]);
        let rc = generate_rc(&cfg);
        assert!(rc.contains("NIU_PLUGINS=(git docker)"), "{rc}");
        assert!(!rc.contains("NIU_DISABLE_DEFAULT_PLUGINS=1"), "{rc}");

        // A previous explicit list is merged, never dropped.
        std::fs::write(
            home.join(PRIMARY_RC_FILE),
            "NIU_PLUGINS=(prompt-core golang)\n",
        )
        .unwrap();
        let cfg = build_config(&ThemePick::Keep, true, &["git"]);
        let rc = generate_rc(&cfg);
        assert!(rc.contains("NIU_PLUGINS=(prompt-core golang git)"), "{rc}");
        assert!(!rc.contains("NIU_DISABLE_DEFAULT_PLUGINS=1"), "{rc}");
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn wizard_external_theme_pick_writes_guarded_loader() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let temp = unique_temp_dir("wizard-omb-theme");
        let root = temp.join("sources");
        let _sources = EnvGuard::set("NIU_PLUGIN_SOURCES_ROOT", &root.to_string_lossy());
        install_trusted_omb_fixture(&root);

        let cfg = WizardConfig {
            theme: "robbyrussell".to_string(),
            theme_source_id: Some("oh-my-bash".to_string()),
            prompt_enabled: true,
            ..WizardConfig::default()
        };
        let rc = generate_rc(&cfg);
        // §3.2/§3.3: OSH_THEME + NIU_THEME_SOURCE=omb + the guarded loader,
        // and no native theme pair or template override.
        assert!(rc.contains("OSH_THEME='robbyrussell'"), "{rc}");
        assert!(rc.contains("NIU_THEME_SOURCE=omb"), "{rc}");
        assert!(rc.contains("if [ -r "), "{rc}");
        assert!(rc.contains(". \"$OSH/oh-my-bash.sh\""), "{rc}");
        assert!(
            rc.contains("${NIU_PLUGIN_SOURCES_ROOT:-$HOME/.niubash/sources}/oh-my-bash"),
            "{rc}"
        );
        assert!(rc.contains("native themes stay as fallback"), "{rc}");
        assert!(!rc.contains("NIU_THEME="), "{rc}");
        assert!(!rc.contains("NIU_THEME_PLUGIN="), "{rc}");
        assert!(!rc.contains("niubash_prompt_use_template"), "{rc}");
        assert!(rc.contains("bash-compatible channel"), "{rc}");

        crate::plugins::sources::remove_source("oh-my-bash").unwrap();
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn wizard_native_theme_pick_writes_theme_pair() {
        let cfg = WizardConfig {
            theme: "p10-classic".to_string(),
            prompt_enabled: true,
            ..WizardConfig::default()
        };
        let rc = generate_rc(&cfg);
        assert!(rc.contains("NIU_THEME='p10-classic'"), "{rc}");
        assert!(rc.contains("NIU_THEME_PLUGIN='theme-p10-classic'"), "{rc}");
        assert!(!rc.contains("OSH_THEME="), "{rc}");
    }

    #[test]
    fn niu_git_answer_round_trip_gates_the_question() {
        let temp = unique_temp_dir("niu-git-answer");
        assert!(
            read_niu_git_answer(&temp).is_none(),
            "no answer recorded yet — the wizard may offer the choice"
        );
        write_niu_git_answer(&temp, "never");
        assert_eq!(read_niu_git_answer(&temp).as_deref(), Some("never"));
        let text = std::fs::read_to_string(wizard_answers_path(&temp)).unwrap();
        assert!(text.contains("niu_git = \"never\""), "{text}");
        assert!(text.contains(WIZARD_ANSWERS_SCHEMA), "{text}");
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn current_theme_pick_reads_existing_rc() {
        let _env_lock = PROCESS_STATE_LOCK.lock().unwrap();
        let _theme_env = EnvGuard::unset("NIU_THEME");
        let temp = unique_temp_dir("wizard-current-theme");
        std::fs::create_dir_all(&temp).unwrap();
        assert_eq!(current_theme_pick(&temp), ThemePick::Keep);

        std::fs::write(temp.join(PRIMARY_RC_FILE), "NIU_THEME='classic'\n").unwrap();
        assert_eq!(
            current_theme_pick(&temp),
            ThemePick::Native {
                name: "classic".to_string()
            }
        );

        std::fs::write(
            temp.join(PRIMARY_RC_FILE),
            "OSH_THEME='robbyrussell'\nNIU_THEME_SOURCE=omb\n",
        )
        .unwrap();
        assert_eq!(
            current_theme_pick(&temp),
            ThemePick::External {
                name: "robbyrussell".to_string(),
                source_id: "oh-my-bash".to_string()
            }
        );
        let _ = std::fs::remove_dir_all(&temp);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), nanos))
    }

    fn host_to_shell_style_path(path: &std::path::Path) -> String {
        let display = display_path(path);
        if cfg!(windows) && display.len() >= 3 && display.as_bytes()[1] == b':' {
            let drive = (display.as_bytes()[0] as char).to_ascii_lowercase();
            format!("/{drive}/{}", &display[3..])
        } else {
            display
        }
    }

    fn display_path(path: &std::path::Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

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

        fn unset(name: &'static str) -> Self {
            let previous = std::env::var_os(name);
            std::env::remove_var(name);
            Self { name, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.name, previous);
            } else {
                std::env::remove_var(self.name);
            }
        }
    }
}
