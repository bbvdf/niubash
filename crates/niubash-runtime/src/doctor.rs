//! `niu doctor`: one-command health check for a Niubash installation.
//!
//! Reuses the same probes the setup wizard trusts (winuxcmd discovery,
//! command links, nerd font) and prints a compact report with fix hints.
//! Critical checks decide the trailing count; advisory rows (font, shims,
//! language) never fail the run.

use std::io::{self, Write};
use std::path::PathBuf;

const OK: &str = "\x1b[1;92m✅\x1b[0m";
const WARN: &str = "\x1b[1;93m⚠️\x1b[0m";
const INFO: &str = "\x1b[1;96mℹ️\x1b[0m";

/// Run all checks and print the report. Colors are dropped when stdout is
/// not a terminal so piped output stays plain.
pub fn run_doctor() -> anyhow::Result<()> {
    let color = crate::terminal::stdout_is_terminal();
    let (ok, warn, info) = if color {
        (OK, WARN, INFO)
    } else {
        ("OK", "ADVICE", "INFO")
    };
    let mut out = io::stdout();
    let mut critical_passed = 0usize;
    let mut critical_total = 0usize;

    if color {
        writeln!(
            out,
            "\x1b[1mniubash doctor\x1b[0m — {}",
            env!("CARGO_PKG_VERSION")
        )?;
    } else {
        writeln!(out, "niubash doctor — {}", env!("CARGO_PKG_VERSION"))?;
    }
    writeln!(out)?;

    // ── Critical: the Unix command core ────────────────────────────────────
    critical_total += 1;
    match crate::winuxcmd::find_winuxcmd() {
        Some(exe) => {
            let version = crate::winuxcmd::version()
                .map(|v| format!(" (winuxcmd {v})"))
                .unwrap_or_default();
            writeln!(out, "  {ok} winuxcmd core       {}{version}", display(&exe))?;
            critical_passed += 1;

            // The bash/sh forwarder shims ship beside the winuxcmd tree.
            let shim = exe
                .parent()
                .map(|dir| dir.join("bash.exe"))
                .filter(|p| p.is_file());
            match shim {
                Some(_) => writeln!(
                    out,
                    "  {info} bash/sh shims       present — `bash` and `sh` forward to niu"
                )?,
                None => writeln!(
                    out,
                    "  {info} bash/sh shims       not beside winuxcmd (optional)"
                )?,
            }
        }
        None => writeln!(
            out,
            "  {warn} winuxcmd core       not found — reinstall Niubash or check PATH"
        )?,
    }

    // ── Critical: command links on PATH ────────────────────────────────────
    critical_total += 1;
    if crate::winuxcmd::command_links_ready() {
        let count = crate::winuxcmd::list_commands().len();
        writeln!(
            out,
            "  {ok} command links       {count} commands (ls, cat, grep, …)"
        )?;
        critical_passed += 1;
    } else {
        writeln!(
            out,
            "  {warn} command links       missing (ls/cat/grep) — restart niu or run `wpm links rebuild`"
        )?;
    }

    // ── Critical: interactive startup rc ───────────────────────────────────
    critical_total += 1;
    let home = crate::path_utils::shell_home_dir().unwrap_or_else(|| PathBuf::from("."));
    let rc = home.join(".niubashrc");
    if rc.is_file() {
        writeln!(out, "  {ok} startup rc          {}", display(&rc))?;
        critical_passed += 1;
    } else if home.join(".winuxshrc").is_file() {
        writeln!(
            out,
            "  {ok} startup rc          {} (legacy, migrates on next start)",
            display(&home.join(".winuxshrc"))
        )?;
        critical_passed += 1;
    } else {
        writeln!(
            out,
            "  {warn} startup rc          none — run `niu setup` to create ~/.niubashrc"
        )?;
    }

    // ── Advisory rows ───────────────────────────────────────────────────────
    if crate::fonts::nerd_font_installed() {
        writeln!(
            out,
            "  {ok} nerd font           detected — icon themes unlocked"
        )?;
    } else {
        writeln!(
            out,
            "  {info} nerd font           not found — icon themes need one: `niu font`"
        )?;
    }

    let terminal = if std::env::var_os("WT_SESSION").is_some() {
        "Windows Terminal"
    } else {
        "console host"
    };
    writeln!(out, "  {info} terminal            {terminal}")?;

    if crate::setup_wizard::wizard_lang_is_chinese() {
        writeln!(
            out,
            "  {info} language            zh (wizard follows it; override with NIU_LANG)"
        )?;
    }

    writeln!(out)?;
    if critical_passed == critical_total {
        writeln!(
            out,
            "  {ok} {critical_passed}/{critical_total} critical checks passed"
        )?;
    } else {
        writeln!(
            out,
            "  {warn} {critical_passed}/{critical_total} critical checks passed — fix the rows above"
        )?;
    }
    out.flush()?;
    Ok(())
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
