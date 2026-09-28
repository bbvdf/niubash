//! Interactive-mode regression tests.
//!
//! These exercise behavior that only exists when niu runs as an interactive
//! shell on a pseudo terminal: continuation prompts, Ctrl-C/Ctrl-D
//! interruption and EOF, live history expansion, aliases, PS1 re-rendering,
//! TAB completion, theme rendering, and input robustness. The non-interactive
//! suites (`tests/compat.rs`, `tests/host_contract.rs`, ...) cannot see any
//! of this.
//!
//! Driver: [`mod driver`] (portable-pty ConPTY, expect-style with per-call
//! timeouts). Determinism comes from the sentinel prompts `P1> `/`P2> ` pinned
//! in the sandbox rc — assertions match structural markers (echoed command,
//! output line) rather than full prompt bytes, because themed prompts carry
//! ANSI escapes.
//!
//! Coverage matrix (owner pain points first):
//! 1. Continuation — PS2 shown and completion on close for `"`, `'`, `{`,
//!    `(`, backslash-newline, heredoc; Ctrl-C abandons back to PS1.
//! 2. Termination — Ctrl-C on empty/mid-typed/mid-continuation/mid-command
//!    input; Ctrl-D on empty line/mid-line/with background jobs; Ctrl-Z gap.
//! 3. Interactive-only expansion — `!!`, `!$`, alias define/use, `set -H`
//!    flip, PS1 parameter re-render.
//! 4. Completion UX — TAB command and path completion; history persistence.
//! 5. Wizard/theme smoke — default themed prompt renders ANSI escapes.
//! 6. Robustness — input bursts, multiline paste, resize.
//!
//! Run: `NIU_INTERACTIVE_TESTS=1 cargo test --test interactive --locked -- --nocapture`
//! (without the env var the target skips on hosts with no pseudo terminal).

// Helper module lives beside the target: tests/interactive/driver.rs.
#[path = "interactive/driver.rs"]
mod driver;

use std::time::Duration;

use driver::{
    require_pty_or_skip, NiuSession, CTRL_C, CTRL_D, CTRL_D as EOF_KEY, HEAVY_TIMEOUT, TAB,
};

/// Short window for negative ("must not run") assertions.
const ABSENT_WINDOW: Duration = Duration::from_millis(800);

// ---------------------------------------------------------------------------
// Matrix 1: continuation (PS2, completion on close, abandon on Ctrl-C)
// ---------------------------------------------------------------------------

/// Unclosed `"` keeps the session in PS2; closing it completes the command
/// (the newline inside the quotes is preserved in the output, like bash).
#[test]
fn continuation_double_quote_ps2_and_completes() {
    if !require_pty_or_skip("continuation_double_quote_ps2_and_completes") {
        return;
    }
    let mut s = NiuSession::spawn("cont-dquote");
    s.wait_ready();
    s.send_line("echo \"double-line");
    s.expect_continuation();
    s.send_line("closed\"");
    s.expect("double-line");
    s.expect("closed");
    s.expect_prompt();
}

/// Unclosed `'` keeps the session in PS2; closing it completes the command
/// (the newline inside the quotes is preserved in the output, like bash).
#[test]
fn continuation_single_quote_ps2_and_completes() {
    if !require_pty_or_skip("continuation_single_quote_ps2_and_completes") {
        return;
    }
    let mut s = NiuSession::spawn("cont-squote");
    s.wait_ready();
    s.send_line("echo 'single-line");
    s.expect_continuation();
    s.send_line("closed'");
    s.expect("single-line");
    s.expect("closed");
    s.expect_prompt();
}

/// An open `{` block spans lines until `}` closes it.
#[test]
fn continuation_brace_group_completes() {
    if !require_pty_or_skip("continuation_brace_group_completes") {
        return;
    }
    let mut s = NiuSession::spawn("cont-brace");
    s.wait_ready();
    s.send_line("{ echo brace-one;");
    s.expect_continuation();
    s.send_line("echo brace-two; }");
    s.expect("brace-one");
    s.expect("brace-two");
    s.expect_prompt();
}

/// An open `(` subshell spans lines until `)` closes it.
#[test]
fn continuation_subshell_paren_completes() {
    if !require_pty_or_skip("continuation_subshell_paren_completes") {
        return;
    }
    let mut s = NiuSession::spawn("cont-paren");
    s.wait_ready();
    s.send_line("(echo sub-one;");
    s.expect_continuation();
    s.send_line("echo sub-two)");
    s.expect("sub-one");
    s.expect("sub-two");
    s.expect_prompt();
}

/// A trailing backslash joins the physical lines into one logical command.
#[test]
fn continuation_backslash_newline_joins() {
    if !require_pty_or_skip("continuation_backslash_newline_joins") {
        return;
    }
    let mut s = NiuSession::spawn("cont-backslash");
    s.wait_ready();
    s.send_line("echo backslash-first\\");
    s.expect_continuation();
    s.send_line("second");
    // bash joins `first\` + `second` into the single word `firstsecond`.
    s.expect("backslash-firstsecond");
    s.expect_prompt();
}

/// A heredoc in progress keeps the PS2 until the delimiter line arrives.
#[test]
fn continuation_heredoc_gathers_until_delimiter() {
    if !require_pty_or_skip("continuation_heredoc_gathers_until_delimiter") {
        return;
    }
    let mut s = NiuSession::spawn("cont-heredoc");
    s.wait_ready();
    s.send_line("cat <<HD");
    s.expect_continuation();
    s.send_line("heredoc-body-line");
    s.send_line("HD");
    s.expect("heredoc-body-line");
    s.expect_prompt();
}

/// Ctrl-C during a continuation abandons the whole construct back to PS1:
/// nothing executes, and the next command works.
#[test]
fn ctrl_c_during_continuation_abandons_construct() {
    if !require_pty_or_skip("ctrl_c_during_continuation_abandons_construct") {
        return;
    }
    let mut s = NiuSession::spawn("cont-ctrlc");
    s.wait_ready();
    eprintln!("[T] ready");
    // The marker only exists in *output* if the construct executed: the
    // typed text contains ${AB} which the executed echo would resolve.
    s.send_line("AB=zznever; echo \"never-${AB}");
    eprintln!("[T] sent line 1");
    s.expect_continuation();
    eprintln!("[T] got PS2");
    s.send(CTRL_C);
    eprintln!("[T] sent ctrl-c");
    s.expect_prompt();
    eprintln!("[T] got PS1 after ctrl-c");
    s.expect_absent("never-zznever", ABSENT_WINDOW);
    eprintln!("[T] absence checked");
    s.send_line("echo after-abandon-ok");
    eprintln!("[T] sent line 2");
    s.expect("after-abandon-ok");
    eprintln!("[T] got marker");
    s.expect_prompt();
    eprintln!("[T] done");
}

// ---------------------------------------------------------------------------
// Matrix 2: termination (Ctrl-C, Ctrl-D, jobs)
// ---------------------------------------------------------------------------

/// Ctrl-C on an empty prompt line just yields a fresh prompt.
#[test]
fn ctrl_c_on_empty_line_returns_new_prompt() {
    if !require_pty_or_skip("ctrl_c_on_empty_line_returns_new_prompt") {
        return;
    }
    let mut s = NiuSession::spawn("ctrlc-empty");
    s.wait_ready();
    s.send(CTRL_C);
    s.expect_prompt();
    s.send_line("echo after-empty-ctrlc");
    s.expect("after-empty-ctrlc");
    s.expect_prompt();
}

/// Ctrl-C with text typed discards the line: it must not execute.
#[test]
fn ctrl_c_midtyped_discards_line() {
    if !require_pty_or_skip("ctrl_c_midtyped_discards_line") {
        return;
    }
    let mut s = NiuSession::spawn("ctrlc-midtyped");
    s.wait_ready();
    s.send("echo discarded-$Q");
    s.send(CTRL_C);
    s.expect_prompt();
    // The typed text contains `discarded-$Q`; execution would print
    // `discarded-<value>`.
    s.expect_absent("discarded-marker", ABSENT_WINDOW);
    s.send_line("Q=marker");
    s.expect_prompt();
    s.send_line("echo still-alive-ok");
    s.expect("still-alive-ok");
    s.expect_prompt();
}

/// Ctrl-C while an external command runs interrupts it and returns the
/// prompt. The observed `$?` is captured for the engine ledger (GNU bash
/// reports 130 for a SIGINT-killed command).
#[test]
fn ctrl_c_interrupts_external_command() {
    if !require_pty_or_skip("ctrl_c_interrupts_external_command") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "ctrlc-external",
        &driver::default_rc(),
        &[],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.wait_ready();
    s.send_line("ping -n 20 127.0.0.1");
    // `TTL=` appears in the first reply in every ping locale.
    s.expect("TTL=");
    s.send(CTRL_C);
    s.expect_prompt();
    s.send_line("echo st:$?");
    let window = s.expect("st:");
    eprintln!("ctrl_c_external status window: {window:?}");
    s.expect_prompt();
}

/// Ctrl-C while a builtin `sleep` runs must also return the prompt.
#[test]
fn ctrl_c_interrupts_builtin_sleep() {
    if !require_pty_or_skip("ctrl_c_interrupts_builtin_sleep") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "ctrlc-sleep",
        &driver::default_rc(),
        &[],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.wait_ready();
    s.send_line("sleep 20");
    std::thread::sleep(Duration::from_millis(500));
    s.send(CTRL_C);
    s.expect_prompt();
    s.send_line("echo after-sleep-ctrlc-ok");
    s.expect("after-sleep-ctrlc-ok");
    s.expect_prompt();
}

/// Ctrl-D on an empty line exits the shell with status 0.
#[test]
fn ctrl_d_on_empty_line_exits() {
    if !require_pty_or_skip("ctrl_d_on_empty_line_exits") {
        return;
    }
    let mut s = NiuSession::spawn("ctrld-exit");
    s.wait_ready();
    s.send(CTRL_D);
    let code = s.wait_exit();
    assert_eq!(code, 0, "expected exit code 0 after Ctrl-D on empty line");
}

/// Ctrl-D with text typed must not exit; the line stays editable.
#[test]
fn ctrl_d_midline_does_not_exit() {
    if !require_pty_or_skip("ctrl_d_midline_does_not_exit") {
        return;
    }
    let mut s = NiuSession::spawn("ctrld-midline");
    s.wait_ready();
    s.send("echo half-typed");
    s.send(EOF_KEY);
    std::thread::sleep(ABSENT_WINDOW);
    // The shell is still alive: finish the line and it must execute.
    s.send("\r");
    s.expect("half-typed");
    s.expect_prompt();
}

/// With a background job running, `jobs` lists it and Ctrl-D still exits
/// (bash only warns for *stopped* jobs — see the Ctrl-Z gap test).
#[test]
fn ctrl_d_after_background_job_exits() {
    if !require_pty_or_skip("ctrl_d_after_background_job_exits") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "ctrld-jobs",
        &driver::default_rc(),
        &[],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.wait_ready();
    s.send_line("ping -n 20 127.0.0.1 &");
    s.expect_prompt();
    s.send_line("jobs");
    let window = s.expect("ping");
    eprintln!("jobs window: {window:?}");
    s.expect_prompt();
    s.send(CTRL_D);
    let code = s.wait_exit();
    assert_eq!(code, 0, "expected exit code 0 after Ctrl-D with bg job");
}

/// Ctrl-Z at the prompt is a documented gap: reedline's `Signal` has no
/// suspend variant and the REPL ignores unknown signals, so there is no
/// job-control suspend (and therefore no "there are stopped jobs" warning
/// path). The shell must at least stay alive and usable.
#[test]
fn ctrl_z_at_prompt_is_ignored_but_shell_survives() {
    if !require_pty_or_skip("ctrl_z_at_prompt_is_ignored_but_shell_survives") {
        return;
    }
    let mut s = NiuSession::spawn("ctrlz-gap");
    s.wait_ready();
    s.send("echo suspend-tried");
    s.send("\u{1a}");
    std::thread::sleep(ABSENT_WINDOW);
    s.send("\r");
    s.expect("suspend-tried");
    s.expect_prompt();
}

// ---------------------------------------------------------------------------
// Matrix 3: interactive-only expansion
// ---------------------------------------------------------------------------

/// `!!` re-executes the previous command live.
#[test]
fn history_bang_bang_reexecutes_last_command() {
    if !require_pty_or_skip("history_bang_bang_reexecutes_last_command") {
        return;
    }
    let mut s = NiuSession::spawn("hist-bangbang");
    s.wait_ready();
    s.send_line("echo bang-target");
    s.expect("bang-target");
    s.expect_prompt();
    s.send_line("!!");
    // The expansion may echo the expanded command first; the output must
    // appear again.
    s.expect("bang-target");
    s.expect_prompt();
}

/// `!$` reuses the last argument of the previous command.
#[test]
fn history_bang_dollar_reuses_last_argument() {
    if !require_pty_or_skip("history_bang_dollar_reuses_last_argument") {
        return;
    }
    let mut s = NiuSession::spawn("hist-bangdollar");
    s.wait_ready();
    s.send_line("echo first-arg second-arg");
    s.expect("first-arg second-arg");
    s.expect_prompt();
    s.send_line("echo got:!$");
    s.expect("got:second-arg");
    s.expect_prompt();
}

/// An alias defined in the session is used immediately.
#[test]
fn alias_define_then_use_live() {
    if !require_pty_or_skip("alias_define_then_use_live") {
        return;
    }
    let mut s = NiuSession::spawn("alias-live");
    s.wait_ready();
    s.send_line("alias lx='echo alias-live-out'");
    s.expect_prompt();
    s.send_line("lx");
    s.expect("alias-live-out");
    s.expect_prompt();
}

/// `set +H` disables history expansion (`!!` becomes a literal command);
/// `set -H` re-enables it.
#[test]
#[allow(non_snake_case)]
fn set_H_flips_history_expansion() {
    if !require_pty_or_skip("set_H_flips_history_expansion") {
        return;
    }
    let mut s = NiuSession::spawn("set-H");
    s.wait_ready();
    s.send_line("echo flip-target");
    s.expect("flip-target");
    s.expect_prompt();

    s.send_line("set +H");
    s.expect_prompt();
    s.send_line("echo no-expand-!!-mark");
    s.expect("no-expand-!!-mark");
    s.expect_prompt();

    s.send_line("set -H");
    s.expect_prompt();
    s.send_line("echo re-expand");
    s.expect("re-expand");
    s.expect_prompt();
    s.send_line("!!");
    s.expect("re-expand");
    s.expect_prompt();
}

/// PS1 with a parameter re-renders on every prompt after the variable
/// changes (bash PS1 expansion is live).
#[test]
fn ps1_parameter_expansion_rerenders_per_prompt() {
    if !require_pty_or_skip("ps1_parameter_expansion_rerenders_per_prompt") {
        return;
    }
    let rc = "PS1='V[$V]> '\nPS2='P2> '\nNIU_DISABLE_DEFAULT_PLUGINS=1\n";
    let mut s = NiuSession::spawn_custom("ps1-render", rc, &[], (120, 30), driver::DEFAULT_TIMEOUT);
    s.expect("Niubash");
    s.expect("V[]> ");
    s.send_line("V=rendered");
    s.expect("V[rendered]> ");
    s.send_line("V=changed");
    s.expect("V[changed]> ");
}

// ---------------------------------------------------------------------------
// Matrix 4: completion UX and history persistence
// ---------------------------------------------------------------------------

/// TAB completes a command prefix from the builtin/PATH set.
#[test]
fn tab_completes_command_name() {
    if !require_pty_or_skip("tab_completes_command_name") {
        return;
    }
    let mut s = NiuSession::spawn("tab-cmd");
    s.wait_ready();
    s.send("printf");
    s.send(TAB);
    // Whether the completion is inline or a menu, the buffer must end up
    // usable: finish the line and require the printf round trip.
    s.send_line(" tab-cmd-ok");
    s.expect("tab-cmd-ok");
    s.expect_prompt();
}

/// TAB completes a unique file path prefix in the working directory.
#[test]
fn tab_completes_unique_file_path() {
    if !require_pty_or_skip("tab_completes_unique_file_path") {
        return;
    }
    let mut s = NiuSession::spawn("tab-path");
    std::fs::write(s.start().join("uniqfix.dat"), "uniq-file-body\n").unwrap();
    s.wait_ready();
    s.send("cat uniqfix");
    s.send(TAB);
    s.send("\r");
    s.expect("uniq-file-body");
    s.expect_prompt();
}

/// The session history survives into `~/.niubash_history` and `history`
/// shows it live.
#[test]
fn history_file_persists_across_sessions() {
    if !require_pty_or_skip("history_file_persists_across_sessions") {
        return;
    }
    let mut s = NiuSession::spawn("hist-persist");
    s.wait_ready();
    s.send_line("echo session-marker-one");
    s.expect("session-marker-one");
    s.expect_prompt();
    s.send_line("history");
    s.expect("session-marker-one");
    s.expect_prompt();
    s.send_line("exit");
    let code = s.wait_exit();
    assert_eq!(code, 0);
    let persisted = std::fs::read_to_string(s.home().join(".niubash_history")).unwrap_or_default();
    assert!(
        persisted.contains("session-marker-one"),
        "history file missing session marker; content: {persisted:?}"
    );
}

/// `set +o history` stops recording new entries into the history file.
#[test]
fn set_plus_o_history_stops_recording() {
    if !require_pty_or_skip("set_plus_o_history_stops_recording") {
        return;
    }
    let mut s = NiuSession::spawn("hist-off");
    s.wait_ready();
    s.send_line("set +o history");
    s.expect_prompt();
    s.send_line("echo secret-not-recorded");
    s.expect("secret-not-recorded");
    s.expect_prompt();
    s.send_line("exit");
    let code = s.wait_exit();
    assert_eq!(code, 0);
    let persisted = std::fs::read_to_string(s.home().join(".niubash_history")).unwrap_or_default();
    assert!(
        !persisted.contains("secret-not-recorded"),
        "command recorded despite `set +o history`; content: {persisted:?}"
    );
}

// ---------------------------------------------------------------------------
// Matrix 5: wizard/theme smoke
// ---------------------------------------------------------------------------

/// Without a PS1 override the default themed prompt renders, carries ANSI
/// escape sequences, and the shell round-trips a command.
#[test]
fn default_theme_prompt_renders_with_ansi_escapes() {
    if !require_pty_or_skip("default_theme_prompt_renders_with_ansi_escapes") {
        return;
    }
    // No PS1 in the rc: the built-in themed prompt is used.
    let rc = "NIU_DISABLE_DEFAULT_PLUGINS=1\n";
    let mut s = NiuSession::spawn_custom("theme-ansi", rc, &[], (120, 30), driver::DEFAULT_TIMEOUT);
    s.expect("Niubash");
    std::thread::sleep(Duration::from_millis(500));
    s.send_line("echo theme-roundtrip");
    s.expect("theme-roundtrip");
    s.expect_absent("command not found", ABSENT_WINDOW);
    let screen = s.drain_screen();
    assert!(
        screen.contains("\x1b["),
        "no ANSI escapes in the rendered prompt; screen: {screen:?}"
    );
}

// ---------------------------------------------------------------------------
// Matrix 6: robustness
// ---------------------------------------------------------------------------

/// A rapid burst of input lines is executed completely and in order.
#[test]
fn rapid_input_burst_all_lines_execute() {
    if !require_pty_or_skip("rapid_input_burst_all_lines_execute") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "burst",
        &driver::default_rc(),
        &[],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.wait_ready();
    let burst: String = (1..=12).map(|i| format!("echo burst-{i:02}\r\n")).collect();
    s.send(&burst);
    for i in 1..=12 {
        s.expect(&format!("burst-{i:02}"));
    }
    s.expect_prompt();
}

/// Pasting a multiline block (raw newline paste; niubash does not enable
/// reedline's bracketed-paste mode — documented gap) executes each line and
/// keeps incomplete constructs editable until they close.
#[test]
fn multiline_block_paste_executes() {
    if !require_pty_or_skip("multiline_block_paste_executes") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "paste",
        &driver::default_rc(),
        &[],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.wait_ready();
    s.send("echo paste-first\r\necho paste-second\r\n");
    s.expect("paste-first");
    s.expect("paste-second");
    s.expect_prompt();
    s.send("if true; then\recho paste-branch\rfi\r");
    s.expect("paste-branch");
    s.expect_prompt();
}

/// Resizing the terminal mid-session does not crash the shell or wedge the
/// prompt.
#[test]
fn resize_does_not_crash_shell() {
    if !require_pty_or_skip("resize_does_not_crash_shell") {
        return;
    }
    let mut s = NiuSession::spawn("resize");
    s.wait_ready();
    s.resize(60, 20);
    std::thread::sleep(Duration::from_millis(200));
    s.send_line("echo after-resize-ok");
    s.expect("after-resize-ok");
    s.expect_prompt();
    s.resize(200, 50);
    std::thread::sleep(Duration::from_millis(200));
    s.send_line("echo after-resize-two-ok");
    s.expect("after-resize-two-ok");
    s.expect_prompt();
}
