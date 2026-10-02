//! niubash#160: `niu -c 'echo "x'` exited silently (worse than reported: the
//! fast path folded the quote and executed `echo x`, rc=0) while GNU Bash
//! 5.3.0 reports `bash: -c: line 1: unexpected EOF while looking for
//! matching `"'` and exits 2.
//!
//! Root cause: `Shell::execute_script` (crates/niubash-runtime/src/shell.rs)
//! tokenized+parsed the command string directly, bypassing the engine's
//! read-time EOF diagnostic gate (rubash script_driver.rs run_source_impl —
//! GNU owner parse.y:5419-5437 read_token_word, error.c:324 exit status 2).
//! The fix inverts the fast path to a whitelist: unclosed syntax falls
//! through to the engine's real driver; the `-c` routes also set
//! __RUBASH_IS_C so the diagnostic takes the `$0: -c: line N:` shape.
//!
//! GNU baseline: WSL GNU Bash 5.3.0 script-file probe with per-shape stream
//! capture (16 shapes, rc and diagnostics byte-identical modulo the shell
//! name, which is $0 and therefore `niu` here — error.c get_name_for_error).

use std::path::PathBuf;
use std::process::Command;

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

fn run_c(command: &str) -> (String, String, i32) {
    let output = Command::new(niu_binary())
        .arg("-c")
        .arg(command)
        .output()
        .expect("spawn niu -c");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// The exact shape from the issue: no silent exit, no folded-word execution.
#[test]
fn unclosed_double_quote_reports_and_exits_2() {
    let (stdout, stderr, code) = run_c("echo \"x");
    assert_eq!(stdout, "", "the broken word must not execute");
    assert_eq!(
        stderr,
        "niu: -c: line 1: unexpected EOF while looking for matching `\"'\n"
    );
    assert_eq!(code, 2);
}

#[test]
fn unclosed_quote_family_reports() {
    for (command, expected) in [
        (
            "echo 'x",
            "niu: -c: line 1: unexpected EOF while looking for matching `''\n",
        ),
        (
            "echo `x",
            "niu: -c: line 1: unexpected EOF while looking for matching ``'\n",
        ),
        (
            "echo ${x",
            "niu: -c: line 1: unexpected EOF while looking for matching `}'\n",
        ),
    ] {
        let (stdout, stderr, code) = run_c(command);
        assert_eq!(stdout, "", "{command}: no stdout");
        assert_eq!(stderr, expected, "{command}: diagnostic");
        assert_eq!(code, 2, "{command}: rc");
    }

    // `$(` reports at the line after the open (GNU read_token shape).
    let (_, stderr, code) = run_c("echo $(x");
    assert_eq!(
        stderr,
        "niu: -c: line 2: unexpected EOF while looking for matching `)'\n"
    );
    assert_eq!(code, 2);
}

/// The heredoc-delimiter quote arm must fire before any heredoc gathers
/// (pre-fix behavior was the here-document-delimited-by-EOF warning, rc=0).
#[test]
fn heredoc_delimiter_unclosed_quote_reports_matching_quote() {
    let (_, stderr, code) = run_c("cat << \"q");
    assert_eq!(
        stderr,
        "niu: -c: line 1: unexpected EOF while looking for matching `\"'\n"
    );
    assert_eq!(code, 2);
}

/// `foo=([)` keeps GNU's rc=1 for the compound-assignment subscript shape.
#[test]
fn unclosed_compound_assignment_subscript_exits_1() {
    let (_, stderr, code) = run_c("foo=([)");
    assert_eq!(
        stderr,
        "niu: -c: line 1: unexpected EOF while looking for matching `]'\n"
    );
    assert_eq!(code, 1);
}

/// Complete prefix lines still execute; same-line `;` prefixes do not.
#[test]
fn complete_prefix_lines_execute_before_diagnostic() {
    let (stdout, stderr, code) = run_c("echo multi1\necho multi2\necho \"x");
    assert_eq!(stdout, "multi1\nmulti2\n");
    assert_eq!(
        stderr,
        "niu: -c: line 3: unexpected EOF while looking for matching `\"'\n"
    );
    assert_eq!(code, 2);

    let (stdout, _, code) = run_c("echo before; echo \"x");
    assert_eq!(stdout, "");
    assert_eq!(code, 2);
}

/// The $0 word after the command string names the diagnostic prefix.
#[test]
fn command_name_word_becomes_diagnostic_prefix() {
    let output = Command::new(niu_binary())
        .arg("-c")
        .arg("echo \"x")
        .arg("myzero")
        .output()
        .expect("spawn niu -c with name");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "myzero: -c: line 1: unexpected EOF while looking for matching `\"'\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

/// The script-file route shares execute_script and must report too: prefix
/// lines run, then the diagnostic names the script, rc=2.
#[test]
fn script_file_unclosed_quote_reports() {
    let dir = std::env::temp_dir().join(format!("niu-issue160-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let script = dir.join("unclosed.sh");
    std::fs::write(&script, b"echo a\necho b\necho \"x\n").expect("write script");

    let output = Command::new(niu_binary())
        .arg(&script)
        .output()
        .expect("spawn niu script");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "a\nb\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.ends_with(": line 3: unexpected EOF while looking for matching `\"'\n"),
        "diagnostic names the script and the open line: {stderr:?}"
    );
    assert_eq!(output.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Whitelist guards: provably-closed input keeps the local fast path — a
/// closed heredoc with an apostrophe in its body (`<<` excludes the generic
/// arm, the closed delimiter excludes arm 1) must still execute, and normal
/// commands must be untouched.
#[test]
fn closed_heredoc_and_normal_input_still_execute() {
    let (stdout, stderr, code) = run_c("cat << 'EOF'\nit's fine\nEOF\necho done");
    assert_eq!(stdout, "it's fine\ndone\n");
    assert_eq!(stderr, "");
    assert_eq!(code, 0);

    let (stdout, _, code) = run_c("echo ok; x=5; echo $((x+1))");
    assert_eq!(stdout, "ok\n6\n");
    assert_eq!(code, 0);
}
