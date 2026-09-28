---
tags: [niubash, setup, wizard, oh-my-niu, ecosystem, ux]
created: 2026-09-27
status: implemented (wt/wizard)
---

# Wizard Redesign: zsh-Style First Run + Ecosystem Surfacing

Owner decision, 2026-09-27. This supersedes the *interactive* parts of
`docs/planning/setup-wizard-redesign.md` (the earlier preset-survey design)
and the first-run shape of §6.1 in `docs/planning/oh-my-niu-ecosystem.md`
(the sectioned wizard with multi-select plugin picking). Those documents
keep their non-wizard content; where they conflict with the directives
below, the directives win.

## Directives (owner, verbatim intent)

1. Do the oh-my-niu / bash-plugin-ecosystem part first, then the wizard.
2. Never tell the user what to do; never bundle-install things for them.
   Every choice is theirs. The wizard offers, the user decides. No "we
   also installed X for you".
3. Simple wizard beats featureful wizard — reference the zsh ecosystem's
   first-run experience (oh-my-zsh post-install hints: a couple of plain
   questions, one screen of "here's how to change things", out of the
   way). NO multi-page survey.
4. Our old built-in themes retire to the END of the gallery — external
   ones (vendored oh-my-bash corpus, proven 252/252 + PS1 byte parity in
   rubash) lead, built-ins last and honestly labeled (保底/fallback),
   because the external ones look better.

## Reference: what zsh-land actually does

- **oh-my-zsh installer** (`sh install.sh`): at most one plain question
  ("change default shell?"), then replaces `.zshrc` with a heavily
  commented template, prints a short post-install hint block, and gets out
  of the way. Unattended mode (`unattended.sh`) asks nothing.
- **oh-my-zsh post-install hints**: a compact block pointing at the rc
  file — "edit ~/.zshrc to change plugins/themes", one line each. Nothing
  installs itself afterwards.
- Lesson applied: one screen, every opt-in default-off, a commented rc
  footer, and a "change things later" block instead of follow-up nagging.

## Phase 1 — ecosystem surfacing (built on the WP-S1 source adapter)

### Theme catalog layering (`crates/niubash-runtime/src/plugins/mod.rs`)

- `plugin_theme_catalog()` now **guarantees** the §0 order by sorting on a
  tier rank: `user` (0) → `external_source` (1) → `bundle` (2), alphabetical
  within a tier (`theme_catalog_rank`). A future compiled-built-in layer
  would sort last.
- Fixed the same-name dedup bug in the bundle loop: bundle entries no
  longer duplicate a name the external layer already claimed — external
  wins the collision once (§11.3), `native:<name>` still reaches the
  built-in.
- `plugin_theme_catalog_text()` (the `niu plugin themes` listing) renders
  three sections: `User themes` → **`External themes (primary)`** →
  separator `── built-in fallback 保底 ──` → `Built-in themes`. When the
  built-in section is empty because no theme bundle is installed, it says
  `(no built-in theme bundle installed — compiled fallback active)` — the
  visible marker for the compiled layer (§11.4).

### `niu plugin discover` — dry, read-only ecosystem overview (`src/main.rs`)

- Lists registered external sources with their §11.4 state
  (`ready` / `untrusted` / `degraded`) and asset counts.
- Lists **available** managers not installed yet, each with the exact
  `niu plugin source add <id> --url <origin>` command the user could run
  (origin from the new `PluginSourceAdapter::default_origin()`; oh-my-bash
  → `https://github.com/ohmybash/oh-my-bash.git`, §12.1 index entry).
- Theme/pack counts with pointers to `niu plugin themes` / `niu plugin list`.
- Touches nothing — asserted in tests: the registry file is not even
  created by a listing.

### Wizard gallery ordering

`theme_gallery()` (setup_wizard.rs) merges user → external (trusted
sources only) → built-in entries, dedup by tier precedence, and exposes
`builtin_start` — the separator index. External oh-my-bash themes are the
default gallery body (corpus-proven); built-ins carry the
`· built-in fallback` label and sit strictly after the separator.

## Phase 2 — the wizard flow (setup_wizard.rs)

Final interactive flow:

1. Welcome banner + environment detection summary (informational, no
   questions; unchanged).
2. **Theme pick** — one gallery menu: `Skip — keep current theme` first
   (always an equal option, shows what "current" resolves to), then user
   and external oh-my-bash themes, then built-in themes after the
   separator, each labeled `· built-in fallback`. Live preview per entry
   (native themes render the real colors; external themes show their
   channel note; Nerd-Font themes without a Nerd Font get an optional
   `niu font` hint line — a note, not a question).
3. **Extra tab completions** (opt-in, default Skip): adds the completion
   packs for tools found on PATH (git/docker/kubectl/npm). Opting in only
   ever *adds* packs to the previous selection; it never writes
   `NIU_DISABLE_DEFAULT_PLUGINS`.
4. **niu-git** (offered once): `Skip` (default) / `Install via wpm
   (wpm install niugit)` / `Don't ask again`. Never auto-installed;
   install runs only on the explicit pick, at Apply time, through wpm.
   `Don't ask again` and successful installs are recorded in
   `~/.niubash/wizard-answers.toml` (`niubash:wizard-answers@0.1.0`) so
   the question never returns; a plain Skip stays transient (a wizard
   re-run is user-initiated, not a nag). The install command comes
   read-only from the niu-git repo docs: `D:/repo/niu-git` README "WPM
   package" + `wpm/niugit.json` (official-index entry name `niugit`).
5. **Summary + explicit Apply/Cancel** — only picked rows are listed;
   everything else is marked untouched. Cancel writes nothing.
6. **Finish**: rc written (backup kept), then one compact
   "Change things later" block (theme / plugins / ecosystem / font / WT /
   re-run) plus the same pointers as comments inside the rc.

Retired from the interactive wizard (still available elsewhere):

| Retired step | Replacement |
| --- | --- |
| Nerd Font install question | `niu font` (standalone); gallery shows an optional hint line |
| Preset menu (recommended/poweruser/minimal/custom) | `niu setup --preset <name>` (explicit, non-interactive); non-interactive `niu setup` keeps applying `minimal` deterministically |
| 10-question custom flow (path/symbol/style/segments/right-side/completion-style/…) | edit `~/.niubashrc`; the defaults are the product's own |
| Starship engine question | `niu plugin enable starship` / `NIU_PROMPT_GIT_BACKEND` documented in the rc/comments |
| wpm companion-tool bundles (Essentials/Modern/Everything) | the user runs `wpm` themselves; the wizard never batch-installs |
| Windows Terminal profile question | `niu --install-wt-profile` (existing explicit command), named in the finish hints |

### Choice-only invariants (asserted by tests)

- **No install without an explicit pick**: only `NiuGitChoice::Install`
  ever invokes wpm; non-interactive setup records no wizard answers at all
  (`setup_noninteractive_stays_deterministic_and_records_no_answers`).
- **Skip paths leave zero side effects**: skip-all on a fresh home writes
  an rc with no `NIU_PLUGINS`, no `NIU_DISABLE_DEFAULT_PLUGINS`, no theme
  lines, no template override, no aliases
  (`wizard_skip_answers_leave_zero_rc_overrides`); on reconfigure the
  previous plugin selection is re-emitted verbatim
  (`wizard_skip_preserves_previous_plugin_selection`); the completion
  opt-in merges, never drops (`wizard_completion_optin_only_adds_packs`).
- **Built-in themes appear after the separator**: unit test over
  `theme_gallery()` and binary test over `niu plugin themes` ordering
  (`theme_gallery_external_first_builtins_after_separator`,
  `plugin_themes_external_first_builtins_after_separator`).

### rc generation semantics

Empty `WizardConfig` fields now mean "no override" — the line is omitted
(so skip paths change nothing). External oh-my-bash picks write
`OSH_THEME=<name>` + the adapter's guarded loader snippet +
`NIU_THEME_SOURCE=omb` (§3.2/§3.3) and deliberately omit
`NIU_THEME`/`NIU_THEME_PLUGIN` and the prompt template call. Presets keep
their exact-selection semantics (`disable_default_plugins = true`), so
`niu setup --preset` output is unchanged.

## Files

- `crates/niubash-runtime/src/plugins/mod.rs` — catalog ordering + dedup +
  layered `niu plugin themes` listing.
- `crates/niubash-runtime/src/plugins/sources.rs` —
  `PluginSourceAdapter::default_origin()`.
- `crates/niubash-runtime/src/setup_wizard.rs` — gallery, minimal flow,
  niu-git choice, answers file, rc generation, i18n.
- `src/main.rs` — `niu plugin discover`.
- `tests/plugin_sources.rs`, `tests/plugin_inventory.rs` — coverage and
  expectation updates.
