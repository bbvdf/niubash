//! niubash#117 (reopened): a PS1 inherited from the process environment that
//! carries Git Bash (MSYS2) session machinery — the `__git_ps1` function from
//! git-prompt.sh, `$MSYSTEM`, the bracketed OSC title skeleton — must not be
//! adopted by prompt-rendering modes (`niu` REPL, `niu -C`). The reopened
//! v1.2.4 evidence showed detection shipping without the discard step: the
//! prompt kept the Git Bash shape and `__git_ps1` was expanded on every
//! render, printing `niu: __git_ps1: command not found` to stderr.
//!
//! Fixed in `Shell::enter_interactive` (+ the piped `-i` engine path): the
//! foreign value is discarded before the startup rc runs, so the shell's own
//! theme renders and the machinery is never expanded. A PS1 set by the user's
//! own rc, and env-var visibility for non-interactive `niu -c`, are unchanged.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn niu_binary() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_BIN_EXE_niu"));
    if p.exists() {
        return p;
    }
    let mut fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fallback.push("target");
    fallback.push("debug");
    fallback.push(if cfg!(windows) { "niu.exe" } else { "niu" });
    fallback
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), nanos))
}

/// Spawn niu with a hermetic HOME, an optional PS1 in the environment, and
/// `--norc` so the machine's own ~/.niubashrc cannot overwrite PS1 (mirrors
/// the issue reporter's broken-rc setup, where the rc failed to parse and the
/// inherited PS1 therefore stayed in charge).
fn run_niu(ps1: Option<&str>, args: &[&str]) -> Output {
    let home = unique_temp_dir("niu-issue117-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(niu_binary());
    command
        .args(args)
        .env("HOME", &home)
        .env_remove("NIU_ENV")
        .env_remove("BASH_ENV")
        .stdin(Stdio::null());
    match ps1 {
        Some(value) => command.env("PS1", value),
        None => command.env_remove("PS1"),
    };
    command
        .output()
        .unwrap_or_else(|err| panic!("spawn niu: {err}"))
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// The exact minimal reproducer from the reopened issue: inherited PS1 with
/// the Git Bash OSC-title skeleton plus `__git_ps1`, driven through `-C`.
#[test]
fn repl_command_with_foreign_ps1_is_quiet_and_runs() {
    let output = run_niu(
        Some(r"\[\033]0;x\007\]`__git_ps1` $ "),
        &["--norc", "-C", "echo repl-ok"],
    );
    assert_eq!(stdout_of(&output), "repl-ok");
    assert!(
        stderr_of(&output).is_empty(),
        "foreign PS1 must not be expanded: {}",
        stderr_of(&output)
    );
    assert_eq!(output.status.code(), Some(0));
}

/// The full PS1 Git for Windows 2.54 exports into child processes (from the
/// issue report), containing `$TITLEPREFIX`, `$MSYSTEM`, and `__git_ps1`.
#[test]
fn full_git_bash_default_ps1_is_discarded() {
    let git_bash_default = concat!(
        r"\[\033]0;$TITLEPREFIX:$PWD\007\]",
        r"\n\[\033[32m\]\u@\h ",
        r"\[\033[35m\]$MSYSTEM ",
        r"\[\033[33m\]\w\[\033[36m\]`__git_ps1`\[\033[0m\]",
        r"\n$ "
    );
    let output = run_niu(Some(git_bash_default), &["--norc", "-C", "echo repl-ok"]);
    assert_eq!(stdout_of(&output), "repl-ok");
    assert!(
        stderr_of(&output).is_empty(),
        "Git Bash session PS1 must not be expanded: {}",
        stderr_of(&output)
    );
    assert_eq!(output.status.code(), Some(0));
}

/// Control from the issue: no PS1 in the environment stays quiet.
#[test]
fn repl_command_without_ps1_stays_quiet() {
    let output = run_niu(None, &["--norc", "-C", "echo repl-ok"]);
    assert_eq!(stdout_of(&output), "repl-ok");
    assert!(stderr_of(&output).is_empty());
}

/// A custom PS1 without Git Bash session machinery is user content: prompt
/// modes adopt it (no error — nothing foreign to expand), and `niu -c` still
/// sees the inherited value verbatim (GNU parity for non-interactive shells).
#[test]
fn custom_ps1_is_user_content_and_stays_visible() {
    let output = run_niu(Some(r"\u@\h:\w\$ "), &["--norc", "-C", "echo custom-ok"]);
    assert_eq!(stdout_of(&output), "custom-ok");
    assert!(stderr_of(&output).is_empty());

    let visible = run_niu(Some(r"\u@\h:\w\$ "), &["-c", r#"echo "[$PS1]""#]);
    assert_eq!(stdout_of(&visible), r"[\u@\h:\w\$ ]");
}

/// Non-interactive `niu -c` never passes through enter_interactive, so even
/// the foreign value remains env-visible there (the issue's diagnostic
/// control #4 pinned exactly this and must keep holding).
#[test]
fn non_interactive_c_mode_keeps_foreign_ps1_visible() {
    let visible = run_niu(
        Some(r"`__git_ps1` $ "),
        &["-c", r#"test "$PS1" = '`__git_ps1` $ ' && echo visible"#],
    );
    assert_eq!(stdout_of(&visible), "visible");
}
