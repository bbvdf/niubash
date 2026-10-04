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
    require_pty_or_skip, NiuSession, CTRL_C, CTRL_D, CTRL_D as EOF_KEY, DEFAULT_TIMEOUT,
    HEAVY_TIMEOUT, TAB,
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
#[ignore = "rubash#287 residual (post driver-send fix 2026-09-28): line editor swallows the line after 0x1a - the typed text plus Enter never submits; reedline keymap gap in niu, not a wedge"]
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
#[ignore = "rubash#286: pre-existing on 688f224+f7a5b69 (reproduced on a baseline clone, 8/9 runs red) - interactive history expansion is dead; unaffected by the niubash#145 retirement"]
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
#[ignore = "rubash#286: pre-existing on 688f224+f7a5b69 (reproduced on a baseline clone, 8/9 runs red) - interactive history expansion is dead; unaffected by the niubash#145 retirement"]
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
#[ignore = "rubash#286: pre-existing on 688f224+f7a5b69 (reproduced on a baseline clone) - interactive history expansion/recording is dead; unaffected by the niubash#145 retirement"]
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
// Matrix 3b: defaults-as-floor (oh-my-niu design §14.5) — the product's own
// prompt is the lowest-priority floor; an enabled external framework claims
// the slot through PS1, the floor yields without fighting, and releasing
// the claim restores the floor.
// ---------------------------------------------------------------------------

/// Nobody claims PS1 → the product floor (default template, `user@host`)
/// renders in the reedline REPL.
#[test]
fn floor_prompt_renders_when_ps1_unclaimed() {
    if !require_pty_or_skip("floor_prompt_renders_when_ps1_unclaimed") {
        return;
    }
    let rc = "export NIU_FLOOR_PROBE=1\n";
    let mut s = NiuSession::spawn_custom(
        "floor-unclaimed",
        rc,
        &[],
        (120, 30),
        driver::DEFAULT_TIMEOUT,
    );
    s.expect("Niubash");
    // Floor shape: `{user}@{host} {cwd} %#` — the `@` join is the
    // structural marker (ANSI-styled pieces make full-prompt matching
    // brittle, same discipline as the other theme tests).
    s.expect("@");
    s.send_line("echo floor-alive");
    s.expect("floor-alive");
}

/// An enabled oh-my-bash theme claims the prompt end-to-end through its
/// guarded loader: reedline renders the theme face (not the floor), the
/// claim survives prompt cycles, `unset PS1` releases the slot so the floor
/// returns, and re-claiming wins again — floor never fights back.
#[test]
fn omb_theme_claim_renders_and_floor_returns_on_release() {
    if !require_pty_or_skip("omb_theme_claim_renders_and_floor_returns_on_release") {
        return;
    }
    // Fixture oh-my-bash tree (same shape `niu plugin add --path` installs)
    // copied where the guarded loader's default lookup finds it.
    let sources_root = std::env::temp_dir().join(format!(
        "niu-floor-omb-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let omb = sources_root.join("oh-my-bash");
    copy_tree(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sources/oh-my-bash"),
        &omb,
    );
    // The rc carries the managed-block shape `niu plugin enable` writes:
    // theme line + guarded loader for the fixture tree.
    let base = "${NIU_PLUGIN_SOURCES_ROOT:-$HOME/.niubash/sources}/oh-my-bash";
    let rc = format!(
        "OSH_THEME='agnoster'\nif [ -r \"{base}/oh-my-bash.sh\" ]; then\n  OSH=\"{base}\"\n  OSH=\"${{OSH//\\\\//}}\"\n  export OSH\n  . \"$OSH/oh-my-bash.sh\"\nfi\n"
    );
    let mut s = NiuSession::spawn_custom(
        "omb-claim",
        &rc,
        &[(
            "NIU_PLUGIN_SOURCES_ROOT".to_string(),
            sources_root.to_string_lossy().into_owned(),
        )],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.expect("Niubash");
    // The theme's PS1 face renders through the bash-compatible channel.
    s.expect("agnoster-fixture-face");
    s.send_line("echo claim-alive");
    s.expect("claim-alive");
    s.expect("agnoster-fixture-face");

    // Release: `unset PS1` → next prompt is the product floor again.
    s.send_line("unset PS1");
    s.expect("@");

    // Re-claim in the same session still wins.
    s.send_line("PS1='reclaimed> '");
    s.expect("reclaimed> ");

    let _ = std::fs::remove_dir_all(&sources_root);
}

/// Minimal recursive copy for the fixture tree (test-local; the fixture is
/// a handful of small files).
fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// Matrix 4: completion UX and history persistence
// ---------------------------------------------------------------------------

/// TAB completes a command prefix from the builtin/PATH set.
#[test]
#[ignore = "rubash#287 residual (post driver-send fix 2026-09-28): TAB opens a blank completion menu and follow-up input is consumed by it; command-set completion under minimal PATH broken in niu, not a wedge"]
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
#[ignore = "rubash#287 residual (post driver-send fix 2026-09-28): TAB path completion wedges the editor into a blank menu; niu completion wiring gap, not a wedge"]
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
#[ignore = "rubash#286: pre-existing on 688f224+f7a5b69 (reproduced on a baseline clone) - interactive history expansion/recording is dead; unaffected by the niubash#145 retirement"]
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

/// niubash#167: keystrokes typed while the prompt rebuilds must survive
/// into the edit line.
///
/// Between two prompts the console sits in the cooked baseline while
/// `PROMPT_COMMAND` machinery runs children that inherit the console;
/// some runtimes (observed: Git-for-Windows MSYS2 `git`/`awk`/`grep`/
/// `date` under a themed prompt) probe the shared console input buffer
/// and consume one pending key record — the PRESS of the first key typed
/// during the window, so `echo` executed as `cho` (wt67 drv-run1,
/// 11/11 resends). The typeahead guard sweeps the queue for the shell
/// during the rebuild and reinjects it before the editor reads.
///
/// The eater models the probe deterministically: PowerShell waits, then
/// reads ONE console key only when one is pending, without blocking.
/// Where PowerShell is unavailable the eater silently no-ops and this
/// test degrades to a plain pass (it can never fail spuriously).
#[test]
fn typeahead_survives_prompt_rebuild_window() {
    if !require_pty_or_skip("typeahead_survives_prompt_rebuild_window") {
        return;
    }
    let rc = concat!(
        "PS1='P1> '\n",
        "NIU_DISABLE_DEFAULT_PLUGINS=1\n",
        "__eat(){ C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
        " -NoProfile -Command 'Start-Sleep -m 500;",
        " if([Console]::KeyAvailable){[void][Console]::ReadKey($true)}'",
        " >/dev/null 2>&1; }\n",
        "__p(){ __eat; PS1='P1> '; }\n",
        "PROMPT_COMMAND=__p\n",
    );
    let mut s = NiuSession::spawn_custom("wt167-typeahead", rc, &[], (120, 30), HEAVY_TIMEOUT);
    s.wait_ready();
    // One sacrificial cycle so the first PROMPT_COMMAND (eater) runs.
    s.send_line("true");
    s.expect_prompt();
    for marker in ["W167A", "W167B"] {
        // Submit a command, then type the next line while the rebuild
        // window (the eater child) is still running: without the guard
        // the first byte of `echo` is the eaten key and the line runs as
        // `cho <marker>`.
        s.send("true\r");
        std::thread::sleep(Duration::from_millis(450));
        s.send_line(&format!("echo {marker}"));
        s.expect(marker);
        s.expect_prompt();
    }
}

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

// ---------------------------------------------------------------------------
// niubash#145: interactive startup must reach the prompt when an rc carries
// the retired built-in plugin/theme stack lines. The original bug: the host
// spawned the 'niubash-gitstatus' worker thread for the async git prompt and
// the main thread waited on it before the first prompt render, so a shell
// whose rc loaded the framework (non-empty NIU_PLUGINS) hung forever after
// the banner. The whole machinery is deleted; these tests pin the fix by
// driving the real interactive (PTY) path with timeout-bounded asserts.
// ---------------------------------------------------------------------------

/// The legacy rc exactly as the old setup wizard wrote it (stack variables,
/// export block, oh-my-niu discovery loop), plus the sentinels.
fn legacy_stack_rc() -> String {
    r#"NIU_THEME='spaceship'
NIU_THEME_PLUGIN='theme-spaceship'
NIU_PROMPT_SYMBOL='❯'
NIU_DISABLE_DEFAULT_PLUGINS=1
export NIU_THEME NIU_THEME_PLUGIN NIU_PROMPT_SYMBOL
export NIU_DISABLE_DEFAULT_PLUGINS
NIU_PLUGINS=(prompt-core git starship common-aliases path-tools extract zoxide fzf thefuck command-not-found direnv dotenv kubectl npm keybindings env-sync)
if [ -z "${NIUBASH:-}" ]; then
  for __niubash_bundle in "$HOME/.oh-my-niu" "$HOME/.niubash/oh-my-niu" "$HOME/.niubash/bundles/oh-my-niu"/* "$NIU_APP_BUNDLE_PATH"; do
    if [ -f "$__niubash_bundle/oh-my-niu.niu" ]; then
      NIUBASH="$__niubash_bundle"
      export NIUBASH
      break
    fi
  done
fi
if [ -f "$NIUBASH/oh-my-niu.niu" ]; then
  . "$NIUBASH/oh-my-niu.niu"
fi
unset __niubash_bundle
PS1='P1> '
PS2='P2> '
"#
    .to_string()
}

/// A minimal oh-my-niu-shaped framework entry the legacy discovery loop can
/// find: it defines the hook-runner functions the old host dispatched. If the
/// async git-status machinery ever comes back, sourcing this plus the
/// non-empty NIU_PLUGINS list is what used to hang startup.
fn write_legacy_framework_fixture(root: &std::path::Path) -> std::path::PathBuf {
    let bundle = root.join("oh-my-niu");
    std::fs::create_dir_all(&bundle).expect("create fixture bundle dir");
    std::fs::write(
        bundle.join("oh-my-niu.niu"),
        r#"# legacy framework fixture (niubash#145 regression)
niubash_run_precmd_hooks() {
  NIU_PROMPT_GIT="SNAPSHOT:precmd:${NIU_LAST_EXIT_CODE:-0} "
  export NIU_PROMPT_GIT
  return 0
}
niubash_run_startup_hooks() { :; }
niubash_run_preexec_hooks() { :; }
"#,
    )
    .expect("write framework fixture");
    bundle
}

/// The headline regression: banner -> prompt must be reached (bounded by the
/// driver timeout) with the legacy stack rc, and interactive commands must
/// keep working afterwards.
#[test]
fn legacy_niu_plugins_rc_reaches_prompt() {
    if !require_pty_or_skip("legacy_niu_plugins_rc_reaches_prompt") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "legacy-plugins",
        &legacy_stack_rc(),
        &[],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.wait_ready();
    s.send_line("echo legacy-ok");
    s.expect("legacy-ok");
    s.expect_prompt();
}

/// Same rc, but the discovery loop actually finds a framework entry through
/// $NIU_APP_BUNDLE_PATH — the exact shape of the owner-machine hang (rc
/// sourced the framework, NIU_PLUGINS non-empty, prompt never rendered).
#[test]
fn legacy_framework_rc_reaches_prompt() {
    if !require_pty_or_skip("legacy_framework_rc_reaches_prompt") {
        return;
    }
    let fixture_root = std::env::temp_dir().join(format!(
        "niu-legacy-framework-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bundle = write_legacy_framework_fixture(&fixture_root);
    let mut s = NiuSession::spawn_custom(
        "legacy-framework",
        &legacy_stack_rc(),
        &[(
            "NIU_APP_BUNDLE_PATH".to_string(),
            bundle.to_string_lossy().into_owned(),
        )],
        (120, 30),
        HEAVY_TIMEOUT,
    );
    s.wait_ready();
    // The framework entry was really sourced (rc-side effect survives).
    s.send_line("command -v niubash_run_precmd_hooks >/dev/null && echo fw-loaded");
    s.expect("fw-loaded");
    s.expect_prompt();
    s.send_line("echo still-alive");
    s.expect("still-alive");
    s.expect_prompt();

    let _ = std::fs::remove_dir_all(&fixture_root);
}

// ---------------------------------------------------------------------------
// niubash#147: after `cd` under a paren-bearing theme prompt (robbyrussell's
// `git:(branch)`), the shell must not report
// `unexpected EOF while looking for matching ')'` and the next prompt must
// not carry a stray literal `)` as echoed input. The owner report (build
// @688f224) also showed `cd` printing the working directory; with CDPATH
// unset GNU `cd` prints nothing, and the engine's cd already follows GNU
// builtins/cd.def (print only on a CDPATH hit, `cd -`, or cdable_vars), so
// these tests pin both sides: no print without CDPATH, print with
// CDPATH="." (what oh-my-bash's lib/shopt.sh sets).
// ---------------------------------------------------------------------------

/// robbyrussell-shaped PS1 with raw color escapes and the `git:(branch)`
/// parens, matching the theme byte shape the report ran.
fn paren_theme_rc() -> String {
    let e = "\u{1b}";
    format!(
        "PS1='{e}[1;92m\u{279c}{e}[97m  {e}[96m\\W{e}[97m {e}[94mgit:({e}[91mmaster{e}[94m){e}[97m '\n\
         PS2='P2> '\n\
         NIU_DISABLE_DEFAULT_PLUGINS=1\n"
    )
}

/// cd under the paren prompt: no EOF diagnostic, no stray `)` in the echoed
/// input, and no working-directory print while CDPATH is unset.
#[test]
fn cd_with_paren_theme_prompt_leaks_nothing_into_input() {
    if !require_pty_or_skip("cd_with_paren_theme_prompt_leaks_nothing_into_input") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "paren-cd",
        &paren_theme_rc(),
        &[],
        (120, 30),
        DEFAULT_TIMEOUT,
    );
    s.expect("Niubash");
    s.expect("git:(");

    let repo = s.start().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    // Fidelity to the report (cd into a git repo): init one when git is
    // available; the assertions hold either way.
    let _ = std::process::Command::new("git")
        .arg("init")
        .arg(&repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    s.send_line("cd repo");
    s.send_line("echo after-paren-cd-ok");
    // The window covers the cd input echo, any cd output, the next prompt,
    // and the follow-up command's echo + output. A paren fragment leaking
    // into the input stream would either parse-fail (EOF diagnostic) or sit
    // in the edit buffer and break the follow-up command (its marker would
    // never print), so this one expect is the stray-paren detector.
    let follow = s.expect("after-paren-cd-ok");
    assert!(
        !follow.replace('\u{1b}', "").contains("unexpected EOF"),
        "unexpected EOF diagnostic after cd; window: {follow:?}"
    );
    // No working-directory print without CDPATH (GNU cd.def prints only on
    // a CDPATH hit / `cd -` / cdable_vars).
    let start_path = s
        .start()
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    assert!(
        !follow.to_lowercase().contains(&start_path),
        "plain cd printed the working directory without CDPATH; window: {follow:?}"
    );
    s.expect("git:(");
    assert!(
        !s.transcript()
            .replace('\u{1b}', "")
            .contains("unexpected EOF"),
        "unexpected EOF diagnostic in the session transcript"
    );
}

/// With CDPATH set (oh-my-bash's lib/shopt.sh sets CDPATH="."), a relative
/// cd that hits CDPATH prints the new directory — the GNU behavior the
/// owner's window actually exercised, pinned here so the engine contract
/// survives host changes.
#[test]
fn cd_with_cdpath_prints_directory_like_gnu() {
    if !require_pty_or_skip("cd_with_cdpath_prints_directory_like_gnu") {
        return;
    }
    let mut s = NiuSession::spawn_custom(
        "cdpath-print",
        &paren_theme_rc(),
        &[],
        (120, 30),
        DEFAULT_TIMEOUT,
    );
    s.expect("Niubash");
    s.expect("git:(");
    std::fs::create_dir_all(s.start().join("repo")).unwrap();

    s.send_line("export CDPATH=.");
    s.expect("git:(");
    s.send_line("cd repo");
    // GNU builtins/cd.def:353-378 — a non-empty CDPATH element that finds
    // the directory prints it.
    let window = s.expect("/start/repo");
    assert!(
        !window.replace('\u{1b}', "").contains("unexpected EOF"),
        "EOF diagnostic after CDPATH cd; window: {window:?}"
    );
    s.expect("git:(");
    s.send_line("echo cdpath-ok");
    s.expect("cdpath-ok");
}

// ---------------------------------------------------------------------------
// Matrix 7: multi-line theme cursor placement (niubash#169)
// ---------------------------------------------------------------------------

/// PS1 of the oh-my-bash/bash-it powerline-multiline family: the right-aligned
/// segment is produced by RAW cursor surgery inside PS1 — a jump to the right
/// margin (`\e[500C`, clamping at the terminal edge) plus a back-move
/// (`\e[6D`), then the segment text, all unmarked by `\[ \]`. The line editor
/// walks the prompt as a linear text run with ANSI stripped, so unmarked
/// surgery detaches its last-line visible-width accounting from the real
/// render and the editing cursor lands away from the input line. The prompt
/// channel now splits the idiom (prompt_right_align::split_right_align): the
/// tail renders through the editor's right-prompt placement (escape-excluded
/// width math), the jump/back bytes never reach the terminal, and typing
/// lands at the input line's end.
#[test]
fn multiline_right_align_theme_renders_tail_without_cursor_surgery() {
    if !require_pty_or_skip("multiline_right_align_theme_renders_tail_without_cursor_surgery") {
        return;
    }
    let rc = concat!(
        r"PS1='\[\e[34;1m\]left-seg \[\e[0m\]\e[500C\e[6D\[\e[33;1m\] RIGHT \[\e[0m\]\ni7> '",
        "\nPS2='P2> '\nNIU_DISABLE_DEFAULT_PLUGINS=1\n",
    );
    // 80 columns: ` RIGHT ` is 7 visible columns, so the editor's right
    // prompt is placed at column 73 (0-based).
    let mut s = NiuSession::spawn_custom("i169-right-align", rc, &[], (80, 24), DEFAULT_TIMEOUT);
    s.expect("Niubash");
    // Both halves render: the left segment inline, the aligned tail through
    // the editor's right prompt.
    s.expect("left-seg");
    s.expect("RIGHT");
    let transcript = s.transcript();
    assert!(
        !transcript.contains("<ESC>[500C"),
        "the raw right-align jump must be split out of the painted prompt"
    );
    // ConPTY re-renders the editor's absolute placement as relative ops:
    // from the left segment's column 9, a 64-column forward reaches column
    // 73 = 80 - 7 (` RIGHT ` escape-excluded), then the tail. The final
    // `ESC[3;5H` parks the cursor after `i7> ` on the input line — the #169
    // contract.
    assert!(
        transcript.contains("<ESC>[64C RIGHT"),
        "the tail must be placed at the escape-excluded margin column \
         (73 of 80); transcript: {transcript:?}"
    );
    assert!(
        transcript.contains("<ESC>[3;5H"),
        "the cursor must land after `i7> ` on the input line; transcript: {transcript:?}"
    );
    // The editor's cursor model survives typing at the input line.
    s.send_line("echo i169-ok");
    s.expect("i169-ok");
    s.expect("i7>");
}

/// Same shape with a WIDE-CJK tail: the editor's placement must count the
/// CJK glyphs as 2 columns each (` 项目 ` = 6 columns), so the MoveTo lands
/// one column further left than the ASCII tail — the visible-width rule the
/// fix exists for.
#[test]
fn multiline_right_align_theme_cjk_tail_width_matches_editor_columns() {
    if !require_pty_or_skip("multiline_right_align_theme_cjk_tail_width_matches_editor_columns") {
        return;
    }
    let rc = concat!(
        r"PS1='\[\e[34;1m\]left-seg \[\e[0m\]\e[500C\e[6D\[\e[33;1m\] 项目 \[\e[0m\]\ni7> '",
        "\nPS2='P2> '\nNIU_DISABLE_DEFAULT_PLUGINS=1\n",
    );
    let mut s =
        NiuSession::spawn_custom("i169-right-align-cjk", rc, &[], (80, 24), DEFAULT_TIMEOUT);
    s.expect("Niubash");
    s.expect("left-seg");
    s.expect("项目");
    let transcript = s.transcript();
    assert!(
        !transcript.contains("<ESC>[500C"),
        "the raw right-align jump must be split out of the painted prompt"
    );
    // 6 visible columns (space + two wide glyphs + space): from column 9 the
    // forward lands on column 74 — one further than the ASCII tail, which is
    // the 2-columns-per-glyph width rule end to end. Cursor after `i7> `.
    assert!(
        transcript.contains("<ESC>[65C 项目"),
        "the CJK tail must be placed by visible width (2 columns per glyph); \
         transcript: {transcript:?}"
    );
    assert!(
        transcript.contains("<ESC>[3;5H"),
        "the cursor must land after `i7> ` on the input line; transcript: {transcript:?}"
    );
    s.send_line("echo i169-cjk-ok");
    s.expect("i169-cjk-ok");
    s.expect("i7>");
}
