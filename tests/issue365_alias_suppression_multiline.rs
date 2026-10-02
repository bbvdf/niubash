//! unixwin/rubash#365: `\cmd` alias suppression inside a multi-line
//! function body recursed infinitely (Windows STATUS_STACK_OVERFLOW,
//! `thread 'niu-main' has overflowed its stack`) through niu's `-C`
//! REPL-command route — the fnm/nvm/zoxide `cd`-wrapper idiom
//! (`__fnmcd() { \cd "$@" || return $?; ... }; alias cd=__fnmcd`) killed
//! the shell on every later `cd`.
//!
//! Root cause: `execute_interactive_line` ran the whole multi-line buffer
//! through the whole-buffer tokenize+parse fast path, whose alias
//! expansion is deferred to execution time against the LIVE table
//! (`expand_aliases_with_raw` over UNSTREAMED_FUNCTION_BODIES bodies).
//! GNU expands aliases while READING, one complete command at a time
//! (parse.y:5756 read_token_word tail `expand_aliases && quoted == 0`,
//! with `quoted` set by ANY backslash or quote in the word, parse.y:5321
//! -5324 and the backslash arm 5366-5397), so a function body is
//! alias-checked once, at definition time, and never re-checked when it
//! runs. The deferred model re-checked the body's `\cd` against the table
//! as of the CALL (which by then held `alias cd=f`) and the executor's
//! quote test does not count a bare backslash as quoting, so `\cd`
//! expanded to `f` -> f -> f -> ... until the stack died. A plain `cd`
//! body diverged the same way (no backslash needed) whenever the alias
//! was defined after the body.
//!
//! Fix: `Shell::execute_interactive_reader_batch` routes alias-live
//! interactive batches that are not a single simple command through the
//! engine's grouped reader (`script_driver::run_script_with_history`,
//! the same driver rubash's own `-i -c` uses), which expands each command
//! group's text against the table live at read time and marks the result
//! alias-streamed so nothing expands twice. Whitelist admission: only
//! provably single-simple-command input keeps the host fast path.
//!
//! GNU baseline (WSL GNU Bash 5.3.0, `bash -i <script>` per shape, file
//! based per the AGENTS script-file rule): variants A-H below all print
//! SURVIVED with rc 0. The sibling shape where GNU itself recurses
//! (`alias cd=f` BEFORE a plain `f() { cd ...; }` definition — the alias
//! legitimately rewrites the body at read time, GNU dies with SIGSEGV
//! rc 11) is intentionally NOT pinned as a success here.
//!
//! Every case runs under a hard timeout: the bug's failure shape is a
//! stack overflow (fast death) but any regression that turns it into an
//! unbounded loop must fail the suite quickly instead of hanging it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Stack-overflow / infinite-recursion shapes must not hang the suite.
const CASE_TIMEOUT: Duration = Duration::from_secs(90);

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

/// Run `niu --norc -C <command>` with piped stdio and a hard deadline.
/// Returns (stdout, stderr, exit_code). Panics when the child outlives
/// the deadline (kill + fail) so a recursion regression can never hang
/// the test binary.
fn run_repl_command_guarded(command: &str) -> (String, String, Option<i32>) {
    let temp = unique_temp_dir("niu-issue365");
    std::fs::create_dir_all(temp.join("home")).expect("create home");
    let mut child = Command::new(niu_binary())
        .arg("--norc")
        .arg("-C")
        .arg(command)
        .current_dir(temp.join("home"))
        .env("HOME", temp.join("home"))
        .env("USERPROFILE", temp.join("home"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("failed to spawn niu -C: {err}"));

    let deadline = Instant::now() + CASE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_dir_all(&temp);
                    panic!(
                        "niu -C did not finish within {CASE_TIMEOUT:?} — recursion hang? \
                         command: {command:?}"
                    );
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(err) => {
                let _ = std::fs::remove_dir_all(&temp);
                panic!("wait failed: {err}");
            }
        }
    };

    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout).expect("read stdout");
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr).expect("read stderr");
    }
    let _ = std::fs::remove_dir_all(&temp);
    (stdout, stderr, status.code())
}

fn assert_survived(command: &str, context: &str) {
    let (stdout, stderr, code) = run_repl_command_guarded(command);
    assert_eq!(
        stdout, "SURVIVED\n",
        "{context}: stdout mismatch (command: {command:?})"
    );
    assert!(
        !stderr.contains("overflowed its stack"),
        "{context}: stack overflow regression (stderr: {stderr:?})"
    );
    assert_eq!(
        code,
        Some(0),
        "{context}: exit code mismatch (stderr: {stderr:?})"
    );
}

/// Variant A — the exact reproducer from the issue: multi-line function
/// body using `\cd`, alias defined after the body, then a call through
/// the alias. Pre-fix: stack overflow, exit 0xC00000FD, no SURVIVED.
#[test]
fn multiline_backslash_suppression_after_alias_definition_survives() {
    assert_survived(
        "f() { \\cd \"$@\"; }\nalias cd=f\ncd \"$HOME\"\necho SURVIVED",
        "rubash#365 variant A (issue reproducer)",
    );
}

/// The deeper divergence the same routing fixes: a PLAIN `cd` body (no
/// backslash) defined before the alias must keep calling the builtin —
/// GNU parses the body at definition time when no alias exists yet.
#[test]
fn multiline_plain_body_before_alias_definition_survives() {
    assert_survived(
        "f() { cd \"$@\"; }\nalias cd=f\ncd \"$HOME\"\necho SURVIVED",
        "rubash#365 plain-body sibling",
    );
}

/// `command cd` suppression and the single-line form (GNU variant B/C).
#[test]
fn command_form_and_single_line_shapes_survive() {
    assert_survived(
        "f() { command cd \"$@\"; }\nalias cd=f\ncd \"$HOME\"\necho SURVIVED",
        "rubash#365 variant B (command cd)",
    );
    assert_survived(
        "f() { \\cd \"$@\"; }; alias cd=f; cd \"$HOME\"; echo SURVIVED",
        "rubash#365 variant C (single line)",
    );
}

/// Alias defined BEFORE the definition: `\` suppression must hold
/// (GNU parses the body with the alias live, but the backslash makes the
/// word quoted for alias purposes — parse.y:5756 quoted == 0).
#[test]
fn alias_before_backslash_definition_shapes_survive() {
    assert_survived(
        "alias cd=f\nf() { \\cd \"$@\"; }\ncd \"$HOME\"\necho SURVIVED",
        "rubash#365 alias-first multi-line",
    );
    assert_survived(
        "alias cd=f\nf() { \\cd \"$@\"; }; cd \"$HOME\"; echo SURVIVED",
        "rubash#365 alias-first single-line",
    );
}

/// No-alias and never-called variants (GNU variant D/E) plus the fnm
/// shape from the issue report (multi-statement body, `|| return $?`).
#[test]
fn no_alias_and_fnm_wrapper_shapes_survive() {
    assert_survived(
        "f() { \\cd \"$@\"; }\nf \"$HOME\"\necho SURVIVED",
        "rubash#365 variant D (no alias)",
    );
    assert_survived(
        "f() { \\cd \"$@\"; }\nalias cd=f\necho SURVIVED",
        "rubash#365 variant E (never called)",
    );
    assert_survived(
        concat!(
            "__use_if_found() {\n",
            "    if [[ -f .node-version || -f .nvmrc ]]; then\n",
            "        echo would-switch\n",
            "    fi\n",
            "}\n",
            "__fnmcd() {\n",
            "    \\cd \"$@\" || return $?\n",
            "    __use_if_found\n",
            "}\n",
            "alias cd=__fnmcd\n",
            "cd \"$HOME\"\n",
            "echo SURVIVED\n"
        ),
        "rubash#365 fnm wrapper shape",
    );
}

/// The reader route must not disturb ordinary multi-command execution:
/// loops, pipelines, `&&` chains and function definitions still run, and
/// a single simple command keeps the host fast path.
#[test]
fn driver_route_and_fast_path_shapes_still_execute() {
    let (stdout, stderr, code) = run_repl_command_guarded(
        "for i in 1 2; do printf 'it%s\\n' \"$i\"; done | tr a-z A-Z; echo SURVIVED",
    );
    assert_eq!(stdout, "IT1\nIT2\nSURVIVED\n");
    assert_eq!(stderr, "");
    assert_eq!(code, Some(0), "driver route for-pipeline");

    let (stdout, stderr, code) =
        run_repl_command_guarded("cd . && printf 'and-ok\\n' && echo SURVIVED");
    assert_eq!(stdout, "and-ok\nSURVIVED\n");
    assert_eq!(stderr, "");
    assert_eq!(code, Some(0), "driver route && chain");

    let (stdout, stderr, code) = run_repl_command_guarded("echo fastpath");
    assert_eq!(stdout, "fastpath\n");
    assert_eq!(stderr, "");
    assert_eq!(code, Some(0), "single simple command keeps the fast path");
}

/// An alias defined on one -C line and used on a LATER line must expand:
/// the grouped reader consults the live table per command group, exactly
/// like GNU's read-one-command/execute-one-command loop (the multi-line
/// `\cd` shapes above are the suppression half of the same rule).
#[test]
fn later_line_alias_use_expands_through_reader_route() {
    assert_survived("alias e=echo\ne SURVIVED", "later-line alias use expands");

    // The alias only exists once its command has EXECUTED, so a later
    // command through the alias resolves the function defined before it.
    let (stdout, _, code) =
        run_repl_command_guarded("g() { printf 'fn:%s\\n' \"$1\"; }\nalias run=g\nrun SURVIVED");
    assert_eq!(stdout, "fn:SURVIVED\n");
    assert_eq!(code, Some(0));
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{}", std::process::id(), nanos))
}

// Silence the unused-import lint when Path is only used on some platforms.
#[allow(dead_code)]
fn _unused(_: &Path) {}
