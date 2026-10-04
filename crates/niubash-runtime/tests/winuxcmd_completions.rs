//! Golden and behavioral coverage for the embedded WinuxCmd completion assets.
//!
//! Layer 1 (artifact honesty): every embedded TOML parses as a `CommandDef`,
//! covers exactly the committed `--help` transcript corpus, and every flag /
//! subcommand token it advertises appears in that command's transcript. The
//! corpus under `tests/fixtures/winuxcmd-help-corpus/` is the specification —
//! the assets must not invent completions the binary does not document.
//!
//! Layer 2 (runtime behavior): `ExternalCompletionPlugin` serves the embedded
//! definitions — flag completion, static values, wpm subcommands — and a user
//! completion dir still overrides them per command.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use niubash_runtime::completion::external::{CommandDef, ExternalCompletionPlugin};
use niubash_runtime::completion::{CompletionContext, CompletionPlugin};

const MIN_EXPECTED_APPLETS: usize = 170;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/winuxcmd-help-corpus")
}

fn corpus_transcripts() -> BTreeMap<String, String> {
    let mut transcripts = BTreeMap::new();
    for entry in std::fs::read_dir(corpus_dir()).expect("corpus dir must exist") {
        let path = entry.expect("corpus entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("help") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("corpus stem")
            .to_string();
        let text = std::fs::read_to_string(&path).expect("corpus transcript");
        transcripts.insert(stem, text);
    }
    transcripts
}

fn embedded_defs() -> Vec<CommandDef> {
    niubash_runtime::completion::winuxcmd_assets::WINUXCMD_COMPLETION_TOMLS
        .iter()
        .map(|(command, toml)| {
            toml::from_str::<CommandDef>(toml)
                .unwrap_or_else(|e| panic!("embedded definition {command} failed to parse: {e}"))
        })
        .collect()
}

#[test]
fn every_embedded_definition_parses_and_covers_the_corpus() {
    let defs = embedded_defs();
    assert!(
        defs.len() >= MIN_EXPECTED_APPLETS,
        "expected at least {MIN_EXPECTED_APPLETS} embedded applet definitions, got {}",
        defs.len()
    );

    let transcripts = corpus_transcripts();
    // `winuxcmd.help` is the multi-call binary's own inventory transcript;
    // every other corpus file is one applet.
    let corpus_applets: BTreeSet<String> = transcripts
        .keys()
        .filter(|stem| stem.as_str() != "winuxcmd")
        .cloned()
        .collect();
    let embedded_names: BTreeSet<String> = defs.iter().map(|d| d.command.clone()).collect();

    assert_eq!(
        embedded_names, corpus_applets,
        "embedded definitions must cover exactly the corpus applets"
    );
}

#[test]
fn every_flag_token_comes_from_the_help_transcript() {
    let transcripts = corpus_transcripts();
    for def in embedded_defs() {
        let transcript = transcripts
            .get(&def.command)
            .unwrap_or_else(|| panic!("no corpus transcript for {}", def.command));
        for flag in &def.flags {
            if let Some(short) = &flag.short {
                assert!(
                    transcript.contains(short.as_str()),
                    "{}: short flag {short} not found in its help transcript",
                    def.command
                );
            }
            if let Some(long) = &flag.long {
                assert!(
                    transcript.contains(long.as_str()),
                    "{}: long flag {long} not found in its help transcript",
                    def.command
                );
            }
        }
        for subcommand in &def.subcommands {
            assert!(
                transcript.contains(subcommand.name.as_str()),
                "{}: subcommand {} not found in its help transcript",
                def.command,
                subcommand.name
            );
        }
    }
}

#[test]
fn every_embedded_definition_has_flags() {
    for def in embedded_defs() {
        if !def.flags.is_empty() {
            continue;
        }
        // An empty definition is only honest when the transcript itself
        // documents no options (e.g. mpicalc prints bare usage text).
        let transcript = corpus_transcripts()
            .get(&def.command)
            .unwrap_or_else(|| panic!("no corpus transcript for {}", def.command))
            .clone();
        let documents_options = transcript.lines().any(|line| {
            let stripped = line.trim_start();
            stripped.starts_with('-')
                && stripped
                    .chars()
                    .nth(1)
                    .is_some_and(|c| c.is_ascii_alphanumeric())
        });
        assert!(
            !documents_options,
            "{}: transcript documents options but the embedded definition has no flags",
            def.command
        );
    }
}

#[test]
fn completes_flags_for_embedded_applet() {
    let plugin = ExternalCompletionPlugin::new();
    let input = "grep --col".to_string();
    let ctx = CompletionContext::new(PathBuf::from("."), input.clone(), input.len());
    let result = plugin
        .complete(&ctx)
        .expect("grep flag completion must fire from embedded definitions");
    assert!(
        result.completions.contains(&"--color".to_string()),
        "missing --color, got {:?}",
        result.completions
    );
    assert!(
        result.completions.contains(&"--colour".to_string()),
        "missing --colour, got {:?}",
        result.completions
    );
}

#[test]
fn completes_static_flag_values() {
    let plugin = ExternalCompletionPlugin::new();
    let input = "ls --color a".to_string();
    let ctx = CompletionContext::new(PathBuf::from("."), input.clone(), input.len());
    let result = plugin
        .complete(&ctx)
        .expect("ls --color value completion must fire");
    assert!(
        result.completions.contains(&"always".to_string())
            && result.completions.contains(&"auto".to_string()),
        "expected always/auto for `ls --color a`, got {:?}",
        result.completions
    );
}

#[test]
fn completes_wpm_subcommands() {
    let plugin = ExternalCompletionPlugin::new();
    let input = "wpm ins".to_string();
    let ctx = CompletionContext::new(PathBuf::from("."), input.clone(), input.len());
    let result = plugin
        .complete(&ctx)
        .expect("wpm subcommand completion must fire");
    assert!(
        result.completions.contains(&"install".to_string())
            && result.completions.contains(&"installed".to_string()),
        "expected install/installed for `wpm ins`, got {:?}",
        result.completions
    );
}

#[test]
fn bracket_test_applet_is_embedded() {
    let plugin = ExternalCompletionPlugin::new();
    let input = "[ -".to_string();
    let ctx = CompletionContext::new(PathBuf::from("."), input.clone(), input.len());
    let result = plugin.complete(&ctx).expect("[ flag completion must fire");
    assert!(
        result.completions.contains(&"-eq".to_string()),
        "expected -eq for `[ -`, got {:?}",
        result.completions
    );
}

#[test]
fn user_dir_overrides_embedded_definition() {
    let temp = std::env::temp_dir().join(format!(
        "niu-winuxcmd-override-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp).expect("create override dir");
    std::fs::write(
        temp.join("ls.toml"),
        r#"command = "ls"

[[flags]]
long = "--niu-override-flag"
description = "user override marker"
"#,
    )
    .expect("write override toml");

    let mut plugin = ExternalCompletionPlugin::new();
    assert!(
        plugin.definition_names().contains(&"ls"),
        "embedded ls must exist before override"
    );
    plugin.load_dir(&temp);

    let input = "ls --niu".to_string();
    let ctx = CompletionContext::new(PathBuf::from("."), input.clone(), input.len());
    let result = plugin
        .complete(&ctx)
        .expect("override flag completion must fire");
    assert!(
        result
            .completions
            .contains(&"--niu-override-flag".to_string()),
        "user override must replace the embedded ls definition, got {:?}",
        result.completions
    );
    // The wholesale per-command replacement must also drop embedded flags
    // that the override does not carry.
    let input = "ls --colo".to_string();
    let ctx = CompletionContext::new(PathBuf::from("."), input.clone(), input.len());
    assert!(
        plugin.complete(&ctx).is_none(),
        "embedded ls flags must not survive the user override"
    );

    let _ = std::fs::remove_dir_all(&temp);
}
