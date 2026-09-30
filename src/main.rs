//! niu entry point (the niubash shell)
//!
//! Usage:
//!   niu                  → interactive REPL
//!   niu -c "command"     → execute one command, print exit code, exit
//!   niu -C "command"     → execute one REPL-style command, then exit
//!   niu script.sh        → execute a script file
//!   niu --help | -h      → usage
//!   niu --version        → version (niubash / rubash / winuxcmd)
//!   niu setup            → re-run the interactive setup wizard
//!   niu plugin discover → read-only overview of external plugin sources
//!   niu plugin source <cmd> → manage external plugin-manager sources
//!   niu --completion-probe "line" [cursor] → print REPL completions
//!   niu --install-wt-profile → add/update the Windows Terminal profile
//!   niu --self-update → download and run the latest installer
//!   self-update / update-niubash → REPL commands for Niubash self-update

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use rubash::invocation::ShellInvocation;

/// #125/#140: std `println!`/`print!` panic on any stdout write error, and
/// this binary builds with `panic = "abort"`, so a reader closing the pipe
/// (`niu --version | head -1`; Windows reports os error 232 = ERROR_NO_DATA
/// rather than EPIPE) aborts the launcher mid-output. Shadow both macros
/// file-wide with writers that follow the engine's closed-pipe rule
/// (`is_closed_output_io_error`: BrokenPipe or raw os error 232) — write
/// what fits, then exit 0 quietly, matching the SIGPIPE termination GNU
/// exhibits when its stdout reader goes away. `eprint!`/`eprintln!` get the
/// same treatment minus the exit: a dead stderr must not abort either, but
/// the launcher's own exit status still belongs to the command that ran.
fn write_stdout_lossy(text: &str) {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => {}
        Err(error)
            if error.kind() == std::io::ErrorKind::BrokenPipe
                || error.raw_os_error() == Some(232) =>
        {
            std::process::exit(0)
        }
        Err(_) => {}
    }
}

fn write_stderr_lossy(text: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr
        .write_all(text.as_bytes())
        .and_then(|()| stderr.flush());
}

macro_rules! print {
    ($($arg:tt)*) => {
        crate::write_stdout_lossy(&format!($($arg)*))
    };
}
macro_rules! println {
    () => {
        crate::write_stdout_lossy("\n")
    };
    ($($arg:tt)*) => {
        crate::write_stdout_lossy(&format!("{}\n", format_args!($($arg)*)))
    };
}
macro_rules! eprint {
    ($($arg:tt)*) => {
        crate::write_stderr_lossy(&format!($($arg)*))
    };
}
macro_rules! eprintln {
    () => {
        crate::write_stderr_lossy("\n")
    };
    ($($arg:tt)*) => {
        crate::write_stderr_lossy(&format!("{}\n", format_args!($($arg)*)))
    };
}

mod self_update;
// GNU variables.c FUNCNEST: 0/unset means no limit, so recursion depth is
// bounded only by the real stack. Debug frames in the engine's call chain
// run ~150KB each; 512MiB (reserved, not committed) covers func4.sub's
// FUNCNEST=0 recursion to f=201 with headroom — mirrors rubash's main.rs.
const NIU_MAIN_STACK_SIZE: usize = 512 * 1024 * 1024;

fn main() -> ExitCode {
    // Restore the console (raw mode, cursor) on the panic path before the
    // default hook reports; with `panic = "abort"` this is the last code
    // that runs because no Drop guards execute.
    niubash_runtime::panic_restore::install_panic_hook();
    std::thread::Builder::new()
        .name("niu-main".to_string())
        .stack_size(NIU_MAIN_STACK_SIZE)
        .spawn(run_main)
        .expect("spawn niubash main thread")
        .join()
        .unwrap_or_else(|_| ExitCode::from(1))
}

fn run_main() -> ExitCode {
    // Initialize logging (only error level by default)
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Error)
        .parse_env("RUST_LOG")
        .init();

    // Install Ctrl+C handler (best-effort)
    niubash_runtime::ctrl_c::install();
    niubash_runtime::console_guard::prefer_utf8_code_page();
    niubash_runtime::console_guard::enable_vt_output();

    // Expose the host binary path so rubash's bash shim can forward to niu.
    // WINUXSH_SHELL is a deprecated bridge for current rubash upstream.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(path) = exe.to_str() {
            std::env::set_var("NIU_SHELL", path);
            std::env::set_var("WINUXSH_SHELL", path);
        }
    }

    let args: Vec<String> = std::env::args().collect();
    if let Some(name) = args
        .get(1)
        .and_then(|arg| arg.strip_prefix("--internal-"))
        .filter(|name| matches!(*name, "yes" | "head" | "wc"))
    {
        run_internal_pipeline_utility(name, &args[2..]);
    }

    if let Err(e) = run(&args) {
        if is_broken_pipe_error(&e) {
            return ExitCode::from(1);
        }
        eprintln!("niu: {}", e);
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run(args: &[String]) -> anyhow::Result<()> {
    if args.len() < 2 {
        return if niubash_runtime::terminal::stdio_is_interactive() {
            run_repl()
        } else {
            run_stdin_script()
        };
    }

    let first = &args[1];
    // GNU shell.c parse_shell_options: every argv word starting with '-' or
    // '+' is shell-option syntax (-c/-i/-o/-O/+B/+o ...), never a script
    // name. Route all of them through the engine's ShellInvocation parser —
    // a rejected option is a usage error, not "No such file or directory".
    if first.starts_with('-') || first.starts_with('+') {
        if !matches!(
            first.as_str(),
            "-h" | "--help"
                | "-V"
                | "--version"
                | "-C"
                | "--repl-command"
                | "--completion-probe"
                | "--install-wt-profile"
                | "--self-update"
        ) && !legacy_command_mode_has_post_c_login_flag(args)
        {
            // GNU shell.c parse_shell_options walks argv in one left-to-right
            // pass: option words may stand in any order before the first
            // non-option operand, and the shell does not re-dispatch on the
            // first word alone. A launcher-owned word therefore keeps its
            // meaning wherever it appears among the leading options
            // (niubash#148): `niu --norc -C 'cmd'` used to fall through to
            // the engine parser here, which has no REPL-command -C (GNU -C
            // is noclobber) and then treated the command string as a script
            // path. Words after the launcher flag's own argument keep GNU
            // operand semantics, so `niu -C 'cmd' --norc` still binds
            // --norc as $0 exactly like `bash -c 'cmd' --norc`.
            if let Some(index) = launcher_dispatch_index(args) {
                return dispatch_launcher_word(args, index);
            }
            // P3 invocation alignment: a leading-dash argument the engine parser
            // rejects is a usage error with the GNU surface (shell.c:874-881):
            // "<shell>: <option>: invalid option" + usage block, rc 2
            // (EX_BADUSAGE). The engine (rubash main.rs) reports under the
            // literal "bash" name so the upstream invocation suite normalizes
            // byte-for-byte; keep that convention.
            return match ShellInvocation::parse(&args[1..]) {
                Ok(_) => run_shell_invocation(&args[1..]),
                Err(message) => {
                    eprintln!("bash: {message}");
                    if message.contains("invalid option") {
                        show_shell_usage();
                    }
                    std::process::exit(2);
                }
            };
        }
        // The leading word is itself a launcher word (or the legacy
        // `niu -c -l cmd` shape kept above): dispatch on argv[1].
        return dispatch_launcher_word(args, 1);
    }
    match first.as_str() {
        "setup" | "configure" => match setup_preset_arg(&args[2..]) {
            Some(name) => niubash_runtime::setup_wizard::apply_preset(&name),
            None => niubash_runtime::setup_wizard::rerun_wizard(),
        },
        "font" => niubash_runtime::fonts::run_font_command(),
        "doctor" => niubash_runtime::doctor::run_doctor(),
        "plugin" => run_plugin_command(args),
        _ => {
            // Treat as a script file to execute
            let mut shell = niubash_runtime::Shell::new()?;
            // GNU shell.c:1572-1601 (open_shell_script): the script name is
            // tried as given; when that fails and the name has no path
            // separator it is searched in $PATH (findcmd.c find_path_file) —
            // that is how `bash ls` finds and then refuses the binary
            // /usr/bin/ls.
            let mut script = script_arg_to_host_path(first);
            if !script.exists() && !first.contains('/') && !first.contains('\\') {
                if let Some(found) = shell.executor.find_script_on_path(first) {
                    script = found;
                }
            }
            if !script.exists() {
                // shell.c shell_execve on a name that is neither option,
                // builtin, nor file: ENOENT surface, EX_NOTFOUND (127).
                eprintln!("niu: {}: No such file or directory", first);
                std::process::exit(127);
            }
            // general.c:718-741 check_binary_file: NUL in the first line(s)
            // or an ELF image is refused with EX_BINARY_FILE (126).
            let bytes = std::fs::read(&script)?;
            if rubash::script_driver::check_binary_file(&bytes)
                || std::str::from_utf8(&bytes).is_err()
            {
                eprintln!("cannot execute binary file");
                std::process::exit(126);
            }
            let content = String::from_utf8(bytes).unwrap_or_default();
            shell.set_script_name(first);
            shell.executor.inherit_process_stdin();
            shell.enable_process_stdin_pipeline_bridge();
            shell.source_non_interactive_env();
            shell.executor.set_positional_params(args[2..].to_vec());
            let code = shell.execute_script(&content)?;
            let code = shell.finish_with_exit_trap(code)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
    }
}

/// Index of the launcher-owned word among the leading option words of `args`
/// (argv[1]..), honoring GNU parse_shell_options' single left-to-right walk:
/// engine options that take a separate argument (`-o`, `-O`, `--rcfile`,
/// `--init-file`) consume the following word, `-c`/`-s` and the first
/// non-option word hand the rest to the engine route, and every other
/// `-`/`+` word is engine shell-option syntax. Returns `None` when the whole
/// leading option run belongs to the engine.
fn launcher_dispatch_index(args: &[String]) -> Option<usize> {
    const LAUNCHER_WORDS: &[&str] = &[
        "-h",
        "--help",
        "-V",
        "--version",
        "-C",
        "--repl-command",
        "--completion-probe",
        "--install-wt-profile",
        "--self-update",
    ];
    let mut index = 1usize;
    while let Some(arg) = args.get(index) {
        if LAUNCHER_WORDS.contains(&arg.as_str()) {
            return Some(index);
        }
        match arg.as_str() {
            // -c consumes the rest as the command string + operands, -s the
            // remaining words as positional parameters; both belong to the
            // engine route either way.
            "-c" | "-s" => return None,
            "-o" | "+o" | "-O" | "+O" | "--rcfile" | "--init-file" => index += 2,
            word if word.starts_with('-') || word.starts_with('+') => index += 1,
            // First non-option word is the script operand (engine route).
            _ => return None,
        }
    }
    None
}

/// Run the launcher word at `args[index]`. `args[1..index]` are the option
/// words that stood before it (applied by the handlers that understand
/// them); `args[index + 1..]` are the word's own arguments.
fn dispatch_launcher_word(args: &[String], index: usize) -> anyhow::Result<()> {
    let word = args[index].as_str();
    let leading_options = &args[1..index];
    let rest = &args[index + 1..];
    match word {
        "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        "--version" | "-V" => {
            print_version();
            Ok(())
        }
        "--completion-probe" => {
            print_completion_probe(rest)?;
            Ok(())
        }
        "--install-wt-profile" => {
            install_windows_terminal_profile(rest)?;
            Ok(())
        }
        "--self-update" => self_update::run(rest),
        "-C" | "--repl-command" => run_repl_command(word, leading_options, rest),
        // Only reachable for the legacy `niu -c -l <cmd>` shape: a plain
        // leading -c routes to the engine parser above.
        "-c" => {
            let command_mode = parse_legacy_command_mode(rest)?;
            let mut shell = niubash_runtime::Shell::new()?;
            niubash_runtime::startup_trace::tick("-c: Shell::new");
            shell.executor.inherit_process_stdin();
            shell.enable_process_stdin_pipeline_bridge();
            shell
                .executor
                .set_env("BASH_EXECUTION_STRING", command_mode.command);
            if let Some(command_name) = command_mode.command_name {
                shell.set_script_name(command_name);
                shell
                    .executor
                    .set_positional_params(command_mode.positional_params.to_vec());
            }
            let code = shell.execute_script(command_mode.command)?;
            niubash_runtime::startup_trace::tick("-c: execute_script");
            let code = shell.finish_with_exit_trap(code)?;
            niubash_runtime::startup_trace::tick("-c: exit trap");
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        other => anyhow::bail!("unknown launcher word '{other}'"),
    }
}

struct LegacyCommandMode<'a> {
    command: &'a str,
    command_name: Option<&'a str>,
    positional_params: &'a [String],
}

/// `niu -c [-l|--login] <command> [name [params...]]` — the legacy shape kept
/// for the `-c -l` combination. `rest` starts right after the `-c` word.
fn parse_legacy_command_mode(rest: &[String]) -> anyhow::Result<LegacyCommandMode<'_>> {
    let mut index = 0;
    while matches!(rest.get(index).map(String::as_str), Some("-l" | "--login")) {
        index += 1;
    }
    let Some(command) = rest.get(index) else {
        anyhow::bail!("-c requires an argument");
    };
    let command_name = rest.get(index + 1).map(String::as_str);
    let positional_params = rest.get(index + 2..).unwrap_or(&[]);
    Ok(LegacyCommandMode {
        command,
        command_name,
        positional_params,
    })
}

fn legacy_command_mode_has_post_c_login_flag(args: &[String]) -> bool {
    matches!(args.get(1).map(String::as_str), Some("-c"))
        && matches!(args.get(2).map(String::as_str), Some("-l" | "--login"))
}

fn run_shell_invocation(args: &[String]) -> anyhow::Result<()> {
    // Read before Shell::new overwrites the process variable (shell name
    // setup writes BASH_ARGV0 back into the environment).
    let inherited_argv0 = std::env::var("BASH_ARGV0").ok().filter(|v| !v.is_empty());
    let invocation =
        ShellInvocation::parse(args).map_err(|error| anyhow::anyhow!("niu: {}", error))?;

    if invocation.dump_strings {
        let input = invocation_input(&invocation)?;
        let source_name = invocation_source_name(&invocation);
        print_locale_strings(&input, invocation.dump_po, &source_name);
        return Ok(());
    }
    if invocation.pretty_print {
        let input = invocation_input(&invocation)?;
        pretty_print_script(&input);
        return Ok(());
    }

    let mut shell = if invocation.read_stdin {
        niubash_runtime::Shell::new_for_stdin_script()?
    } else {
        niubash_runtime::Shell::new()?
    };
    niubash_runtime::startup_trace::tick("invocation: Shell::new");
    shell.no_rc = invocation.no_rc;
    shell.no_profile = invocation.no_profile;
    shell.rc_file = invocation.rc_file.clone().map(PathBuf::from);
    shell.no_editing = invocation.no_editing;
    invocation
        .apply_to_executor(&mut shell.executor)
        .map_err(|error| {
            // shell.c reports a bad -o/-O option name through the line-0
            // diagnostic ("bash: line 0: badopt: invalid shell option name").
            if error.contains("invalid shell option name") {
                eprintln!("bash: line 0: {error}");
                std::process::exit(2);
            }
            anyhow::anyhow!("niu: {error}")
        })?;
    shell.executor.inherit_process_stdin();
    shell.enable_process_stdin_pipeline_bridge();

    if let Some(command) = invocation.command {
        // GNU shell.c: -i sets forced_interactive during option parsing, so
        // `bash -i -c 'cmd'` takes run_startup_files' interactive branch
        // (shell.c:1222: rc file) and never reads BASH_ENV; plain `-c` runs
        // the shell.c:1214-1220 non-interactive BASH_ENV branch (the
        // shell.c:1156 sshd bashrc case is compiled out of the reference
        // build — see source_non_interactive_env).
        if invocation.interactive {
            shell.run_interactive_startup_rc();
        } else {
            shell.source_non_interactive_env();
        }
        niubash_runtime::startup_trace::tick("invocation: setup done");
        // GNU shell.c: $0 for -c is the word after the command string, or
        // $BASH_ARGV0 from the environment when exported by the caller.
        if let Some(argv0) = inherited_argv0.clone() {
            shell.set_script_name(&argv0);
        } else if let Some(name) = invocation.command_name.clone() {
            shell.set_script_name(&name);
        }
        shell.executor.set_env("BASH_EXECUTION_STRING", &command);
        let code = shell.execute_script(&command)?;
        niubash_runtime::startup_trace::tick("invocation: execute_script");
        let code = shell.finish_with_exit_trap(code)?;
        niubash_runtime::startup_trace::tick("invocation: exit trap");
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }
    if let Some(script_name) = invocation.script {
        // GNU: `bash -i script` is an interactive shell (forced_interactive)
        // and sources the rc file, not BASH_ENV (shell.c:1214 checks
        // interactive_shell == 0).
        if invocation.interactive {
            shell.run_interactive_startup_rc();
        } else {
            shell.source_non_interactive_env();
        }
        shell.set_script_name(&script_name);
        let content = std::fs::read_to_string(script_arg_to_host_path(&script_name))?;
        let code = shell.execute_script(&content)?;
        let code = shell.finish_with_exit_trap(code)?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }
    // GNU bash -i with a non-tty stdin still drives readline
    // (parse.y yy_readline_get -> bashline.c bash_readline): prompts and the
    // input echo go to stderr, editing keys are honored, and every command
    // is recorded to engine history. The reedline product REPL cannot run
    // without a terminal, so delegate to the engine's interactive stdin
    // driver instead of enter_interactive()/run_repl.
    if invocation.interactive && !niubash_runtime::terminal::stdio_is_interactive() {
        shell.executor.set_env("__RUBASH_INTERACTIVE", "1");
        shell.executor.set_shopt_option("expand_aliases", true);
        // GNU decides "interactive" from the -i flag, never from the shape
        // of stdin (shell.c:672 forced_interactive is set in option parsing,
        // before run_startup_files at shell.c:722 sources ~/.bashrc for an
        // interactive shell). The interactive startup rc — and --rcfile,
        // which the shell already carries — must therefore run on the piped
        // -i path too (niubash#146), still before the interactive history
        // setup, which shell.c:806-811 runs only after the startup files.
        shell.run_startup_rc();
        rubash::script_driver::prepare_interactive_history(&mut shell.executor);
        let code = rubash::script_driver::run_interactive_stdin(&mut shell.executor);
        std::process::exit(code);
    }
    // Bash -i forces an interactive shell even when stdin is not a terminal;
    // with no command or script, a terminal (or -i) means the REPL.
    if invocation.interactive || niubash_runtime::terminal::stdio_is_interactive() {
        shell.enter_interactive();
        return niubash_runtime::repl::run_repl(shell);
    }
    shell.source_non_interactive_env();
    let mut content = String::new();
    std::io::stdin().read_to_string(&mut content)?;
    let code = shell.execute_script(&content)?;
    let code = shell.finish_with_exit_trap(code)?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

fn invocation_input(invocation: &ShellInvocation) -> anyhow::Result<String> {
    if let Some(command) = &invocation.command {
        return Ok(command.clone());
    }
    if let Some(script_name) = &invocation.script {
        let path = script_arg_to_host_path(script_name);
        return Ok(std::fs::read_to_string(&path)?);
    }
    let mut content = String::new();
    std::io::stdin().read_to_string(&mut content)?;
    Ok(content)
}

/// -D / --dump-strings: list every locale string ($"...") without executing,
/// the way GNU bash's dump-strings option does. --dump-po-strings selects the
/// GNU gettext PO output format.
///
/// GNU recognizes a locale string only where the word lexer reads `$` followed
/// by `"` while scanning a word (parse.y read_token_word's
/// `character == '$' && peek_char == '"'` branch; the dump itself is
/// locale.c locale_expand: printf("\"%s\"\n")). Comment text and here-doc
/// bodies are never lexed as words, so they never dump; single-quoted text,
/// double-quoted spans and backtick bodies are skipped as units; word-
/// embedded, quoted and arithmetic-embedded command substitutions are
/// re-lexed, so locale strings inside them do dump (parse.y:4100 processes
/// `$(` units encountered inside a matched pair).
///
/// This pass therefore walks the rubash token stream -- which already
/// excludes the comment and here-doc-body classes structurally, since the
/// lexer never yields word-shaped tokens from them -- and applies the
/// word-level quote rules to the raw spelling of word-shaped tokens. The
/// old implementation byte-scanned the raw script instead and misfired in
/// exactly those positions.
fn print_locale_strings(input: &str, po: bool, source_name: &str) {
    let mut strings = Vec::new();
    collect_locale_strings(input, 1, &mut strings);
    print!("{}", render_locale_string_dump(&strings, po, source_name));
}

/// The `#: name:lineno` anchor GNU bash prints in --dump-po-strings entries
/// (locale.c locale_expand passes yy_input_name()): the script path as given
/// on argv, the literal `-c` for -c input, and the shell's own argv[0] for
/// standard input.
fn invocation_source_name(invocation: &ShellInvocation) -> String {
    if invocation.command.is_some() {
        return "-c".to_string();
    }
    if let Some(script) = &invocation.script {
        return script.clone();
    }
    std::env::args().next().unwrap_or_else(|| "niu".to_string())
}

/// Collects `(line, raw body)` for every locale string in `input`, in source
/// order. `base_line` is the line the token stream's own numbering starts
/// from: top-level tokens carry real script lines in `token.position`, while
/// a re-lexed substitution body restarts at 1, so nested strings are mapped
/// back with `base_line + position - 1`. GNU reports the physical line of
/// each nested string; the two agree whenever the substitution body starts
/// on its token's start line (the overwhelmingly common single-line word).
fn collect_locale_strings(input: &str, base_line: usize, out: &mut Vec<(usize, String)>) {
    for token in rubash::lexer::tokenize(input) {
        let line = base_line + token.position.saturating_sub(1);
        match token.kind {
            rubash::TokenKind::Word
            | rubash::TokenKind::Assignment
            | rubash::TokenKind::BraceExpand => scan_locale_words(&token.raw, line, out),
            rubash::TokenKind::CommandSubst => match substitution_span(&token.raw) {
                SubstitutionSpan::Command(body) => collect_locale_strings(&body, line, out),
                SubstitutionSpan::Arithmetic(body) => scan_arithmetic_text(&body, line, out),
                SubstitutionSpan::None => {}
            },
            _ => {}
        }
    }
}

/// Word-level scan of one token's raw spelling, mirroring where GNU's word
/// lexer recognizes `$"`: outside single quotes, double-quoted spans,
/// backtick bodies and `${...}`/`$'...'` units. `\"` at word level escapes
/// the next character, so `\$"x"` is not a locale string introducer.
fn scan_locale_words(raw: &str, line: usize, out: &mut Vec<(usize, String)>) {
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = skip_single_quoted(bytes, i + 1),
            b'`' => i = skip_backquoted(bytes, i + 1),
            b'"' => i = scan_double_quoted(raw, i + 1, line, out),
            b'$' => match bytes.get(i + 1) {
                Some(b'"') => {
                    let close = locale_body_end(raw, i + 2, line, out);
                    out.push((line, raw[i + 2..close].to_string()));
                    i = close + 1;
                }
                Some(b'\'') => i = skip_ansi_c_quoted(bytes, i + 2),
                Some(b'{') => i = skip_dollar_brace(bytes, i + 2),
                Some(b'(') => i = scan_substitution_unit(raw, i + 1, line, out),
                _ => i += 1,
            },
            b'\\' => i = (i + 2).min(bytes.len()),
            _ => i += 1,
        }
    }
}

/// Byte index of the closing `"` of a locale string body whose text starts at
/// `start` (just past the opening quote). Nested `${...}`/`` `...` ``/`$(...)`
/// units are skipped the way GNU parse_matched_pair skips them while it
/// extracts the pair, and command-substitution units are re-lexed so their
/// own locale strings dump first (GNU order: inner before outer). The
/// surrounding body is reported verbatim: GNU additionally rewrites nested
/// `$"..."` units to `"..."` inside the body it dumps, which needs a
/// byte-exact body serializer rubash does not expose (host-semantic-layer
/// plan, C1 residual).
fn locale_body_end(raw: &str, start: usize, line: usize, out: &mut Vec<(usize, String)>) -> usize {
    let bytes = raw.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return i,
            b'\\' => i = (i + 2).min(bytes.len()),
            b'`' => i = skip_backquoted(bytes, i + 1),
            b'$' => match bytes.get(i + 1) {
                Some(b'{') => i = skip_dollar_brace(bytes, i + 2),
                Some(b'(') => i = scan_substitution_unit(raw, i + 1, line, out),
                _ => i += 1,
            },
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Walks a double-quoted span. `$` followed by `"` is literal data here (GNU
/// dumps nothing for `echo "$"dqp" tail"`), while `$(...)` units are re-lexed
/// (parse.y:4100) and their locale strings dump.
fn scan_double_quoted(
    raw: &str,
    start: usize,
    line: usize,
    out: &mut Vec<(usize, String)>,
) -> usize {
    let bytes = raw.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return i + 1,
            b'\\' => i = (i + 2).min(bytes.len()),
            b'`' => i = skip_backquoted(bytes, i + 1),
            b'$' => match bytes.get(i + 1) {
                Some(b'{') => i = skip_dollar_brace(bytes, i + 2),
                Some(b'(') => i = scan_substitution_unit(raw, i + 1, line, out),
                _ => i += 1,
            },
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Extracts the span of a `$(...)` / `$((...))` unit whose `(` sits at `open`
/// and dispatches it: command-substitution bodies are re-lexed (GNU dumps
/// their locale strings, unquoted, word-embedded or double-quoted alike) and
/// arithmetic bodies keep dumping only through the quoted command
/// substitutions they contain.
fn scan_substitution_unit(
    raw: &str,
    open: usize,
    line: usize,
    out: &mut Vec<(usize, String)>,
) -> usize {
    let bytes = raw.as_bytes();
    let Some(close) = paren_close(bytes, open) else {
        return bytes.len();
    };
    if bytes.get(open + 1) == Some(&b'(') {
        scan_arithmetic_text(&raw[open + 2..close - 1], line, out);
    } else {
        collect_locale_strings(&raw[open + 1..close], line, out);
    }
    close + 1
}

/// Arithmetic text (`$(( ... ))` inner span): `$"..."` never fires here, but
/// quoted command substitutions are parsed by GNU and their locale strings
/// dump (GNU 5.3.0: `$(( "$(echo $"x")" + 1 ))` dumps `"x"`).
fn scan_arithmetic_text(text: &str, line: usize, out: &mut Vec<(usize, String)>) {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => i = scan_double_quoted(text, i + 1, line, out),
            b'`' => i = skip_backquoted(bytes, i + 1),
            b'$' if bytes.get(i + 1) == Some(&b'(') => {
                if bytes.get(i + 2) == Some(&b'(') {
                    // Nested arithmetic span: no locale recognition inside.
                    i += 3;
                } else {
                    i = scan_substitution_unit(text, i + 1, line, out);
                }
            }
            b'\\' => i = (i + 2).min(bytes.len()),
            _ => i += 1,
        }
    }
}

enum SubstitutionSpan {
    Command(String),
    Arithmetic(String),
    None,
}

/// Classifies a CommandSubst token's raw spelling: a `` `...` `` token shares
/// the kind but never starts with `$(`, and its body must not be re-lexed
/// (GNU keeps backtick bodies verbatim at parse time, so `echo `echo $"x"``
/// dumps nothing). `$((...))` yields its arithmetic inner span.
fn substitution_span(raw: &str) -> SubstitutionSpan {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'$') || bytes.get(1) != Some(&b'(') {
        return SubstitutionSpan::None;
    }
    let Some(close) = paren_close(bytes, 1) else {
        return SubstitutionSpan::None;
    };
    if bytes.get(2) == Some(&b'(') {
        SubstitutionSpan::Arithmetic(raw[3..close - 1].to_string())
    } else {
        SubstitutionSpan::Command(raw[2..close].to_string())
    }
}

/// Byte index of the `)` matching the `(` at `open`, honoring quoting the way
/// GNU parse_matched_pair does while it extracts a substitution span. Returns
/// None when the span never closes; callers then treat the rest of the text
/// as the unit, which keeps the scan total on malformed input.
fn paren_close(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(i - 1);
                }
            }
            b'\'' => i = skip_single_quoted(bytes, i + 1),
            b'"' => i = skip_double_span(bytes, i + 1),
            b'`' => i = skip_backquoted(bytes, i + 1),
            b'\\' => i = (i + 2).min(bytes.len()),
            _ => i += 1,
        }
    }
    None
}

fn skip_single_quoted(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_ansi_c_quoted(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => return i + 1,
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_backquoted(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'`' => return i + 1,
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_double_span(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return i + 1,
            b'\\' => i = (i + 2).min(bytes.len()),
            b'`' => i = skip_backquoted(bytes, i + 1),
            b'$' if bytes.get(i + 1) == Some(&b'{') => i = skip_dollar_brace(bytes, i + 2),
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_dollar_brace(bytes: &[u8], mut i: usize) -> usize {
    let mut depth = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return i;
                }
            }
            b'\'' => i = skip_single_quoted(bytes, i + 1),
            b'"' => i = skip_double_span(bytes, i + 1),
            b'`' => i = skip_backquoted(bytes, i + 1),
            b'\\' => i = (i + 2).min(bytes.len()),
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Renders collected locale strings exactly the way GNU bash prints them
/// (locale.c locale_expand): plain mode is `"body"` with the raw body text
/// verbatim (escapes stay escaped, embedded newlines split the output line),
/// and PO mode is the mk_msgstr form anchored by `#: name:lineno`.
fn render_locale_string_dump(strings: &[(usize, String)], po: bool, source_name: &str) -> String {
    let mut out = String::new();
    for (line, body) in strings {
        if po {
            let mut escaped = String::new();
            let mut multiline = false;
            for ch in body.chars() {
                match ch {
                    '\n' => {
                        escaped.push_str("\\n\"\n\"");
                        multiline = true;
                    }
                    '"' | '\\' => {
                        escaped.push('\\');
                        escaped.push(ch);
                    }
                    _ => escaped.push(ch),
                }
            }
            if multiline {
                out.push_str(&format!(
                    "#: {source_name}:{line}\nmsgid \"\"\n\"{escaped}\"\nmsgstr \"\"\n"
                ));
            } else {
                out.push_str(&format!(
                    "#: {source_name}:{line}\nmsgid \"{escaped}\"\nmsgstr \"\"\n"
                ));
            }
        } else {
            out.push('"');
            out.push_str(body);
            out.push_str("\"\n");
        }
    }
    out
}

/// --pretty-print: GNU pretty_print_loop (eval.c:215-253) reads one command
/// at a time: a blank input line ends the current command, an empty parse
/// prints one newline (suppressed right after another newline), and each
/// parsed command prints as its canonical text plus one newline. Mirrors the
/// engine's rubash main.rs implementation over the public parser API.
fn pretty_print_script(input: &str) {
    let posix = std::env::var("__RUBASH_POSIX_MODE").as_deref() == Ok("1");
    let mut output = String::new();
    let mut pending = String::new();
    let mut last_was_newline = false;
    for line in input.lines() {
        if line.trim().is_empty() && !rubash::lexer::has_unclosed_input_syntax(&pending) {
            last_was_newline =
                flush_pretty_print_chunk(&pending, posix, &mut output, last_was_newline);
            pending.clear();
            if !last_was_newline {
                output.push('\n');
                last_was_newline = true;
            }
            continue;
        }
        if !pending.is_empty() {
            pending.push('\n');
        }
        pending.push_str(line);
    }
    last_was_newline = flush_pretty_print_chunk(&pending, posix, &mut output, last_was_newline);
    if !last_was_newline && !output.is_empty() {
        output.push('\n');
    }
    print!("{output}");
}

fn flush_pretty_print_chunk(
    chunk: &str,
    posix: bool,
    output: &mut String,
    last_was_newline: bool,
) -> bool {
    let tokens = rubash::lexer::tokenize_with_initial_posix(chunk, posix);
    let ast = rubash::parser::parse(&tokens);
    let mut printed = false;
    for command in &ast.commands {
        if is_pretty_print_empty(command) {
            continue;
        }
        output.push_str(&rubash::parser::ast_print::pretty_print_command(command));
        output.push('\n');
        printed = true;
    }
    if printed {
        return false;
    }
    last_was_newline
}

fn is_pretty_print_empty(command: &rubash::parser::CommandNode) -> bool {
    command.words.is_empty()
        && command.assignments.is_empty()
        && command.compound_assignments.is_empty()
        && command.array_element_assignments.is_empty()
        && command.for_command.is_none()
        && command.select_command.is_none()
        && command.loop_command.is_none()
        && command.if_command.is_none()
        && command.case_command.is_none()
        && command.function_command.is_none()
        && command.arithmetic_command.is_none()
        && command.conditional_command.is_none()
        && command.coproc_command.is_none()
        && command.brace_group.is_none()
        && command.pipeline_command.is_none()
        && command.and_or_list.is_none()
}

fn script_arg_to_host_path(value: &str) -> PathBuf {
    if cfg!(windows) {
        let normalized = value.replace('\\', "/");
        let bytes = normalized.as_bytes();
        if bytes.len() >= 2
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && (bytes.len() == 2 || bytes.get(2) == Some(&b'/'))
        {
            let drive = (bytes[1] as char).to_ascii_uppercase();
            let rest = if normalized.len() == 2 {
                "/"
            } else {
                &normalized[2..]
            };
            return PathBuf::from(format!("{drive}:{rest}"));
        }
    }

    PathBuf::from(value)
}

fn run_repl() -> anyhow::Result<()> {
    self_update::maybe_print_update_hint();
    let mut shell = niubash_runtime::Shell::new()?;
    shell.enter_interactive();
    niubash_runtime::repl::run_repl(shell)
}

/// `niu [-C|--repl-command] <command> [name [params...]]`: execute one
/// REPL-style command and exit. `leading_options` are the shell option words
/// that stood before the -C word (niubash#148); they are parsed with the
/// engine's `ShellInvocation` — the same surface the engine route uses — so
/// `niu --norc -C 'cmd'` and `niu -C 'cmd'` see one consistent option model,
/// with rc-affecting fields applied before the startup rc runs.
fn run_repl_command(flag: &str, leading_options: &[String], rest: &[String]) -> anyhow::Result<()> {
    let Some(command) = rest.first() else {
        anyhow::bail!("{flag} requires an argument");
    };
    if let Some(self_update_args) = niubash_runtime::repl::self_update_command_args(command) {
        if let Some(code) = niubash_runtime::repl::spawn_self_update(&self_update_args) {
            std::process::exit(code);
        }
    }
    let mut shell = niubash_runtime::Shell::new()?;
    niubash_runtime::startup_trace::tick("-C: Shell::new");
    if !leading_options.is_empty() {
        // Same option words, same GNU error surface as the engine route
        // below (shell.c:874-881): a rejected option is a usage error under
        // the engine's "bash" name, rc 2, with the usage block when the
        // word is an invalid option.
        let invocation = match ShellInvocation::parse(leading_options) {
            Ok(invocation) => invocation,
            Err(message) => {
                eprintln!("bash: {message}");
                if message.contains("invalid option") {
                    show_shell_usage();
                }
                std::process::exit(2);
            }
        };
        shell.no_rc = invocation.no_rc;
        shell.no_profile = invocation.no_profile;
        shell.rc_file = invocation.rc_file.clone().map(PathBuf::from);
        shell.no_editing = invocation.no_editing;
        invocation
            .apply_to_executor(&mut shell.executor)
            .map_err(|error| {
                if error.contains("invalid shell option name") {
                    eprintln!("bash: line 0: {error}");
                    std::process::exit(2);
                }
                anyhow::anyhow!("{error}")
            })?;
    }
    shell.enter_interactive();
    shell.executor.inherit_process_stdin();
    shell.enable_process_stdin_pipeline_bridge();
    if let Some(command_name) = rest.get(1) {
        shell.set_script_name(command_name);
        shell.executor.set_positional_params(rest[2..].to_vec());
    }
    shell.run_startup_rc();
    niubash_runtime::startup_trace::tick("-C: startup rc");
    shell.run_precmd_hooks();
    niubash_runtime::startup_trace::tick("-C: precmd hooks");
    let code = shell.execute_interactive_line(command)?;
    niubash_runtime::startup_trace::tick("-C: execute_interactive_line");
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

fn run_stdin_script() -> anyhow::Result<()> {
    let mut shell = niubash_runtime::Shell::new_for_stdin_script()?;
    shell.executor.inherit_process_stdin();
    shell.source_non_interactive_env();
    let mut line = String::new();
    let mut pending = Vec::new();

    loop {
        line.clear();
        // GNU input.c bash_input binds the script reader to fd 0: a
        // permanent `exec 0<file` (redir.c do_redirections) moves the
        // script source to the new input. read_unbuffered_line on the raw
        // fd also avoids StdinLock prefetch stealing bytes from `&`
        // children that inherit fd 0 (redir1.sub, redir.tests heredocs).
        match shell
            .executor
            .script_fd0_line(&mut line)
            .map(Ok)
            .unwrap_or_else(|| read_unbuffered_line(&mut line))?
        {
            0 => {
                if !pending.is_empty() {
                    let code = shell.execute_script(&pending.join("\n"))?;
                    let code = shell.finish_with_exit_trap(code)?;
                    if code != 0 {
                        std::process::exit(code);
                    }
                }
                break;
            }
            _ => {}
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if pending.is_empty() && line.trim().is_empty() {
            continue;
        }
        pending.push(line.to_string());
        let script = pending.join("\n");
        if !niubash_runtime::repl::is_script_input_complete(&script) {
            continue;
        }

        let code = match shell.stdin_current_shell_child(&script) {
            Some(child) => {
                let mut child_stdin = String::new();
                let _ = read_unbuffered_line(&mut child_stdin)?;
                shell.execute_stdin_current_shell_child(child, &child_stdin)?
            }
            None => shell.execute_script(&script)?,
        };
        if code != 0 {
            let code = shell.finish_with_exit_trap(code)?;
            std::process::exit(code);
        }
        pending.clear();
    }

    let code = shell.finish_with_exit_trap(0)?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

fn read_unbuffered_line(output: &mut String) -> std::io::Result<usize> {
    let mut stdin = std::io::stdin().lock();
    let mut bytes = [0_u8; 1];
    let mut read = 0;

    loop {
        match stdin.read(&mut bytes)? {
            0 => break,
            count => {
                read += count;
                output.push(bytes[0] as char);
                if bytes[0] == b'\n' {
                    break;
                }
            }
        }
    }

    Ok(read)
}

fn run_internal_pipeline_utility(name: &str, args: &[String]) -> ! {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    match name {
        "yes" => {
            let line = if args.is_empty() {
                "y".to_string()
            } else {
                args.join(" ")
            };
            let chunk = format!("{line}\n").repeat(256);
            loop {
                if stdout.write_all(chunk.as_bytes()).is_err() || stdout.flush().is_err() {
                    std::process::exit(0);
                }
            }
        }
        "head" => {
            let count = internal_head_line_count(args).unwrap_or(10);
            let mut input = std::io::BufReader::new(stdin.lock());
            let mut line = Vec::new();
            for _ in 0..count {
                line.clear();
                match input.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if stdout.write_all(&line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = stdout.flush();
            std::process::exit(0);
        }
        "wc" => {
            let mut input = stdin.lock();
            let mut buffer = [0_u8; 8192];
            let mut lines = 0usize;
            loop {
                match input.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => {
                        lines += buffer[..size].iter().filter(|byte| **byte == b'\n').count()
                    }
                    Err(_) => break,
                }
            }
            let _ = writeln!(stdout, "{lines}");
            std::process::exit(0);
        }
        _ => std::process::exit(127),
    }
}

fn internal_head_line_count(args: &[String]) -> Option<usize> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "-n" {
            return args.get(index + 1)?.parse().ok();
        }
        if let Some(value) = arg.strip_prefix("-n") {
            if !value.is_empty() {
                return value.parse().ok();
            }
        }
        if let Some(value) = arg.strip_prefix('-') {
            if !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()) {
                return value.parse().ok();
            }
        }
        if let Some(value) = arg.strip_prefix("--lines=") {
            return value.parse().ok();
        }
        index += 1;
    }
    None
}

/// GNU shell.c show_shell_usage (shell.c:2056-2103) with extra=0: the usage
/// block the upstream invocation suite expects after an invalid option. The
/// "bash" spelling is the engine convention (rubash main.rs) so the suite's
/// `sed 's|^.*/bash|bash|'` normalization matches byte for byte.
fn show_shell_usage() {
    eprint!(
        "bash [GNU long option] [option] ...
bash [GNU long option] [option] script-file ...
"
    );
    eprintln!("GNU long options:");
    for name in LONG_OPTIONS {
        eprintln!("	--{name}");
    }
    eprintln!("Shell options:");
    eprintln!("	-ilrsD or -c command or -O shopt_option		(invocation only)");
    eprintln!("	-abefhkmnptuvxBCEHPT or -o option");
}

const LONG_OPTIONS: &[&str] = &[
    "debug",
    "debugger",
    "dump-po-strings",
    "dump-strings",
    "help",
    "init-file",
    "login",
    "noediting",
    "noprofile",
    "norc",
    "posix",
    "pretty-print",
    "rcfile",
    "restricted",
    "verbose",
    "version",
];

fn print_usage() {
    println!(
        "Niubash {} \u{2014} a bash-compatible shell that feels at home on Windows.",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("Usage:  niu [option]");
    println!("        niu -c <cmd>         Run a command then exit");
    println!("        niu -C <cmd>         Run one REPL-style command then exit");
    println!("        niu setup           Re-run prompt/plugin setup");
    println!("        niu font            Install a Nerd Font for icon-rich themes");
    println!("        niu doctor          Health-check the installation");
    println!("        niu <script> [args]  Run a script file");
    println!();
    println!("Options:");
    println!("  -h, --help                Show this help");
    println!("  -V, --version             Version and component info");
    println!("  -c <command>              Execute a command ad-hoc");
    println!("  -C, --repl-command <cmd>  Execute one non-interactive REPL command");
    println!();
    println!("  --install-wt-profile      Add/update the Windows Terminal profile");
    println!("      --set-default         Also set Niubash as the WT default profile");
    println!("      --quiet               Suppress non-error profile output");
    println!("  --self-update             Download and run the latest release installer");
    println!("      --check               Only report the latest release");
    println!("      --dry-run             Download installer without running it");
    println!("  self-update               REPL command: update Niubash and exit this shell");
    println!("  update-niubash            Alias for self-update");
    println!();
    println!("  plugin discover [--verbose]");
    println!("                            Read-only overview of external plugin sources");
    println!("  plugin source list [--json]");
    println!("                            List external plugin-manager sources");
    println!("  plugin source add <id|url|path> [--ref <ref>] [--checksum <sha256>]");
    println!("                            Install a plugin-manager source (untrusted)");
    println!("  plugin source trust <id>  Review and activate a source's assets");
    println!();
    println!("  --completion-probe <line> [cursor]  Debug: print completion candidates");
    println!();
    println!("Configuration: ~/.niubashrc for interactive startup; a pre-rename ~/.winuxshrc is migrated once into ~/.niubashrc");
    println!();
    println!("Environment:");
    println!(
        "  NIU_ENV=<file>          Non-interactive init file sourced by -c, scripts, and stdin"
    );
    println!(
        "                          before running the command (bash BASH_ENV is also honored,"
    );
    println!(
        "                          NIU_ENV takes precedence). Unset by default, keeping -c fast."
    );
    println!("  BASH_ENV=<file>         GNU bash compatible: same as NIU_ENV, lower precedence.");
    println!("  NIU_LANG=<lang>         Setup wizard language (zh / en). Falls back to the");
    println!("                          Windows UI language, then LC_ALL/LANG.");
}

fn run_plugin_command(args: &[String]) -> anyhow::Result<()> {
    let Some(subcommand) = args.get(2) else {
        print_plugin_usage();
        return Ok(());
    };

    match subcommand.as_str() {
        "-h" | "--help" => {
            print_plugin_usage();
            Ok(())
        }
        "discover" => run_plugin_discover_command(&args[3..]),
        "source" | "sources" => run_plugin_source_command(&args[3..]),
        // The built-in pack/bundle subcommands retired with the plugin
        // stack (niubash#145); external plugin-manager sources remain.
        "list" | "info" | "search" | "themes" | "bundle" | "doctor" | "review" | "update"
        | "rollback" | "add" | "trust" | "use" | "remove" | "enable" | "disable" => {
            anyhow::bail!(
                "plugin '{}' retired with the built-in plugin/theme stack (niubash#145); \
                 see `niu plugin source --help` for the external ecosystem",
                subcommand
            )
        }
        unknown => anyhow::bail!("unknown plugin subcommand '{}'", unknown),
    }
}

/// `niu plugin discover`: a dry, read-only overview of the external plugin
/// ecosystem. Shows installed sources (with their ready/untrusted/degraded
/// state) and the known plugin managers that are *not* installed yet —
/// without installing, trusting, sourcing, or writing anything. Every
/// install stays an explicit command the user runs.
fn run_plugin_discover_command(args: &[String]) -> anyhow::Result<()> {
    for arg in args {
        match arg.as_str() {
            "--verbose" => {}
            unknown => anyhow::bail!("unknown plugin option '{}'", unknown),
        }
    }
    println!(
        "{}",
        niubash_runtime::text_style::bold("Niubash plugin ecosystem")
    );
    println!(
        "{}",
        niubash_runtime::text_style::dim(
            "  read-only overview — nothing is installed, sourced, or changed"
        )
    );
    println!();

    println!(
        "{}",
        niubash_runtime::text_style::cyan("Plugin sources (external plugin managers)")
    );
    let statuses = niubash_runtime::plugins::sources::list_sources();
    if statuses.is_empty() {
        println!("  (none installed)");
    }
    for status in &statuses {
        let marker = match status.state.as_str() {
            "ready" => niubash_runtime::text_style::green("ready"),
            "untrusted" => niubash_runtime::text_style::yellow("untrusted"),
            _ => niubash_runtime::text_style::red("degraded"),
        };
        let assets = status
            .asset_count
            .map(|count| format!(" ({count} assets)"))
            .unwrap_or_default();
        println!(
            "  {} {:<12} {:<12} {}{}",
            marker, status.record.id, status.record.version, status.record.license, assets
        );
    }
    println!();

    println!("{}", niubash_runtime::text_style::cyan("Available sources"));
    let mut listed = 0usize;
    for adapter in niubash_runtime::plugins::sources::builtin_source_adapters() {
        if statuses
            .iter()
            .any(|status| status.record.id == adapter.id())
        {
            continue;
        }
        let add_hint = match adapter.default_origin() {
            Some(origin) => format!("niu plugin source add {} --url {}", adapter.id(), origin),
            None => format!("niu plugin source add {} --path <dir>", adapter.id()),
        };
        println!(
            "  {:<12} {:<9} {}",
            adapter.display_name(),
            adapter.license(),
            niubash_runtime::text_style::dim(&add_hint)
        );
        listed += 1;
    }
    if listed == 0 {
        println!(
            "  {}",
            niubash_runtime::text_style::dim("(every known manager is already installed)")
        );
    }
    println!();
    println!(
        "{}",
        niubash_runtime::text_style::dim(
            "Themes render through the bash-compatible PS1 channel; the built-in \
             plugin/theme stack is retired (niubash#145)."
        )
    );
    Ok(())
}

/// `niu plugin source <verb>` — external plugin-manager sources
/// (oh-my-bash loader, bash-it, bpkg) as first-class plugin origins.
/// Design: docs/planning/oh-my-niu-ecosystem.md §11-§12.
fn run_plugin_source_command(args: &[String]) -> anyhow::Result<()> {
    let Some(verb) = args.first() else {
        print_plugin_source_usage();
        return Ok(());
    };
    match verb.as_str() {
        "-h" | "--help" => {
            print_plugin_source_usage();
            Ok(())
        }
        "list" => run_plugin_source_list_command(&args[1..]),
        "add" => run_plugin_source_add_command(&args[1..]),
        "trust" => run_plugin_source_trust_command(&args[1..]),
        "remove" => run_plugin_source_remove_command(&args[1..]),
        "update" => run_plugin_source_update_command(&args[1..]),
        "rollback" => run_plugin_source_rollback_command(&args[1..]),
        "verify" => run_plugin_source_verify_command(&args[1..]),
        unknown => anyhow::bail!("unknown plugin source subcommand '{}'", unknown),
    }
}

fn print_plugin_source_usage() {
    println!("Usage:  niu plugin source <command>");
    println!();
    println!("External plugin-manager sources (oh-my-bash loader, bash-it, bpkg).");
    println!("Sources install untrusted; assets activate only after trust.");
    println!();
    println!("Commands:");
    println!("  list [--json]           List installed sources and their state");
    println!("  add <id|url|path> [--ref <ref>] [--checksum <sha256>]");
    println!("                          Install a source tree (untrusted)");
    println!("  trust <id>              Review and activate a source's assets");
    println!("  remove <id>             Uninstall a source tree and its record");
    println!("  update <id> [--ref <ref>] [--checksum <sha256>]");
    println!("                          Update a source (previous state kept)");
    println!("  rollback <id>           Restore the previous source state");
    println!("  verify <id>             Re-check the source tree checksum");
}

struct PluginSourceArgs {
    /// First positional: adapter id, git url, or local path.
    target: Option<String>,
    ref_name: Option<String>,
    checksum: Option<String>,
    path: Option<String>,
    url: Option<String>,
}

fn parse_plugin_source_args(args: &[String]) -> anyhow::Result<PluginSourceArgs> {
    let mut parsed = PluginSourceArgs {
        target: None,
        ref_name: None,
        checksum: None,
        path: None,
        url: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--ref" {
            parsed.ref_name = Some(
                iter.next()
                    .ok_or_else(|| anyhow::anyhow!("--ref requires a value"))?
                    .clone(),
            );
        } else if let Some(value) = arg.strip_prefix("--ref=") {
            parsed.ref_name = Some(value.to_string());
        } else if arg == "--checksum" {
            parsed.checksum = Some(
                iter.next()
                    .ok_or_else(|| anyhow::anyhow!("--checksum requires a value"))?
                    .clone(),
            );
        } else if let Some(value) = arg.strip_prefix("--checksum=") {
            parsed.checksum = Some(value.to_string());
        } else if arg == "--path" {
            parsed.path = Some(
                iter.next()
                    .ok_or_else(|| anyhow::anyhow!("--path requires a value"))?
                    .clone(),
            );
        } else if arg == "--url" {
            parsed.url = Some(
                iter.next()
                    .ok_or_else(|| anyhow::anyhow!("--url requires a value"))?
                    .clone(),
            );
        } else if !arg.starts_with('-') {
            if parsed.target.is_some() {
                anyhow::bail!("plugin source accepts at most one positional argument");
            }
            parsed.target = Some(arg.clone());
        } else {
            anyhow::bail!("unknown plugin source option '{}'", arg);
        }
    }
    Ok(parsed)
}

/// Resolve CLI target/flags into an install request. The first positional
/// may be an adapter id (origin then comes from --url/--path), or the origin
/// itself (git url / local directory), with the adapter auto-detected from
/// the fetched tree's layout.
fn resolve_source_install_request(args: PluginSourceArgs) -> anyhow::Result<PluginSourceRequest> {
    let explicit_origin = args.url.clone().or_else(|| args.path.clone());
    let (adapter, origin) = match (&args.target, &explicit_origin) {
        (Some(target), Some(origin)) => {
            // Target names a known adapter kind.
            let known = niubash_runtime::plugins::sources::adapter_for(target).is_some();
            if !known {
                anyhow::bail!(
                    "unknown source kind '{}'; {}",
                    target,
                    supported_sources_hint()
                );
            }
            (Some(target.clone()), origin.clone())
        }
        (Some(target), None) => {
            // The positional is the origin (git url or local directory);
            // a bare adapter id with no origin is an error.
            if niubash_runtime::plugins::sources::adapter_for(target).is_some()
                && !std::path::Path::new(target).exists()
                && !target.contains('/')
                && !target.contains('\\')
                && !target.contains("://")
            {
                anyhow::bail!(
                    "source '{target}' needs an origin: add --url <git-url> or --path <dir>"
                );
            }
            (None, target.clone())
        }
        (None, Some(origin)) => (None, origin.clone()),
        (None, None) => {
            anyhow::bail!("plugin source add requires <id|url|path> (or --url/--path with an id)")
        }
    };
    Ok(PluginSourceRequest {
        adapter,
        origin,
        ref_name: args.ref_name,
        expected_checksum: args.checksum,
    })
}

struct PluginSourceRequest {
    adapter: Option<String>,
    origin: String,
    ref_name: Option<String>,
    expected_checksum: Option<String>,
}

impl PluginSourceRequest {
    fn to_install_request(&self) -> niubash_runtime::plugins::sources::SourceInstallRequest {
        niubash_runtime::plugins::sources::SourceInstallRequest {
            adapter: self.adapter.clone(),
            origin: self.origin.clone(),
            ref_name: self.ref_name.clone(),
            expected_checksum: self.expected_checksum.clone(),
        }
    }
}

fn supported_sources_hint() -> String {
    let ids: Vec<&str> = niubash_runtime::plugins::sources::builtin_source_adapters()
        .iter()
        .map(|adapter| adapter.id())
        .collect();
    format!("supported plugin-manager sources: {}", ids.join(", "))
}

/// Trust-boundary notice printed before fetching third-party shell code
/// (§12.2 fetch gate). Fetching never executes the fetched code and the
/// result registers untrusted, so non-interactive runs stay safe.
fn print_source_trust_boundary(id: &str, request: &PluginSourceRequest) {
    println!(
        "{}: fetching third-party shell code",
        niubash_runtime::text_style::yellow("Trust boundary")
    );
    println!("  source:   {}", id);
    println!("  origin:   {}", request.origin);
    if let Some(ref_name) = &request.ref_name {
        println!("  ref:      {}", ref_name);
    }
    println!(
        "  license:  {}",
        niubash_runtime::plugins::sources::adapter_for(id)
            .map(|adapter| adapter.license())
            .unwrap_or("(detected after fetch)")
    );
    if let Some(checksum) = &request.expected_checksum {
        println!("  checksum: {}", checksum);
    }
    println!(
        "  {}",
        niubash_runtime::text_style::dim(
            "fetched code is inert until you review and trust it; nothing is sourced yet"
        )
    );
}

fn run_plugin_source_add_command(args: &[String]) -> anyhow::Result<()> {
    let parsed = parse_plugin_source_args(args)?;
    let request = resolve_source_install_request(parsed)?;
    let display_id = request
        .adapter
        .clone()
        .unwrap_or_else(|| "(auto-detect)".to_string());
    print_source_trust_boundary(&display_id, &request);
    let record = niubash_runtime::plugins::sources::add_source(request.to_install_request())?;
    println!(
        "{} source '{}' ({}) into {}",
        niubash_runtime::text_style::green("Installed"),
        record.id,
        niubash_runtime::text_style::dim(&record.version),
        niubash_runtime::text_style::dim(&record.path.display().to_string())
    );
    println!(
        "license {} | tree sha256 {}",
        record.license, record.checksum_sha256
    );
    println!("the source is untrusted; review it, then run:");
    println!("  niu plugin source trust {}", record.id);
    Ok(())
}

fn run_plugin_source_list_command(args: &[String]) -> anyhow::Result<()> {
    let json = args.iter().any(|arg| arg == "--json");
    let statuses = niubash_runtime::plugins::sources::list_sources();
    if json {
        println!("{}", serde_json::to_string_pretty(&statuses)?);
        return Ok(());
    }
    println!(
        "{}",
        niubash_runtime::text_style::bold("Niubash plugin sources")
    );
    println!(
        "{}",
        niubash_runtime::text_style::dim("  (external plugin managers; untrusted until trusted)")
    );
    if statuses.is_empty() {
        println!("(no sources installed; add one with niu plugin source add <id|url|path>)");
        return Ok(());
    }
    for status in statuses {
        let marker = match status.state.as_str() {
            "ready" => niubash_runtime::text_style::green("ready"),
            "untrusted" => niubash_runtime::text_style::yellow("untrusted"),
            _ => niubash_runtime::text_style::red("degraded (native fallback active)"),
        };
        let assets = status
            .asset_count
            .map(|count| {
                format!(
                    "{} asset{} ({})",
                    count,
                    if count == 1 { "" } else { "s" },
                    status.asset_kinds.join("/")
                )
            })
            .unwrap_or_else(|| "no assets".to_string());
        println!(
            "  {} {:<12} {:<10} {} {}",
            marker,
            status.record.id,
            status.record.version,
            niubash_runtime::text_style::dim(&assets),
            niubash_runtime::text_style::dim(&status.record.path.display().to_string())
        );
    }
    Ok(())
}

fn run_plugin_source_trust_command(args: &[String]) -> anyhow::Result<()> {
    let Some(id) = args.first() else {
        anyhow::bail!("plugin source trust requires a source id");
    };
    // Review summary before flipping the execution gate (§12.2).
    let statuses = niubash_runtime::plugins::sources::list_sources();
    let Some(status) = statuses.iter().find(|status| status.record.id == *id) else {
        anyhow::bail!("unknown source '{}'; run niu plugin source add first", id);
    };
    let verify = niubash_runtime::plugins::sources::verify_source(id)?;
    println!(
        "{} source '{}' review",
        niubash_runtime::text_style::bold("Trust"),
        status.record.id
    );
    println!("  origin:   {}", status.record.url);
    println!("  version:  {}", status.record.version);
    println!("  license:  {}", status.record.license);
    println!(
        "  checksum: {} ({})",
        status.record.checksum_sha256,
        if verify.verified {
            niubash_runtime::text_style::green("verified")
        } else if verify.degraded {
            niubash_runtime::text_style::red("tree missing")
        } else {
            niubash_runtime::text_style::red("MISMATCH")
        }
    );
    println!("  path:     {}", status.record.path.display());
    if verify.degraded {
        anyhow::bail!("cannot trust a degraded source (tree missing)");
    }
    let trusted = niubash_runtime::plugins::sources::trust_source(id)?;
    println!(
        "{} '{}' is now trusted; its themes/assets join the catalog",
        niubash_runtime::text_style::green("Trusted:"),
        trusted.id
    );
    println!("restart niu (or reload ~/.niubashrc) for the change to take effect");
    Ok(())
}

fn run_plugin_source_remove_command(args: &[String]) -> anyhow::Result<()> {
    let Some(id) = args.first() else {
        anyhow::bail!("plugin source remove requires a source id");
    };
    let path = niubash_runtime::plugins::sources::remove_source(id)?;
    println!(
        "{} source '{}' ({})",
        niubash_runtime::text_style::green("Removed"),
        id,
        niubash_runtime::text_style::dim(&path.display().to_string())
    );
    Ok(())
}

fn run_plugin_source_update_command(args: &[String]) -> anyhow::Result<()> {
    let parsed = parse_plugin_source_args(args)?;
    let Some(id) = parsed.target.clone() else {
        anyhow::bail!("plugin source update requires a source id");
    };
    let request = PluginSourceRequest {
        adapter: None,
        // Empty origin means "re-fetch the registered origin".
        origin: parsed
            .url
            .clone()
            .or(parsed.path.clone())
            .unwrap_or_default(),
        ref_name: parsed.ref_name,
        expected_checksum: parsed.checksum,
    };
    let summary =
        niubash_runtime::plugins::sources::update_source(&id, request.to_install_request())?;
    println!(
        "{} source '{}' to {}",
        niubash_runtime::text_style::green("Updated"),
        summary.id,
        niubash_runtime::text_style::dim(&summary.version)
    );
    println!("tree sha256 {}", summary.checksum_sha256);
    if summary.previous.is_some() {
        println!(
            "{} niu plugin source rollback {}",
            niubash_runtime::text_style::dim("undo:"),
            summary.id
        );
    }
    Ok(())
}

fn run_plugin_source_rollback_command(args: &[String]) -> anyhow::Result<()> {
    let Some(id) = args.first() else {
        anyhow::bail!("plugin source rollback requires a source id");
    };
    let summary = niubash_runtime::plugins::sources::rollback_source(id)?;
    println!(
        "{} source '{}' to {}",
        niubash_runtime::text_style::green("Rolled back"),
        summary.id,
        niubash_runtime::text_style::dim(&summary.version)
    );
    Ok(())
}

fn run_plugin_source_verify_command(args: &[String]) -> anyhow::Result<()> {
    let Some(id) = args.first() else {
        anyhow::bail!("plugin source verify requires a source id");
    };
    let report = niubash_runtime::plugins::sources::verify_source(id)?;
    if report.degraded {
        anyhow::bail!(
            "source '{}' tree is missing ({})",
            report.id,
            report.recorded_checksum
        );
    }
    if !report.verified {
        anyhow::bail!(
            "checksum mismatch for source '{}': recorded {}, got {}",
            report.id,
            report.recorded_checksum,
            report.actual_checksum.as_deref().unwrap_or("?")
        );
    }
    println!(
        "{} source '{}' tree checksum {}",
        niubash_runtime::text_style::green("Verified"),
        report.id,
        report.recorded_checksum
    );
    Ok(())
}

fn print_plugin_usage() {
    println!("Usage:  niu plugin <command>");
    println!();
    println!("External plugin ecosystem (the built-in plugin/theme stack retired,");
    println!("niubash#145): plugin-manager sources install untrusted and activate");
    println!("only after an explicit trust review.");
    println!();
    println!("Commands:");
    println!("  discover [--verbose]      Dry ecosystem overview (read-only)");
    println!("  source list [--json]     List external plugin-manager sources");
    println!("  source add <id|url|path> [--ref <ref>] [--checksum <sha256>]");
    println!("                           Install a plugin-manager source (untrusted)");
    println!("  source trust <id>        Review and activate a source's assets");
    println!("  source remove <id>       Uninstall a source tree");
    println!("  source update <id>       Update a source (previous state kept)");
    println!("  source rollback <id>     Restore the previous source state");
    println!("  source verify <id>       Re-check the source tree checksum");
}

/// Parse `--preset <name>` / `--preset=<name>` from `niu setup` arguments.
/// Unknown flags are ignored so the interactive wizard keeps working.
fn setup_preset_arg(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--preset" {
            return iter.next().cloned();
        }
        if let Some(name) = arg.strip_prefix("--preset=") {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(windows)]
fn install_windows_terminal_profile(rest: &[String]) -> anyhow::Result<()> {
    let mut set_default = false;
    let mut quiet = false;

    for arg in rest {
        match arg.as_str() {
            "--set-default" => set_default = true,
            "--quiet" => quiet = true,
            unknown => anyhow::bail!("unknown --install-wt-profile option '{}'", unknown),
        }
    }

    let commandline = std::env::current_exe()?;
    let icon = windows_terminal_icon_path(&commandline);
    let summary = niubash_runtime::windows_terminal::install_niubash_profile(
        &commandline,
        icon.as_deref(),
        set_default,
        None,
    )?;

    if !quiet {
        if summary.updated.is_empty() {
            println!("No Windows Terminal settings path was found.");
        } else {
            for path in summary.updated {
                println!("Updated Windows Terminal profile: {}", path.display());
            }
        }
    }

    Ok(())
}

/// Windows Terminal profile management is Windows-only; fail explicitly
/// instead of silently succeeding on Unix.
#[cfg(not(windows))]
fn install_windows_terminal_profile(_rest: &[String]) -> anyhow::Result<()> {
    anyhow::bail!("--install-wt-profile is only supported on Windows")
}

#[cfg(windows)]
fn windows_terminal_icon_path(commandline: &std::path::Path) -> Option<PathBuf> {
    let app_dir = commandline.parent()?;
    [
        app_dir.join("assets").join("niubash-icon-256.png"),
        app_dir.join("assets").join("niubash-icon.png"),
        app_dir.join("niubash-icon-256.png"),
        app_dir.join("niubash-icon.png"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn print_completion_probe(rest: &[String]) -> anyhow::Result<()> {
    let Some(line) = rest.first() else {
        anyhow::bail!("--completion-probe requires an input line");
    };
    let cursor_pos = if let Some(raw) = rest.get(1) {
        raw.parse::<usize>()
            .map_err(|_| anyhow::anyhow!("invalid cursor position '{}'", raw))?
    } else {
        line.len()
    };
    let mut shell = niubash_runtime::Shell::new()?;
    shell.run_startup_rc();
    for suggestion in shell.completion_probe(line, cursor_pos) {
        println!("{}", suggestion);
    }
    Ok(())
}

fn print_version() {
    // niubash#140: println! panics when stdout is a pipe the reader already
    // closed (os error 232), so `niu --version | true` aborted the launcher.
    // Write through an explicit handle and swallow the error, matching the
    // engine-side #125 policy (is_broken_pipe_error below).
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "Niubash {} \u{2014} bash-compatible shell for Windows",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(out, "  rubash   {}", rubash_revision_label());
    if let Some(v) = niubash_runtime::winuxcmd::version() {
        let _ = writeln!(out, "  winuxcmd {}", v);
    }
}

/// Format the embedded rubash revision. The `git ` prefix is only truthful
/// when build.rs resolved a real commit; a build without git access resolves
/// to "unknown" and must not be advertised as a branch name.
fn rubash_revision_label() -> String {
    let revision = option_env!("NIU_RUBASH_REV").unwrap_or("unknown");
    if revision == "unknown" {
        revision.to_string()
    } else {
        format!("git {revision}")
    }
}

fn is_broken_pipe_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(is_broken_pipe_io_error)
            || cause.to_string().contains("os error 232")
            || cause.to_string().contains("管道正在被关闭")
    })
}

fn is_broken_pipe_io_error(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::BrokenPipe || error.raw_os_error() == Some(232)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dumped_bodies(input: &str) -> Vec<String> {
        let mut strings = Vec::new();
        collect_locale_strings(input, 1, &mut strings);
        strings.into_iter().map(|(_, body)| body).collect()
    }

    /// GNU bash 5.3.0 (`--dump-strings`, probed case-by-case): locale
    /// strings dump only where the word lexer sees `$"`, never in comment
    /// text, here-doc bodies, single-quoted text, double-quoted spans or
    /// backtick bodies; assignment RHS and command substitutions do.
    #[test]
    fn dump_strings_recognition_matches_gnu_word_lexer() {
        assert_eq!(dumped_bodies("echo $\"plain\""), vec!["plain"]);
        assert_eq!(dumped_bodies("echo a$\"mid\"dle"), vec!["mid"]);
        assert_eq!(dumped_bodies("x=$\"assign.rhs\""), vec!["assign.rhs"]);
        assert_eq!(dumped_bodies("echo $\"one\" $\"two\""), vec!["one", "two"]);
        assert_eq!(dumped_bodies("echo $\"a\"$\"b\""), vec!["a", "b"]);
        assert!(dumped_bodies("# comment with $\"in.comment\" text").is_empty());
        assert!(dumped_bodies("echo '$\"single.quoted\" not locale'").is_empty());
        assert!(dumped_bodies("echo \"$\"dqp\" tail\"").is_empty());
        assert!(dumped_bodies("echo \"a$\"b\"c\"").is_empty());
        assert!(dumped_bodies("echo `echo $\"in.backtick\"`").is_empty());
        assert!(dumped_bodies("echo \\$\"escaped.dollar\"").is_empty());
        assert_eq!(
            dumped_bodies("cat <<EOF\nheredoc body with $\"in.heredoc\"\nEOF\necho $\"after\""),
            vec!["after"]
        );
    }

    /// GNU prints the raw body text between the quotes, verbatim: escapes
    /// stay escaped (`locale.c locale_expand` printf("\"%s\"\n", temp)).
    #[test]
    fn dump_strings_keeps_escapes_raw() {
        assert_eq!(
            dumped_bodies("echo $\"esc \\\"q1\\\" q2\""),
            vec!["esc \\\"q1\\\" q2"]
        );
        assert_eq!(
            dumped_bodies("echo $\"tail.backslash\\\\\""),
            vec!["tail.backslash\\\\"]
        );
        // A real newline inside the string stays in the dumped body.
        assert_eq!(dumped_bodies("echo $\"multi\nline\""), vec!["multi\nline"]);
    }

    /// Command substitutions are re-lexed (parse.y:4100), whichever quoting
    /// context hides them; backtick bodies are not.
    #[test]
    fn dump_strings_recurses_into_command_substitutions() {
        assert_eq!(
            dumped_bodies("echo $(echo $\"in.comsub\")"),
            vec!["in.comsub"]
        );
        assert_eq!(
            dumped_bodies("echo pre$(echo $\"midcomsub\")post"),
            vec!["midcomsub"]
        );
        assert_eq!(
            dumped_bodies("echo \"$(echo $\"quotedcomsub\")\""),
            vec!["quotedcomsub"]
        );
        assert_eq!(
            dumped_bodies("echo $(( $(echo $\"arithcomsub\") + 1 ))"),
            vec!["arithcomsub"]
        );
    }

    /// locale.c mk_msgstr: `"` and `\` backslash-escaped, embedded newlines
    /// split as `\n` + quote close/reopen with an empty first msgid, entry
    /// anchored by `#: name:lineno` (line = the `$"` line).
    #[test]
    fn dump_po_strings_matches_gnu_format() {
        let render = |input: &str| {
            let mut strings = Vec::new();
            collect_locale_strings(input, 1, &mut strings);
            render_locale_string_dump(&strings, true, "probe.sh")
        };
        assert_eq!(
            render("echo $\"plain\""),
            "#: probe.sh:1\nmsgid \"plain\"\nmsgstr \"\"\n"
        );
        assert_eq!(
            render("echo $\"esc \\\"q1\\\" q2\""),
            "#: probe.sh:1\nmsgid \"esc \\\\\\\"q1\\\\\\\" q2\"\nmsgstr \"\"\n"
        );
        assert_eq!(
            render("echo $\"multi\nline\""),
            "#: probe.sh:1\nmsgid \"\"\n\"multi\\n\"\n\"line\"\nmsgstr \"\"\n"
        );
        // One entry per string, anchored on its own line.
        assert_eq!(
            render("echo $\"one\" $\"two\"\necho $\"three\""),
            "#: probe.sh:1\nmsgid \"one\"\nmsgstr \"\"\n\
             #: probe.sh:1\nmsgid \"two\"\nmsgstr \"\"\n\
             #: probe.sh:2\nmsgid \"three\"\nmsgstr \"\"\n"
        );
        assert_eq!(
            render("echo $\"\""),
            "#: probe.sh:1\nmsgid \"\"\nmsgstr \"\"\n"
        );
    }

    /// GNU yy_input_name() convention: the script path as given, the literal
    /// `-c` for -c input, the shell's own argv[0] for standard input.
    #[test]
    fn invocation_source_name_follows_gnu_convention() {
        let mut invocation = ShellInvocation::parse(&[]).unwrap();
        assert_eq!(
            invocation_source_name(&invocation),
            std::env::args().next().unwrap_or_else(|| "niu".to_string())
        );
        invocation.command = Some("echo hi".to_string());
        assert_eq!(invocation_source_name(&invocation), "-c");
        invocation.command = None;
        invocation.script = Some("D:/repo/probe.sh".to_string());
        assert_eq!(invocation_source_name(&invocation), "D:/repo/probe.sh");
    }
}
