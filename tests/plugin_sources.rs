//! Binary-level integration smoke for external plugin-manager sources
//! (oh-my-bash loader adapter; docs/planning/oh-my-niu-ecosystem.md §11-§12).
//!
//! Vertical covered: `niu plugin source add` (fetch gate, untrusted) →
//! `trust` (execution gate) → catalog exposure (`niu plugin themes`) →
//! theme load through the niubash engine from the adapter-installed tree →
//! `verify` (tree checksum) → `remove` → built-in fallback verified when
//! the source is absent.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn niu_binary() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_BIN_EXE_niu"));
    if p.exists() {
        return p;
    }
    let mut fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fallback.push("target");
    fallback.push("debug");
    fallback.push(if cfg!(windows) { "niu.exe" } else { "niubash" });
    fallback
}

fn base_niubash_command() -> Command {
    Command::new(niu_binary())
}

fn run_niu_with_env(args: &[&str], envs: &[(&str, PathBuf)]) -> Output {
    let mut command = base_niubash_command();
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .output()
        .unwrap_or_else(|err| panic!("failed to run niubash {args:?}: {err}"))
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with {}:\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("niubash-{name}-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The vendored oh-my-bash-shaped fixture tree (LF, pinned by .gitattributes;
/// layout mirrors D:/repo/rubash/target-ecosys/repos/oh-my-bash).
fn omb_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sources")
        .join("oh-my-bash")
}

#[test]
fn oh_my_bash_source_add_trust_load_and_fallback_lifecycle() {
    let temp = temp_dir("plugin-source-omb");
    let root = temp.join("sources");
    let fixture = omb_fixture();
    let envs = [("NIU_PLUGIN_SOURCES_ROOT", root.clone())];
    let fixture_str = fixture.to_string_lossy().into_owned();

    // Baseline: no sources, catalog falls back to the built-in layers.
    let empty = run_niu_with_env(&["plugin", "source", "list"], &envs);
    assert_success(&empty, "plugin source list empty");
    assert!(
        stdout_text(&empty).contains("no sources installed"),
        "{}",
        stdout_text(&empty)
    );

    // Fetch gate: install is untrusted; assets must not activate yet.
    let add = run_niu_with_env(&["plugin", "source", "add", &fixture_str], &envs);
    assert_success(&add, "plugin source add");
    let add_out = stdout_text(&add);
    assert!(
        add_out.contains("Installed source 'oh-my-bash'"),
        "{add_out}"
    );
    assert!(
        add_out.contains("Trust boundary") && add_out.contains("nothing is sourced yet"),
        "{add_out}"
    );
    assert!(
        add_out.contains("niu plugin source trust oh-my-bash"),
        "{add_out}"
    );

    let listed_untrusted = run_niu_with_env(&["plugin", "source", "list"], &envs);
    assert_success(&listed_untrusted, "plugin source list untrusted");
    assert!(
        stdout_text(&listed_untrusted).contains("untrusted"),
        "{}",
        stdout_text(&listed_untrusted)
    );
    let themes_untrusted = run_niu_with_env(&["plugin", "themes"], &envs);
    assert_success(&themes_untrusted, "plugin themes untrusted");
    assert!(
        !stdout_text(&themes_untrusted).contains("robbyrussell"),
        "untrusted source must not contribute themes: {}",
        stdout_text(&themes_untrusted)
    );

    // Execution gate: trust activates the source's assets in the catalog.
    let trust = run_niu_with_env(&["plugin", "source", "trust", "oh-my-bash"], &envs);
    assert_success(&trust, "plugin source trust");
    let trust_out = stdout_text(&trust);
    assert!(
        trust_out.contains("license")
            && trust_out.contains("MIT")
            && trust_out.contains("verified"),
        "{trust_out}"
    );

    let listed_ready = run_niu_with_env(&["plugin", "source", "list"], &envs);
    assert_success(&listed_ready, "plugin source list ready");
    assert!(
        stdout_text(&listed_ready).contains("ready"),
        "{}",
        stdout_text(&listed_ready)
    );

    // External-first catalog exposure (§11.3): theme entries show up with
    // the external_source trust marker.
    let themes = run_niu_with_env(&["plugin", "themes", "--verbose"], &envs);
    assert_success(&themes, "plugin themes after trust");
    let themes_out = stdout_text(&themes);
    assert!(
        themes_out.contains(
            "- robbyrussell source=external_source owner=oh-my-bash bundle=none pack=none trust_source=external_source"
        ),
        "{themes_out}"
    );
    assert!(themes_out.contains("agnoster"), "{themes_out}");

    // Load the vendored theme through the adapter-installed tree under the
    // niubash engine (script face; interactive activation is gated on
    // rubash#251 per §12.6). Source order mirrors the loader: lib, theme.
    let installed_theme = root
        .join("oh-my-bash")
        .join("themes")
        .join("robbyrussell")
        .join("robbyrussell.theme.sh");
    let installed_lib = root.join("oh-my-bash").join("lib").join("utils.sh");
    let load_script = format!(
        ". {}; . {}; printf '%s' \"$PS1\"",
        installed_lib.to_string_lossy(),
        installed_theme.to_string_lossy()
    );
    let load = run_niu_with_env(&["-c", &load_script], &envs);
    assert_success(&load, "engine theme load");
    let load_out = stdout_text(&load);
    assert_eq!(
        load_out.trim_end(),
        "loading robbyrussell\n➜ prompt-robbyrussell",
        "theme must compose PS1 under the niubash engine"
    );

    // Non-interactive loader safety: the oh-my-bash.sh interactive guard
    // makes the loader a no-op in -c mode (PS1 untouched).
    let osh = root.join("oh-my-bash");
    let guard_script = format!(
        "PS1=before; export OSH={}; . \"{}/oh-my-bash.sh\"; printf '%s' \"$PS1\"",
        osh.to_string_lossy(),
        osh.to_string_lossy()
    );
    let guard = run_niu_with_env(&["-c", &guard_script], &envs);
    assert_success(&guard, "loader guard");
    assert_eq!(stdout_text(&guard).trim_end(), "before");

    // Checksum verification (§12.3).
    let verify = run_niu_with_env(&["plugin", "source", "verify", "oh-my-bash"], &envs);
    assert_success(&verify, "plugin source verify");
    assert!(
        stdout_text(&verify).contains("Verified source 'oh-my-bash'"),
        "{}",
        stdout_text(&verify)
    );

    // Uninstall, then prove the built-in fallback: the catalog returns to
    // the compiled/bundle layers and theme resolution still works.
    let remove = run_niu_with_env(&["plugin", "source", "remove", "oh-my-bash"], &envs);
    assert_success(&remove, "plugin source remove");
    let themes_after = run_niu_with_env(&["plugin", "themes", "--verbose"], &envs);
    assert_success(&themes_after, "plugin themes after remove");
    assert!(
        !stdout_text(&themes_after).contains("robbyrussell"),
        "removed source must stop contributing themes: {}",
        stdout_text(&themes_after)
    );
    let theme_resolve = run_niu_with_env(&["-c", "printf '%s' \"$NIU_THEME\""], &envs);
    assert_success(&theme_resolve, "theme resolution after remove");

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn plugin_source_add_rejects_unknown_layout_and_bad_checksum() {
    let temp = temp_dir("plugin-source-omb-errors");
    let root = temp.join("sources");
    let envs = [("NIU_PLUGIN_SOURCES_ROOT", root.clone())];

    // Not a known plugin-manager tree.
    let random = temp.join("random-tree");
    fs::create_dir_all(&random).unwrap();
    fs::write(random.join("notes.txt"), "not a manager\n").unwrap();
    let bad = run_niu_with_env(
        &["plugin", "source", "add", &random.to_string_lossy()],
        &envs,
    );
    assert!(!bad.status.success(), "unknown layout must fail");
    let bad_out = format!(
        "{}{}",
        stdout_text(&bad),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(
        bad_out.contains("no supported plugin manager") && bad_out.contains("oh-my-bash"),
        "{bad_out}"
    );

    // Checksum mismatch aborts and installs nothing.
    let fixture = omb_fixture();
    let wrong = run_niu_with_env(
        &[
            "plugin",
            "source",
            "add",
            &fixture.to_string_lossy(),
            "--checksum",
            "deadbeef",
        ],
        &envs,
    );
    assert!(!wrong.status.success(), "checksum mismatch must fail");
    assert!(
        String::from_utf8_lossy(&wrong.stderr).contains("checksum mismatch"),
        "{}",
        String::from_utf8_lossy(&wrong.stderr)
    );
    let listed = run_niu_with_env(&["plugin", "source", "list"], &envs);
    assert!(
        stdout_text(&listed).contains("no sources installed"),
        "{}",
        stdout_text(&listed)
    );

    let _ = fs::remove_dir_all(&temp);
}
