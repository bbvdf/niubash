//! niubash#156: `xargs` must stop parsing its own options at the first
//! non-option operand — the utility name — and pass every later argument to
//! that utility verbatim (POSIX Shell & Utilities, xargs; GNU findutils
//! behaves the same).
//!
//! STATUS: upstream binary gap, NOT a niubash/engine bug. Reproduced
//! standalone by spawning the WinuxCmd xargs binary directly (no shell in
//! between): `xargs cat -n` reports `xargs: option '-n' requires an
//! argument` rc=1, `xargs wc -l` silently eats `-l` as xargs's own
//! max-lines and prints a wrong three-column count, `xargs echo -- help`
//! swallows the `--`. The shell side is proven clean by the controls: the
//! same utility exes work through `find -exec`, and through a GNU-compatible
//! xargs on PATH the shell hands the arguments over untouched.
//!
//! Root cause (WinuxCmd source, read-only reference):
//! `src/core/command_context.cppm` `option_policy_for_command` sets
//! `stop_options_after_positionals` for timeout/getopt/nohup/printf but not
//! for xargs, so the shared parser (`src/core/opt.cppm:623`) keeps parsing
//! utility arguments as xargs options. Upstream issue filed with the
//! reproduction matrix and the one-line suggested fix:
//! https://github.com/unixwin/WinuxCmd/issues/1139
//!
//! These tests are #[ignore]d (house pattern for tests needing winuxcmd
//! command links on PATH, cf. tests/compat.rs) and pin the GNU-verified
//! targets; they pass against a fixed WinuxCmd xargs and fail against the
//! buggy one, so they double as the product-side verification for the
//! upstream fix. GNU baseline (WSL GNU Bash 5.3.0 environment, streams
//! separated) is recorded in each case.

use std::path::{Path, PathBuf};
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

/// Run `niu -c <script>` inside a prepared fixture directory (f1.txt/f2.txt
/// exist there). stderr is returned separately, GNU-style.
fn run_niu_c(script: &str, cwd: &Path) -> Output {
    let mut command = Command::new(niu_binary());
    command
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .stdin(Stdio::null());
    command
        .output()
        .unwrap_or_else(|err| panic!("spawn niu: {err}"))
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn fixture() -> PathBuf {
    let dir = unique_temp_dir("niu-156-xargs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f1.txt"), "one\n").unwrap();
    std::fs::write(dir.join("f2.txt"), "two\n").unwrap();
    dir
}

/// `xargs cat -n` must number the lines: GNU prints `     1\tone`, rc=0.
#[test]
#[ignore = "needs a fixed WinuxCmd xargs on PATH (upstream \
            unixwin/WinuxCmd#1139); the current one eats -n"]
fn xargs_passes_dash_n_to_cat() {
    let dir = fixture();
    let output = run_niu_c(r#"printf 'f1.txt\n' | xargs cat -n"#, &dir);
    assert_eq!(stdout_of(&output), "     1\tone");
    assert_eq!(stderr_of(&output), "");
    assert_eq!(output.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `xargs wc -l` must count lines per file plus the total — not treat `-l`
/// as xargs's own max-lines and print a wrong three-column count.
#[test]
#[ignore = "needs a fixed WinuxCmd xargs on PATH (upstream \
            unixwin/WinuxCmd#1139); the current one eats -l silently"]
fn xargs_passes_dash_l_to_wc() {
    let dir = fixture();
    let output = run_niu_c(r#"printf 'f1.txt\nf2.txt\n' | xargs wc -l"#, &dir);
    assert_eq!(stdout_of(&output), "1 f1.txt\n1 f2.txt\n2 total");
    assert_eq!(stderr_of(&output), "");
    assert_eq!(output.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A literal `--` operand belongs to the utility: GNU prints `-- help a`.
#[test]
#[ignore = "needs a fixed WinuxCmd xargs on PATH (upstream \
            unixwin/WinuxCmd#1139); the current one swallows the --"]
fn xargs_passes_literal_double_dash_to_echo() {
    let dir = fixture();
    let output = run_niu_c(r#"printf 'a\n' | xargs echo -- help"#, &dir);
    assert_eq!(stdout_of(&output), "-- help a");
    assert_eq!(output.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Option-looking utility arguments after xargs's own options: `-s` is
/// echo's data, GNU prints `-s a`.
#[test]
#[ignore = "needs a fixed WinuxCmd xargs on PATH (upstream \
            unixwin/WinuxCmd#1139); the current one parses -s as its own"]
fn xargs_dash_l1_then_utility_option() {
    let dir = fixture();
    let output = run_niu_c(r#"printf 'a\n' | xargs -L1 echo -s"#, &dir);
    assert_eq!(stdout_of(&output), "-s a");
    assert_eq!(output.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}
