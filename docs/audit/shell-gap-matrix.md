# Shell capability gap matrix: niubash/rubash vs fish, zsh, nushell

Lane wt93/abbr (scope-rewritten 2026-10-05: full-capability gap analysis).
Branch `wt93/abbr`, base `b4b87f1`. Companion machine-readable file:
`docs/audit/gap-matrix.json` (generated from the tables in this document).

## Method and provenance

- **Ours** = the actual trees: rubash engine `D:/repo/rubash/src/` (HEAD
  `804d680f`, niubash 1.3.4 chain) and niubash product
  `crates/niubash-runtime/src/` at this branch (base `b4b87f1`). Every "ours"
  cell cites `file:line` I read in those trees.
- **fish** = snapshot `D:/repo/_rtstudy_refs/fish-shell-master` (master
  tarball, Rust fish, `Cargo.toml` package version 4.9.3, changelog unfilled
  "?.?.?" header = between releases; acquired 2026-10-02 by the wt74/rtstudy
  lane). Cited as `fish: src/file.rs:NN`.
- **zsh** = snapshot `D:/repo/_rtstudy_refs/zsh-master` (`Config/version.mk`
  says `VERSION=5.9.999.3-test`, `VERSION_DATE='May 27, 2026'` — upstream
  master after 5.9). Provenance caveat: the snapshot `CHANGELOG` carries one
  non-upstream-looking top entry (2026-09-28, "tiandic"), and master has
  removed the restricted shell (`Src/options.c:359` "formerly RESTRICTED"),
  so this is NOT the 5.9 release tree; treat feature file:line refs as
  representative of master, not a release. Cited as `zsh: Src/...`.
- **nushell** = release tarball 0.108.0, extracted read-only at
  `D:/repo/_rtstudy_refs/nushell-0.108.0`. Cited as `nu: crates/...`.
- **reedline** = snapshot 0.52.0 (`reedline-main`); niubash vendors **0.50**
  (`crates/niubash-runtime/Cargo.toml:11`). Where the engine capability is
  reedline's, the row says so explicitly — "reedline supports X" is not the
  same as "we wire X".
- Every row: dimension, capability, per-shell evidence (`file:line`), our
  status (equivalent / partial / missing / ahead), user impact (high / med /
  low), and a fix-path sketch. Rows where WE are ahead are recorded with
  `—` impact and an `(ahead)` fix path. Do not fabricate: anything I could
  not verify from source is marked *shallow* in the row or in the closing
  honesty notes.

Status vocabulary: **equivalent** = we ship the same capability with
comparable depth; **partial** = present but materially narrower; **missing**
= absent; **ahead** = we have something the references lack; **deliberate** =
gap accepted by an owner decision (cited, e.g. the v3 "no second shell
language runtime" non-goal in `docs/planning/niubash-zsh-gap-analysis.md`).

## Executive top-20 by user impact

1. **Shell-function widgets with a live buffer API** (zsh `BUFFER`/`CURSOR`,
   fish `commandline`): our widget bridge passes a snapshot in/out
   (`shell.rs:49 WidgetOutcome`, `repl.rs:51-69`), and the engine `bind`
   builtin is a syntax-only stub (`rubash src/builtins/bind.rs:24-93`). This
   one primitive unblocks fzf-style tools, abbr, expand-on-space, multi-key
   chords. Highest-leverage editor fix (#185).
2. **Abbreviations** — fish-only among the three, but the single most
   requested modern-editor feature; fully missing here (dimension D02, native
   design sketch included).
3. **Git/VCS prompt segment** — fish `fish_git_prompt`/`fish_vcs_prompt`
   (share/functions), zsh `Functions/VCS_Info` (24 files), nu `nu_plugin_gstat`;
   we have no git segment at all (`prompt_segments.rs:24-35` SegmentId enum).
4. **User-authored prompt rendering** — fish `fish_prompt` function, zsh
   `PROMPT_SUBST`, nu `PROMPT_COMMAND` closure; ours is a template + fixed
   segment catalog (`config.rs:12-29`), no user function surface.
5. **Async prompt execution** — fish renders prompts from background threads
   (`src/threads.rs`, `src/reader/iothreads.rs`); ours computes segments
   synchronously per draw.
6. **Completion authoring depth** — `NIU_COMPDEFS` exists
   (`shell.rs:116-118`) but the contract is narrower than zsh `compdef`
   (`Completion/compinit`) / fish `complete -c` / nu custom completers
   (`nu-cli/src/completions/custom_completions.rs`).
7. **Transient prompt** — reedline already ships it
   (`engine.rs:161,775`); nu wires it (`reedline_config.rs`), fish has it
   (`reader.rs:478,833`); we do not wire it (no `transient` in `repl.rs`).
   Cheapest big win on this list.
8. **`bind` surface (list/rebind/keymaps)** — real `bindkey`/`bind` in zsh
   (`zle_keymap.c`) and fish (`src/builtins/bind.rs`); ours validates syntax
   and prints defaults only (`bind.rs:33-38`).
9. **Incremental history search UX** — fish `src/reader/history_search.rs`
   (smartcase, search modes), zsh `zle_hist.c` incremental search; ours is a
   menu (`repl.rs:129`), not a type-to-narrow search.
10. **History timestamps + cwd annotation** — fish stores timestamps and
    `required_paths` per entry (`src/history/history.rs:163-195`); nu sqlite
    rows; zsh `EXTENDED_HISTORY`. We store neither.
11. **Fuzzy matching** — fish `src/complete.rs:33 StringFuzzyMatch` powers
    completions and autosuggest; our completer and hinter are prefix-only
    (`autosuggest.rs:40 SearchQuery::last_with_prefix`).
12. **Binary plugin protocol + registry** — nu `nu-plugin-protocol`/
    `nu-plugin-engine` with a persisted registry (`nu-cmd-plugin/src/util.rs:43-63`);
    fish/zsh run on script ecosystems; our plugin driver is a curated
    catalog only (`plugins/recipes.rs:60-73` Git + catalog-only Download).
13. **Native string/math/path command families** — fish `src/builtins/string/`
    (15 subcommands), `math`, `path`; nu `math-*`/`str-*`/`path-*`/`date-*`;
    zsh `mathfunc`/`datetime` modules. We depend on winuxcmd externals.
14. **Variable-watcher events** — fish `--on-event --on-variable`
    (`src/event.rs:217-224`), nu `env_change` hooks
    (`nu-protocol/src/config/hooks.rs:10`); our hook set
    (`config.rs:32-39`) has no variable watchers.
15. **Startup/config cache (instant prompt)** — zsh `zcompile`
    (`Src/builtin.c:136`), nu const-eval + plugin cache; we re-source
    everything (the 1.3.4 "6s source" fix treats the symptom).
16. **Did-you-mean diagnostics** — fish suggests `set ...` on bad usage
    (`src/builtins/set.rs:950`); we print bash-style errors only.
17. **Menu search-as-you-type** — fish pager search field
    (`src/pager.rs:34-35`); our reedline menus are arrow-navigable but the
    typed-filter UX is unverified/shallow.
18. **Job lifecycle notifications** — fish fires `job_exit`/`process_exit`
    events (`event.rs:231-242`), zsh `NOTIFY`; we only display a
    background-jobs count segment (`prompt_segments.rs:24`).
19. **LSP / MCP surfaces** (nu-only today: `nu-lsp`, `nu-mcp` crates) —
    strategic opportunity given the AI-native positioning; nobody else has
    MCP.
20. **Universal variables** (fish persistent cross-session env,
    `env_universal_common.rs:301-318,644`) — nothing here persists env
    across sessions except user rc.

Where we are ahead (recorded in-line as `(ahead)` rows): Windows-native
ConPTY without WSL; setup wizard + `niu doctor` onboarding (none of the three
has an equivalent); bash-61 builtin floor plus zsh-name aliases
(`setopt`/`typeset`/`unsetopt`, `rubash src/executor/builtin_names.rs:56-97`);
programmable-completion builtins (`compgen`/`complete`/`compopt` port) plus a
bash-completion importer (`completion/bash_import.rs`); p10k-style segment
presets; a broader lifecycle hook list than fish/zsh cores
(precmd/preexec/chpwd/postcmd/zshaddhistory/zshexit/greeting/title,
`config.rs:32-39`); startup tracing; winuxcmd GNU toolchain; `NIU_ENV`
agent-env sourcing; PTY-driven interactive regression harness + GNU upstream
test corpus.

## Dimension D01 — Interactive line editor core

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D01-01 | vi insert/normal editing modes | fish_vi_key_bindings + fish_mode_prompt (share/functions/fish_vi_key_bindings.fish, fish_mode_prompt.fish) | Src/Zle/zle_vi.c (vi widget set) + zle_keymap.c keymaps | reedline EditMode::Vi wired by nu-cli | equivalent (repl.rs:135-150 build_edit_mode Emacs+Vi; EditorMode config.rs:44) | — | (equivalent) keep |
| D01-02 | emacs binding baseline | fish_default_key_bindings | Src/Zle/zle_bindings.c | reedline Emacs | equivalent (reedline 0.50 via repl.rs:143) | — | (equivalent) keep |
| D01-03 | user-defined editor widgets written in shell code | bind + fish_user_key_bindings + commandline builtin (src/builtins/commandline.rs) | zle -N widgets; params BUFFER/CURSOR/PENDING/POSTDISPLAY/LASTWIDGET/WIDGET (Src/Zle/zle_params.c:142-173) | keybindings map to reedline events only; `keybindings listen` for introspection (nu-cli/src/commands/keybindings_listen.rs) | partial (repl.rs:51-69 parse_user_bindkeys + shell-function widgets via run_widget_function, shell.rs tests:3527; native widget set is a closed name mapping) | high | expose WidgetOutcome with buffer read/write so a shell function can rewrite the line (extends shell.rs:49) |
| D01-04 | live line-buffer access from widget (READLINE_LINE equivalent) | commandline -b/-C/-f builtin | Src/Zle/zle_params.c:142 BUFFER, :147 CURSOR, :164 POSTDISPLAY | reedline gives host nothing mid-edit (engine.rs); nu cannot mutate buffer from user code | partial (WidgetOutcome passes line+cursor in/out at submit boundary, shell.rs:49; no live mutation, no POSTDISPLAY) | high | same as D01-03; add pending-buffer + cursor-write to the widget contract |
| D01-05 | Vim-style text objects (ciw, daw...) | — (word motions: src/reader/word_motion.rs) | Src/Zle/textobjects.c:2 ("ZLE widgets implementing Vim style text objects") | — | missing (reedline word motions only, core_editor/word.rs) | low | post-1.4 editor work; needs widget DSL first (D01-03) |
| D01-06 | undo/redo with transient-edit collapsing | reader.rs:670-672 transient edits merged in undo history | iwidgets.list:121 "undo" | reedline EditCommand Undo/Redo (enums.rs:588-591) | equivalent (reedline undo bound in default edit mode; depth unverified — shallow) | low | verify undo spans for expansions when D02 lands |
| D01-07 | multi-line editing + submit validation | reader validates completeness before accept | zle accept-line + parse check | Validator trait (nu-cli/src/validation.rs) | equivalent (ReplValidator repl.rs:601-603 over rubash continuation) | — | keep |
| D01-08 | bracketed paste mode | src/input/decode.rs | ZLE bracketed-paste widget | reedline supports | equivalent (reedline 0.50; shallow — not hand-verified this pass) | — | keep |
| D01-09 | suffix widgets (auto-suffix-remove/retain) | — | iwidgets.list:16-17 auto-suffix-* | — | missing | low | niche; only after widget DSL |
| D01-10 | key-code reader tool | fish_key_reader builtin (src/builtins/fish_key_reader.rs) | zkbd helper (Completion + Functions) | — | missing | low-med | ship `niu key-reader` on the product layer using crossterm |
| D01-11 | system clipboard keybindings | fish_clipboard_copy/paste functions | — | reedline system_clipboard feature | ahead (feature enabled, crates/niubash-runtime/Cargo.toml:11; add_system_clipboard_keybindings repl.rs:353) | — | (ahead) keep |
| D01-12 | transient prompt (slim prompt for past lines) | reader.rs:478 field, :833 fish_transient_prompt var, :1127 toggle | — (p10k plugin territory) | reedline engine.rs:161,775 with_transient_prompt; nu wires TRANSIENT_PROMPT (nu-cli/src/reedline_config.rs) | missing (reedline 0.50 ships the hook; repl.rs never calls with_transient_prompt — grep shows no transient use) | med-high | wrap current prompt in a compact one-line transient Prompt and pass to with_transient_prompt in repl.rs:135 builder |

## Dimension D02 — Abbreviations (fish-only reference; our #187 fold-in)

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D02-01 | abbreviation storage + token match (name, regex key, per-command scoping, command vs anywhere position) | src/abbrs.rs:11 static set; :25-28 Position::{Command,Anywhere}; :36-44 regex + commands scoping; :47-48 replacement functions | — | — | missing | high | native editor-layer abbr (sketch below); table in config + `abbr` builtin |
| D02-02 | expand at separators (space/enter) before execution | src/reader/reader.rs:4338 expand_abbreviation_at_cursor(1) on separator; :4572-4593 expand before execute (even when incomplete) | — | — | missing | high | hook expansion into the REPL submit path (repl.rs Signal::Submit) + separator key handling |
| D02-03 | cursor placement inside expansion (set-cursor marker) | src/builtins/abbr.rs:38 set_cursor_marker | — | — | missing | med | part of abbr builtin surface |
| D02-04 | function expanders (replacement is a function run at expansion time) | src/abbrs.rs:47-48 replacement_is_function; reader.rs:6018-6030 expand replacer | — | — | missing | med | call widget-function machinery (shell.rs run_widget_function) for function abbrs |
| D02-05 | management builtin: add/rename/show/list/erase/query | src/builtins/abbr.rs:29-64 cmds + flags | — (zsh-abbr is a third-party plugin) | — | missing | med | implement `abbr` builtin in niubash-runtime over the config table |
| D02-06 | persistence across sessions | universal var fish_user_abbreviations (src/env_universal_common.rs:644 UVARS_VERSION_3_0 file format) | — | — | missing | med | persist in niu config (FullConfig config.rs:384) instead of a universal-var file |
| D02-07 | history records the EXPANDED command line | expansion replaces the buffer before execution (reader.rs:4572-4593), history receives expanded form | — | — | n/a (follows once D02-02 lands) | med | expansion must mutate the submitted line, not just the display |
| D02-08 | design decision recorded | — | — | — | missing — **recommendation: native editor-layer abbr.** Sketch: store abbrs in FullConfig + runtime table; in repl.rs, on SPACE/ENTER key events, if the token left of the cursor matches, run the expansion as a ReedlineEvent sequence (send EditCommands replacing the token) so undo collapses it (reedline enums.rs:588), then let execute see the expanded line (acceptance: fish D02-01/02/07 semantics). Engine `bind` (#185) is NOT required for this; the product-layer REPL owns the keys. | high | separate lane wt??/abbr-native |

## Dimension D03 — Syntax highlighting (type-time)

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D03-01 | token-class highlighting while typing | src/highlight/highlight.rs:1327 HighlightRole enum | — core (zle_highlight covers regions only, Src/Zle/zle_refresh.c:207-211) | nu-cli/src/syntax_highlight.rs + nu-color-config | partial (17 SyntaxKind classes, syntax_highlighting.rs:16-36; style keys nushell-compatible names) | low-med | add error/keyword-argument/glob classes toward fish's role set |
| D03-02 | unknown/invalid command colored as error at type time | highlight.rs command-not-found check via parser + completions | — | nu validator/highlighter marks errors | partial (UnknownToken kind exists, syntax_highlighting.rs:19; whether it distinguishes "will not resolve" is unverified — shallow) | med | validate token against builtin_names() + PATH at highlight time |
| D03-03 | autosuggestion validity re-check (don't suggest dead commands) | reader.rs:49 autosuggest_validate_from_history | — | — | missing (hinter returns raw history, autosuggest.rs:40) | low | validate hint head before display |
| D03-04 | live parse-error marking | fish highlights syntax errors live | — | Validator returns error message position | partial (ReplValidator blocks submit only, repl.rs:601; no error span rendering) | low | reuse rubash parser error offsets in the highlighter |
| D03-05 | user-configurable highlight styles | fish_color_* universal vars (src/env_dispatch.rs dispatch) | zle_highlight | nu color_config block | equivalent (SyntaxHighlightConfig config.rs:303) | — | keep |

## Dimension D04 — Autosuggestions and hints

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D04-01 | history-prefix autosuggestion inline | reader.rs:474-475 autosuggest_ok, :635 autosuggestion field | — core (zsh-autosuggestions is a plugin) | reedline Hinter (nu HistoryAutosuggestHinter equivalents in nu-cli) | partial (HistoryAutosuggestHinter autosuggest.rs:11-46: latest prefix match only) | med | rank candidates (freq/recency), respect multi-line entries |
| D04-02 | fuzzy/subsequence history suggestion | src/complete.rs:33 StringFuzzyMatch used by history search paths | — | reedline search supports substring/fuzzy via SearchQuery | missing (prefix-only, autosuggest.rs:40) | med | switch to reedline fuzzy SearchQuery + dedupe |
| D04-03 | word-wise accept of suggestion (shift-right) | fish accept-one-word binding | plugin | reedline CompleteHistoryWord event | partial (present in reedline 0.50; binding surfaced in product — shallow, unverified) | low | verify + add default keybinding if missing |
| D04-04 | multiple hint sources chainable | — | — | single Hinter trait (reedline hinter/mod) | partial (single hinter wired, repl.rs:151) | low | composable hinter list when second source exists |
| D04-05 | type-ahead protection during slow renders | fish input queue handling (src/input/input.rs) | ZLE handles pending input | reedline input buffering | ahead (typeahead_guard.rs dedicated module) | — | (ahead) keep |

## Dimension D05 — History (search UX + internals)

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D05-01 | incremental type-to-narrow search (Ctrl+R) | src/reader/history_search.rs (SearchMode + smartcase_flags :21) | Src/Zle/zle_hist.c incremental (incl. pattern) search | reedline Search menu | partial (history ListMenu repl.rs:129 — menu browsing, not buffer-driven incremental search; shallow on UX details) | med | bind reedline Search event with SearchHistory corpus to Ctrl+R |
| D05-02 | full-screen history pager UI | reader.rs:710 history_pager (fish 4.0 GUI pager) | — | nu history explorer via menus | missing | med | product-layer pager over LiveFileBackedHistory |
| D05-03 | smartcase / pattern matching in search | history_search.rs:21 smartcase_flags | zle pattern search widgets | reedline SearchQuery options | missing | low-med | pass SearchQuery case-insensitive + regex variants |
| D05-04 | multi-session shared, live-merged history | src/history/ merges all sessions on read (file.rs + history.rs) | SHARE_HISTORY + cross-process fcntl lock (Src/hist.c:2877-2903) | SqliteBackedHistory shared db with session ids (nu-cli/src/repl.rs:37-38) | equivalent-partial (LiveFileBackedHistory modes shared/session/private, history.rs:62-84 + config.rs:62-84; live re-read on search) | low | keep; document semantics |
| D05-05 | per-entry timestamps | src/history/history.rs:163-195 Timestamps + last_added_timestamp :251 | EXTENDED_HISTORY (Src/hist.c readhistfile :2676) | sqlite row timestamps | missing (no timestamp stored — grep shows none in history.rs) | med | extend entry format (versioned) with ts; surface in `history` output |
| D05-06 | cwd annotation + path restore on replay | history.rs:165 required_paths | — | sqlite stores cwd (nu history sqlite schema) | missing | med-low | store cwd alongside entry (fuels `history --cwd` filters) |
| D05-07 | dedup strategies | history/mod.rs:7 dedupe + sort by timestamp | HIST_IGNORE_*_DUPS option family | sqlite PRIMARY KEY dedup | partial (file append model; dedup behavior unverified — shallow) | low | define dedup at load/write |
| D05-08 | history management commands (search/save/show/merge/delete/import/export) | src/builtins/history.rs subcommands | fc/histcb + widgets | nu-cli/src/commands/history/ (history_, import, session) | partial (rubash bash-parity history builtin, src/builtins/history.rs; no search/show subcommands) | low-med | extend builtin with list/search/show on the product layer |
| D05-09 | session identity in entries | HistoryItemId/HistorySessionId in reedline-based fish (src/history/history.rs ids) | — | HistorySessionId in sqlite rows | partial (reedline HistorySessionId used, history.rs:24-30 imports) | — | keep |
| D05-10 | leading-space / exclusion prefix support | fish: space-prefix not recorded | HIST_IGNORE_SPACE | reedline history_exclusion_prefix | equivalent (history_exclusion_prefix wired repl.rs:138) | — | keep |

## Dimension D06 — Menus and pager UX

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D06-01 | columnar completion menu with column memory | src/pager.rs:147-148 ("column memory") | Src/Zle/complist.c list interface | reedline ColumnarMenu | partial (ColumnarMenu with width bounds repl.rs:190-195; no column memory noted) | low | upstream column memory or accept |
| D06-02 | description pane | src/pager.rs descriptions | complist descriptions via zstyle | DescriptionMenu + IdeMenu (reedline 0.52 menu/) | equivalent (DescriptionMode::PreferRight repl.rs:195) | — | keep |
| D06-03 | search-as-you-type inside menu | src/pager.rs:34-35 search_field + editable line | menu-select accepts typed prefix | menus filter on input | partial (reedline menu filtering unverified — shallow) | med | verify; if absent, feed typed chars into completer filter |
| D06-04 | interactive menu-select traversal mode | pager selection + accept | complist.c:157-159 menu-select widget (ZLE_MENUCMP) | reedline menu events | equivalent (reedline menus accept via Enter/Right; shallow) | low | keep |
| D06-05 | three-pane IDE menu | — | — | reedline 0.52 IdeMenu (menu/ide_menu.rs) | missing (reedline 0.50 lacks it) | low | reedline 0.52+ upgrade path |
| D06-06 | bounded pager for large candidate sets | pager.rs:75 max columns logic | complist paging | page_size config | equivalent (page_size + completion_page_size repl.rs:177,193) | — | keep |

## Dimension D07 — Completion system

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D07-01 | candidate descriptions | src/complete.rs:49-66 description strings | _describe + zstyle descriptions | Signature/extra descriptions | equivalent (descriptions flow completer.rs:450-455, 516-529) | — | keep |
| D07-02 | user-authorable per-command completions in shell code | complete -c X -a '...' (src/builtins/complete.rs) + completion functions | compdef functions + compinit autoload (Completion/compinit:72+) | custom completer closures + external_completer (nu-cli/src/completions/custom_completions.rs) | partial (NIU_COMPDEFS cmd->function bridge shell.rs:116-118, load_user_compdefs repl.rs:1189; contract narrower: no per-word-position dispatch options) | med-high | document + extend compdef contract (word/previous-word/leading options) over the existing bridge |
| D07-03 | curated ecosystem corpus | 1095 completion files (share/completions/) | 391 command files (Completion/Unix/Command/) + 53 self (Completion/Zsh/Command/) | signatures built into commands | ahead-ish (winuxcmd_assets.rs + external.rs asset-driven completion data; scale unmeasured — shallow) | low | grow asset corpus with completion data per tool |
| D07-04 | import foreign completion scripts | — | — (native ecosystem) | — | ahead (completion/bash_import.rs imports bash-completion scripts) | — | (ahead) keep investing; it is a moat vs fish/nu |
| D07-05 | --help scraping / dynamic option completion | — (static files) | — | — | ahead-ish (cmd -h scrape + 3-level cache, per docs/planning/niubash-zsh-gap-analysis.md pillar table) | — | (ahead) verify cache invalidation |
| D07-06 | self-completion of the shell's own commands/options | share/completions includes fish's own | 53 files Completion/Zsh/Command | signature-derived automatic | partial (host completion reads builtin_names(), rubash src/executor/builtin_names.rs:101; option-level self-completion shallow) | low-med | generate option tables from builtin help_texts.rs |
| D07-07 | path completions with escaping correctness | src/complete.rs file completion | _files | directory completions (nu-cli/src/completions/directory_completions.rs) | equivalent (CwdPathCandidate completer.rs:304-340; escaped-word test completer.rs:647) | — | keep |
| D07-08 | fuzzy candidate matching | src/complete.rs:33 StringFuzzyMatch | matcher zstyle (compmatch.c) | completion_options fuzzy flag | missing (prefix filter only, completer.rs:481) | med | add subsequence matcher with score, config-gated |
| D07-09 | completion result caching | in-memory per-token cache | per-call | — | ahead (3-level cache per zsh-gap doc) | — | (ahead) keep |
| D07-10 | bash programmable completion builtins | — | — | — | ahead (compgen/complete/compopt port: rubash src/complete/pcomplete.rs, pcomplib.rs, builtins/complete.rs) | — | (ahead) unique compatibility moat |
| D07-11 | completion config surface (case, filtering, ordering) | fish_complete_ envs; pager options | zstyle styles ( dozens of :completion:* keys) | completion_options (nu) | partial (MenuConfig config.rs:154; no case-sensitivity/matcher styles) | med | expose case/fuzzy/sort knobs in FullConfig |

## Dimension D08 — Key binding configuration surface

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D08-01 | bind/bindkey builtin to set keyseq->function at runtime | src/builtins/bind.rs (bind, -K presets, fish_user_key_bindings) | Src/Zle/zle_keymap.c:212+ keymap management | keybindings config table + `keybindings` commands (nu-cli/src/commands/keybindings*.rs) | partial (engine bind is a stub: validates options, prints defaults on -p, rubash src/builtins/bind.rs:24-93; product NIU_BINDKEYS env parse repl.rs:56-69) | high (#185) | implement NIU_BINDKEYS as first-class config + real bind dispatch in product layer; keep engine bind bash-compatible in output only |
| D08-02 | keymap switching / named keymaps | fish_default_key_bindings / fish_vi_key_bindings switch functions | keymaps emacs/vicmd/main aliasing (zle_keymap.c:74-76) | EditMode switch config | partial (EditorMode Emacs/Vi config.rs:44; no runtime switch command) | low-med | `niu mode vi/emacs` verb or Ctrl-bound toggle |
| D08-03 | list/introspect bindings | bind -L/-K/-P | bindkey -l/-L/-D | keybindings_list.rs | partial (bind -p prints the 10 default arrows only, bind.rs:11-22,33-38; no real listing) | low-med | list actual effective keybindings from reedline Keybindings |
| D08-04 | bind arbitrary keyseq strings ("\e[1;5C" style) | yes (key syntax in bind builtin) | yes | yes (reedline KeyEvent tables) | partial (parse_user_bindkeys accepts `keyspec:widget` fixed syntax repl.rs:51-69; no raw keyseq grammar) | med | adopt reedline KeyEvent parser for arbitrary specs |

## Dimension D09 — Scripting language core

Context: the v3 non-goal "no second shell language runtime"
(`docs/planning/niubash-zsh-gap-analysis.md` verdict) means fish-language and
nu-language parity are deliberate non-goals. Rows below record where a
bash-compatible user still loses versus the zsh superset, or where the
references' language models differ structurally.

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D09-01 | POSIX/bash core semantics (functions, loops, case, test, [[ ]], arithmetic, arrays, traps) | different language entirely (no POSIX; functions/args/`set` model) | superset, mostly bash-compatible | different language (structured pipeline) | equivalent (rubash parser/executor suites; 86/86 GNU upstream tests claim per docs/planning/niubash-zsh-gap-analysis.md) | — | keep at parity; this is the product |
| D09-02 | parameter expansion flags (P/j/f/z/S/D/A) | — | Src/subst.c:2163-2440 case arms | — | missing (deliberate; bash ${var...} forms covered instead) | low | none (non-goal) |
| D09-03 | glob qualifiers (*(.) for dirs, e.g. m-7) | — | Src/glob.c:1390+ qualflags dispatch | — | missing (deliberate) | low | none (non-goal); consider `find`/fd wrappers |
| D09-04 | history modifiers :h/:t/:r/:e on params and events | — | Src/subst.c modifiers | — | missing (bash ${var%…}/#… cover most; event-designator modifiers partial) | low | maybe :t/:h as product-layer helper only |
| D09-05 | try/always block | — | Src/parse.c:1610-1625 always block | try/catch with error records (nu-protocol errors) | missing (bash trap ERR partial coverage) | low | non-goal; document trap ERR idiom |
| D09-06 | event designators + fc re-execution | ! events behind feature flag; fc builtin | full | — | equivalent (rubash src/history_expand.rs, src/builtins/fc.rs) | — | keep |
| D09-07 | ${var@…} transforms (Q/A/a/K/k/L/U/P) | — | different mechanism | — | equivalent (rubash src/executor/parameter_transforms.rs:11,204,342 incl. array transforms) | — | keep |
| D09-08 | scoping: local/dynamic scope, export -n, namerefs | set -l/-g/-U scopes | local + typeset + private params (Modules/param_private) | let/mut/const scopes | equivalent (declare_local.rs, local_helpers.rs, arrayref.rs) | — | keep |
| D09-09 | arithmetic language (ternary, bases, assignment ops) | math builtin (separate, with scales/bases) | arithmetic + mathfunc functions in (( )) | typed expressions with units | partial-equivalent (executor/arithmetic; no mathfunc functions — see D11) | low | mathfunc is a winuxcmd/external concern |
| D09-10 | select/case/coproc/process substitution | select-like loops via read; psub for procsub | all present | different model | equivalent (parser/select_command.rs, coproc_command.rs, process_substitution.rs) | — | keep |
| D09-11 | extglob + globstar | different wildcard model (src/wildcard.rs, fuzzy glob) | kshglob + recursive ** | glob command (nu-glob crate) | equivalent (conditional/extglob.rs, parser/extglob_pattern.rs; globstar glob.rs:149-161) | — | keep |
| D09-12 | functions as first-class (function command, autoload) | function builtin + autoload (src/autoload.rs) | functions + autoload -Xz | closures + module exports | partial (bash functions + autoload via FPATH/source; no lazy autoload index) | low | lazy function index when startup cost demands it |

## Dimension D10 — Builtins inventory (what exists vs what we lack entirely)

Ours: 63 builtins — the bash-61 floor plus `setopt`/`typeset`/`unsetopt`
zsh-name aliases, `env`, and Windows `sudo`
(`rubash src/executor/builtin_names.rs:56-97`, doc-synced against
`docs/builtins.md`). fish: 48 builtins (`src/builtins/`). zsh: ~180 built-in
ZLE widgets (`Src/Zle/iwidgets.list`, 203 lines) + core builtins + 31
loadable modules (`Src/Modules/*.mdd`). nushell: ~400+ commands across 19
category dirs (`crates/nu-command/src/`).

| id | capability family | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D10-01 | bash-61 builtin floor | partial (fish has a different set) | superset | — | equivalent (builtin_names.rs:110-168 BASH_61 doc-sync test) | — | keep; this test is the guardrail |
| D10-02 | abbreviation builtin | src/builtins/abbr.rs | — | — | missing | high | D02 |
| D10-03 | string manipulation family (join/split/match/replace/escape/sub/trim/pad/repeat/shorten/collect/length) | src/builtins/string/ (15 files) | via subst flags + external tools | str-* commands (nu-command/src/strings) | missing (winuxcmd sed/awk externals) | med | product-layer `str` builtin delegating to a Rust string engine; beats sed.exe robustness on Windows |
| D10-04 | math command (scales, bases, bit ops, abs/max/min...) | src/builtins/math.rs | Modules/mathfunc.mdd | math-* commands | missing (bash $(( )) + external bc) | med | `math` builtin backed by the existing arithmetic evaluator |
| D10-05 | path manipulation builtin (resolve/basename/dirname/filter/normalize) | src/builtins/path.rs | via :h/:t modifiers | path-* commands | missing (winuxcmd dirname/basename) | med | Windows-first `path` builtin; native paths are a differentiator |
| D10-06 | date/time builtins | via date external | Modules/datetime.c strftime/strconv (:42-99) | date-* commands with typed dates | partial (printf %()T bash-parity; no strftime builtin) | low-med | `date` builtin wrapping chrono |
| D10-07 | random | src/builtins/random.rs | Modules/random.mdd | random commands | missing (external) | low | trivial builtin |
| D10-08 | argparse (option parser for functions) | src/builtins/argparse.rs | zparseopts (Modules/zutil) | signatures + rest params | missing (getopts only, bash parity) | med | needed once function-based completions/widgets grow (D07-02, D01-03) |
| D10-09 | commandline (query/mutate editor buffer from shell code) | src/builtins/commandline.rs | ZLE params zle_params.c:142-173 | — | missing (WidgetOutcome partial, D01-04) | high | same fix as D01-03/04 |
| D10-10 | emit / custom events | src/builtins/emit.rs + event.rs fire | zsh/zselect + zshaddhistory hacks | hooks | missing | low-med | pair with D17-07 |
| D10-11 | status (job summary, features, login, variables) | src/builtins/status.rs:175 print_features | — | version/commands verbs | partial (`niu doctor` + `niu --version` cover setup health, not shell state query) | low | `niu status` verb |
| D10-12 | zpty / job-in-pty control | — | Modules/zpty.mdd | — | missing | low | Windows ConPTY could actually do this well |
| D10-13 | networking builtins (zftp, ztcp/socket, http) | — | Modules/zftp.mdd, tcp.mdd, socket.mdd | http get/post etc. (nu-command/src/network) | missing (WinHttp used internally by self_update only, Cargo.toml windows-sys features) | med | `http` builtin via WinHttp — zero new deps |
| D10-14 | TUI builtins (zcurses, zle, complist) | — | Modules/curses.mdd; Zle modules | nu-explore TUI | partial (interactive_menu.rs crossterm menus, plugins/ui.rs — product verbs, not user-scriptable TUI) | low-med | expose menu toolkit to widgets post-D01-03 |
| D10-15 | misc modules (db_gdbm, pcre, stat, mapfile(zsh), watch, termcap/terminfo, langinfo, prof, clone, cap) | — | Src/Modules/*.mdd (31 modules) | — | missing (deliberate; platform-specific) | low | none; case-by-case if requested |

## Dimension D11 — Structured data / pipeline model (nu-only; paradigm note)

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D11-01 | typed values through the pipeline (tables/records/lists) | — | — | Value enum: Bool/Int/Float/Filesize/Duration/Date/Range/Record/List/Closure/Binary (nu-protocol/src/value/mod.rs:52+, value/duration.rs, filesize.rs) | missing (deliberate: bash text streams) | low (paradigm) | none in core; consider `table`-style pretty-printer wrapper over winuxcmd output |
| D11-02 | cell-path access into structured data | — | — | cell_path_completions.rs + Value record access | missing (deliberate) | low | none |
| D11-03 | interactive data explorer | — | — | crates/nu-explore (src/pager, views) | missing | low-med | `niu explore` over json/csv via asset data |
| D11-04 | dataframe engine | — | — | nu_plugin_polars | missing (deliberate) | low | none |
| D11-05 | database access | — | — | nu-command/src/database | missing | low | none |

## Dimension D12 — Jobs and process control

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D12-01 | jobs/fg/bg/disown/wait core | src/builtins/jobs.rs, fg.rs, disown.rs, wait.rs | Src/jobs.c | job list/send/recv/unfreeze/freeze (nu-command/src/experimental/job_*.rs) | equivalent (rubash src/jobs/ table.rs+signals.rs; builtins jobs.rs, fg_bg.rs, disown.rs, wait.rs) | — | keep |
| D12-02 | job lifecycle notifications | job_exit/process_exit events (src/event.rs:231-242) | NOTIFY option; job notify messages | — | partial (BackgroundJobs prompt segment count only, prompt_segments.rs:24; no async notify) | med-low | fire postcmd-style hook or toast on job completion |
| D12-03 | terminal ownership/grouping | JobGroup with wants_terminal (src/job_group.rs:49-94) | MONITOR + tcsetpgrp dance | — | partial (Windows has no pgrp semantics; ConPTY constraints documented in docs/windows-signal-compatibility.md) | — | platform noise; document |
| D12-04 | signal handling + trap | src/signal.rs + traps | full POSIX | kill + limited traps | partial (trap.rs DEBUG/ERR/RETURN wired, trap_exec.rs:675-758; Windows signal delivery limits known) | med | keep closing Windows signal gaps (existing docs) |
| D12-05 | wait -n (next job) | wait builtin | wait -n | job recv | partial (wait.rs — -n support unverified, shallow) | low | verify + implement |
| D12-06 | process substitution | psub temp-file approach (share/functions/psub.fish) | =(...) form | different (nu closures over externals) | equivalent (parser/process_substitution.rs, real fd-based) | — | keep |

## Dimension D13 — Prompt and theme system

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D13-01 | prompt as user code | fish_prompt function (share/functions/fish_prompt.fish) | PROMPT + PROMPT_SUBST (promptexpand Src/prompt.c:182, % sequences :809) | PROMPT_COMMAND closure (nu-cli/src/prompt.rs) | partial (template + segment catalog config.rs:12-29; Custom(String) segment is static text, prompt_segments.rs:24-35) | med-high | let a segment be a niu function name evaluated per draw (needs cheap function-call path) |
| D13-02 | right prompt | fish_right_prompt function | RPROMPT/RPS1 | PROMPT_COMMAND_RIGHT (reedline_config.rs) | partial (right_prompt_format template + right_prompt_elements, config.rs:18,26; static) | med | same function-segment mechanism as D13-01 |
| D13-03 | async prompt execution | prompt runs on background threads (src/threads.rs, src/reader/iothreads.rs) | — (sync; themes hack via precmd caching) | — (sync) | missing (segments computed synchronously in the draw path) | med-high | thread pool + timeout + cached segment values; prerequisite for a git segment |
| D13-04 | vi-mode prompt indicator | fish_mode_prompt function | — | render_prompt_indicator(PromptEditMode) | partial (repl.rs:674 ignores the mode parameter — `_prompt_mode`; one static indicator) | med | render per-mode chars for Insert/Normal/Replace |
| D13-05 | git/VCS segment | fish_git_prompt/fish_vcs_prompt (share/functions/fish_git_prompt.fish:9+) | Functions/VCS_Info (24 files: VCS_INFO_adjust, bydir_detect, formats, hook, get_cmd...) | nu_plugin_gstat plugin | missing (no git SegmentId, prompt_segments.rs:24-35) | high | add Git segment once D13-03 lands (git status --porcelain=v2 -b, cached) |
| D13-06 | p10k-style presets | — (external themes) | — (external themes) | — (external) | ahead (presets lean/classic/rainbow/pure/robbyrussell, prompt_segments.rs:80+; "Built-in segment prompt presets mirroring p10k themes") | — | (ahead) keep |
| D13-07 | execution-time segment | CMD_DURATION var feeding themes | themes compute manually | — | ahead (CommandExecutionTime segment built-in, prompt_segments.rs:24) | — | (ahead) keep |
| D13-08 | prompt colors/themes config | fish_color_* universal vars | %F/%K escapes + theme scripts | color_config | partial (segment fg/bg per preset, prompt_segments.rs:112-157; no user color table) | low-med | extend FullConfig with per-segment color overrides |
| D13-09 | terminal title | fish_title function | precmd sets title via %? | shell_integration config (nu-protocol/src/config/shell_integration.rs) | partial (title hook in HookConfig config.rs:32-39) | — | keep |

## Dimension D14 — Plugin / extension ecosystem

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D14-01 | built-in manager + catalog | — (fisher/omf are third-party) | — (oh-my-zsh/zinit third-party) | plugin add/use + registry file (nu-cmd-plugin/src/util.rs:43-63) | ahead (catalog.rs + recipes.rs + ui.rs + trust.rs + sync.rs + mirrors.rs — curated manager with TUI) | — | (ahead) keep |
| D14-02 | third-party binary plugin protocol | — | — | nu-plugin-protocol/nu-plugin-engine crates; plugin signatures feed completions/docs | missing (only the command-not-found provider ABI draft, docs/planning/plugin-command-not-found-provider-abi.md:1-10) | med-high | generalize the provider ABI to {command, completions, prompt, hooks} contracts over JSON frames |
| D14-03 | run foreign ecosystem plugins | n/a | n/a | n/a | partial (bash compat means most bash plugins source OK; `bind` calls tolerated by the stub — bind.rs:40-47 comment says exactly this) | med | fix the missing 20% (D01-03/D08-01) and the oh-my-* compat story mostly closes itself |
| D14-04 | plugin provides completions/prompt/hooks | functions+events | any sourced script | signatures + plugin commands | partial (NIU_COMPDEFS + HookConfig + widget functions are separate surfaces, not unified under one pack format) | med | unify into a pack manifest (plugins/spec.rs exists — extend) |
| D14-05 | trust/verification model | — | — | — (unsigned registry) | ahead-ish (plugins/trust.rs + registry-derived trust bound per wt87/gatecal) | — | (ahead) keep |

## Dimension D15 — Variables, scopes, types

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D15-01 | arrays/lists with element semantics | real lists (no word splitting) | arrays + associative + tied | typed lists | equivalent-bash (executor/arrays/storage.rs, dynamic_arrays.rs; shell/arrays/) | — | keep |
| D15-02 | persistent cross-session variables (universal vars) | env_universal_common.rs:301-318 binary v3.0 format, :644 SETUVAR records | — | — (config only) | missing | med-low | niu config env table applied at startup (simpler than uvar file) |
| D15-03 | typed variable attributes | — (all strings) | typeset -E/-F/-L/-R/-Z/-U/-T/-H (Src/builtin.c:5380+ case arms) | typed Values | partial (bash declare -i/-x/-r/-a/-A/-n; declare.rs) | low | non-goal beyond bash |
| D15-04 | PATH as a list (auto split/join) | PATH is a list | typeset -T path PATH tie | PATH is a list | missing (colon-string, bash semantics) | low-med | product helper (`path add`) before any engine change |
| D15-05 | pipestatus arrays | $pipestatus | pipestatus | exit codes in sqlite history | equivalent ($PIPESTATUS bash parity, executor) | — | keep |
| D15-06 | special params completeness (@ * # ? - $ ! 0) | different set ($status etc.) | superset | — | equivalent (parameter_core.rs) | — | keep |
| D15-07 | private/module-scoped params | function-local scope | Modules/param_private (private params) | module exports + hide | partial (bash local + function_env.rs) | low | non-goal |

## Dimension D16 — Event hooks

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D16-01 | precmd/preexec | fish_preexec/fish_postexec events (src/reader/reader.rs:872-888) | callhookfunc preexec (Src/init.c:192-213) | pre_prompt/pre_execution (nu-protocol/src/config/hooks.rs:8-9) | equivalent (HookConfig precmd/preexec config.rs:32-39) | — | keep |
| D16-02 | postcmd (after each command) | fish_postexec event | — core | — | ahead (postcmd hook config.rs:35) | — | (ahead) keep |
| D16-03 | chpwd (directory change) | --on-variable PWD convention | chpwd hook (zsh core) | env_change on PWD (hooks.rs:10) | equivalent (chpwd hook config.rs:34) | — | keep |
| D16-04 | zshaddhistory / zshexit | fish_postexec covers history-ish | zshaddhistory/zshexit hooks | — | ahead (zshaddhistory + zshexit hooks config.rs:36-38) | — | (ahead) keep |
| D16-05 | variable watchers | --on-variable events (src/event.rs:217-224 EventDescription::Variable) | — | env_change per-variable hooks (hooks.rs:10) | missing (no variable hooks in HookConfig) | med | add on_variable hook list; watch a fixed set first (PWD, VIRTUAL_ENV, PATH) |
| D16-06 | job/process exit hooks | process_exit/job_exit events (event.rs:231-242) | — | — | missing | med-low | pair with D12-02 |
| D16-07 | custom named events + emit builtin | emit builtin + fire_generic (reader.rs:888) | — | — | missing | low-med | D10-10 |
| D16-08 | periodic timer hook | — | callhookfunc "periodic" + PERIOD (Src/utils.c:1530-1585) | — | missing | low | trivial against precmd path + monotonic clock |
| D16-09 | command-not-found handler | command_not_found_handler function | command_not_found_handler | error hooks | partial (host builtin diagnostics + provider ABI runtime per docs/planning/plugin-command-not-found-provider-abi.md:5-10) | med | ship the provider contract (same lane as D14-02) |
| D16-10 | greeting hook | fish_greeting function | — | banner in config | ahead (greeting hook config.rs:37) | — | (ahead) keep |

## Dimension D17 — Startup and configuration surface

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D17-01 | rc file chain | config.fish + conf.d/*.fish | zshenv/zprofile/zshrc/zlogin (global + user) | env.nu + config.nu | partial (NIU_ENV file with BASH_ENV fallback, shell.rs:904-917 + tests :3370-3434; single file by design) | low-med | optional conf.d-style drop-in dir alongside niu.env |
| D17-02 | per-feature feature flags | status features + fish_features (src/builtins/status.rs:175) | 219 setopt names (Src/options.c optns :79) | experimental flags | partial (bash set/shopt + setopt alias, zsh_options.rs; no niu-specific feature flags) | low | niu feature flags only when a behavioral fork needs one |
| D17-03 | compiled/cached startup artifacts | — | zcompile wordcode (Src/builtin.c:136) | plugin cache msgpackz + const-eval of config | missing (the 1.3.4 "6s source" fix addresses a symptom; no cache) | med | cache parsed config + completion assets with mtime validation |
| D17-04 | lazy autoload of functions | src/autoload.rs (functions load on first call) | zmodload on demand; autoload -Xz | — | partial (source-time function defs; no lazy index) | low | lazy function index if startup regressions demand |
| D17-05 | startup timing visibility | --profile (fish) | zprof module | nu --log-level timing | ahead (startup_trace.rs dedicated module) | — | (ahead) keep |
| D17-06 | instant prompt | — | p10k instant prompt (plugin) | — | missing | low | far-term; needs D13-03 first |
| D17-07 | first-run onboarding + health check | — | — | — | ahead (setup_wizard.rs 3209 lines + doctor.rs probes with fix hints) | — | (ahead) keep; nothing in fish/zsh/nu has an equivalent |
| D17-08 | agent/AI env file contract | — | — | — | ahead (NIU_ENV sourcing incl. agent-init file, shell.rs:3352-3461 tests) | — | (ahead) unique AI-native surface |

## Dimension D18 — Errors and diagnostics

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D18-01 | span-annotated, labeled errors | plain text with caret for parse errors | text | miette-backed labeled errors (nu-protocol/src/errors/{labeled_error,report_error}.rs) | partial (bash-format text diagnostics, src/posix_errors.rs; stderr ordering caveat per rubash AGENTS.md) | low-med | keep bash text parity (compat contract); add rich rendering only behind a niu-specific flag |
| D18-02 | did-you-mean suggestions | set.rs:950 "Did you mean `set %s %s`?" | spell correction hooks for commands | did-you-mean in parse errors | missing | med | Levenshtein over builtin_names() + PATH heads for command-not-found and `set`-style usage errors |
| D18-03 | cross-shell migration hints ("Unsupported use of '=', use 'set'") | src/parse_constants.rs:539 | — | — | n/a (we are the bash side of that hint; potentially emit zsh-to-bash hints for zsh refugees — opportunity, not gap) | low | optional migration hints for zsh syntax (`$(...)` differences, `[[` vs `(`) |
| D18-04 | warnings channel distinct from errors | — | warn_* | shell_warning.rs | missing | low | pair with D18-01 work |
| D18-05 | chained error causes | — | — | chained_error.rs | missing | low | non-goal for bash parity |
| D18-06 | installation health diagnostics | — | — | — | ahead (doctor.rs: winuxcmd discovery, command links, nerd font probes with fix hints) | — | (ahead) keep |

## Dimension D19 — Performance-relevant architecture

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D19-01 | async/threads for prompt+IO | src/threads.rs + src/reader/iothreads.rs | single-threaded ZLE | engine is single-threaded per eval; plugins are separate processes | missing (synchronous draw path; see D13-03) | med-high | same fix as D13-03 |
| D19-02 | incremental lexer/parse reuse | fish AST reparse + caching (src/ast.rs) | ZLE in C, no reparse of history | IR compile of blocks (nu-engine) | partial (lexer brace_scan_cache.rs caches brace scans; no whole-line incremental parse) | low | extend cache family only on profiler evidence |
| D19-03 | persistent helper processes | — | — | plugins are persistent processes with GC (nu-protocol/src/config/plugin_gc.rs) | missing | low-med | not until D14-02 protocol exists |
| D19-04 | precompiled wordcode/bytecode | — | zcompile | nu-engine IR | missing | low | non-goal while cold start is fast |
| D19-05 | cold-start time on Windows | n/a (no Windows build) | n/a (MSYS layer) | native | ahead (170 ms cold start claim, docs/planning/niubash-zsh-gap-analysis.md pillar table; rubash docs/PERF-BASELINE.md tracks engine perf) | — | (ahead) keep measuring per release |
| D19-06 | history search performance structures | history cache nodes + timestamp merge (src/history/history.rs:144-149) | in-memory HISTSIZE list | sqlite indexes | partial (prefix search over loaded entries; perf unmeasured — shallow) | low | measure before optimizing |

## Dimension D20 — Platform and OS integration

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D20-01 | native Windows without WSL | missing (Unix targets) | missing (MSYS2/Cygwin layer) | equivalent (native) | ahead (ConPTY-native, ConPTY test harness per rubash Cargo.toml:16-19 comment) | — | (ahead) this is the moat |
| D20-02 | terminal profile installation | — | — | — | ahead (windows_terminal.rs:6 NIU_PROFILE_GUID profile install) | — | (ahead) keep |
| D20-03 | font management (nerd fonts) | — | — | — | ahead (fonts.rs) | — | (ahead) keep |
| D20-04 | self-update channel | — | — | nupm/nu — not shell-update | ahead (src/self_update.rs; WinHttp dependency in Cargo.toml) | — | (ahead) keep |
| D20-05 | elevation builtin | — | — | — | ahead (Windows sudo builtin, builtin_names.rs windows-conditional + winuxcmd sudo) | — | (ahead) keep |
| D20-06 | GNU coreutils injection | — | — | — | ahead (winuxcmd.rs:1-8 PATH injection of GNU-compatible toolchain) | — | (ahead) keep |
| D20-07 | LSP server | — | — | nu-lsp crate | missing | low-med | strategic: our completion data (D07-03/05) could back a bash LSP |
| D20-08 | MCP server surface | — | — | nu-mcp crate | missing | med (strategic) | natural fit for AI-native positioning; expose execution + completion over MCP |
| D20-09 | i18n message translation | po/ + src/localization/gettext.rs | — | — | partial (locale.rs + zh-CN default docs; no translated message catalog) | low | extract user-facing strings when market demands |

## Dimension D21 — Testing and self-quality infrastructure

| id | capability | fish | zsh | nushell | ours | impact | fix path |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D21-01 | PTY-driven interactive tests | tests/pexpects/*.py (pexpect byte-stream asserts; per docs/interactive-test-methodology.md) | C test suite (util + zle tests) | nu has command-level tests | equivalent (tests/interactive.rs portable-pty driver; journey gates scripts/journey/golden-journey.py) | — | keep |
| D21-02 | upstream conformance corpus | own spec | own spec | own spec | ahead (GNU bash upstream test suites + COMPATIBILITY-STATUS.md ledger — the only shell here whose spec is literally GNU bash) | — | (ahead) keep |
| D21-03 | editor behavior references study | docs/interactive-test-methodology.md documents fish two-tier approach (pexpect + tmux screens) | — | reedline snapshot testing | partial (methodology doc adopted wt74; adoption of fish two-tier testing not complete) | low | port pexpect-style byte-stream asserts for abbr/widgets work |

## Lane-splitting proposals

Each dimension or tight cluster below is sized for one lane, matching the
audit-plan lane discipline (discovery/fix lanes are separate):

- **L1 (editor primitives, P0):** D01-03/04 + D08-01/03/04 + D10-09 — the
  widget contract + real bind surface. Unlocks L2/L3. Files:
  `crates/niubash-runtime/src/{repl.rs,shell.rs,config.rs}`.
- **L2 (abbr):** D02 — depends on L1 only for the function-abbr variant; the
  literal-table MVP can start immediately in repl.rs. Files: repl.rs,
  config.rs, new builtins/abbr surface in product layer.
- **L3 (prompt):** D13-01/02/03/04/05 + D19-01 — async segment runner, then
  Git segment, then function segments + mode indicators. Files:
  prompt_segments.rs, new prompt_providers.rs, repl.rs hook-in.
- **L4 (transient prompt):** D01-12 — tiny, independent: wire
  `with_transient_prompt` in repl.rs. Good first lane.
- **L5 (history):** D05-01/05/06/08 + D04-01/02 — timestamps + cwd + search
  UX + fuzzy. Files: history.rs, autosuggest.rs, repl.rs.
- **L6 (completion):** D07-02/07-08/07-11 + D06-03 — compdef contract
  broadening, fuzzy matcher, config knobs. Files: completion/*, config.rs.
- **L7 (hooks/events):** D16-05/06/07 + D12-02 — variable watchers, exit
  events, notify. Files: config.rs HookConfig, shell.rs, repl.rs.
- **L8 (command families):** D10-03/04/05/06/07 — str/math/path/date/random
  builtins. Rubash engine + winuxcmd decision per builtin.
- **L9 (plugin protocol):** D14-02 + D16-09 — generalize provider ABI.
  Files: plugins/spec.rs, plugins/recipes.rs, new provider runtime.
- **L10 (strategic surfaces):** D20-08 (MCP) + D20-07 (LSP) — one lane, one
  spike each, before any commitment.

## Honest shallow notes

- **Depth vs breadth:** every cell was verified to the cited line, but not
  every subsystem was read end-to-end. Rows marked *shallow* (D01-06 undo
  depth, D01-08 bracketed paste, D04-03 word-wise accept binding, D05-01
  incremental-search UX details, D05-07 dedup behavior, D06-03 menu typed
  filtering, D07-03 asset-corpus scale, D07-06 option-level self-completion,
  D12-05 wait -n, D19-06 history search perf) need a focused pass before
  they drive roadmap decisions.
- **zsh snapshot provenance:** master snapshot (5.9.999.3-test), not the 5.9
  release; one non-upstream changelog entry present; restricted shell was
  removed upstream in that window. No feature evidence in this matrix relies
  on those deltas.
- **fish snapshot** is a between-releases master (4.9.3 dev); abbr semantics
  cited from src are stable across 3.x/4.x but line numbers will drift.
- **reedline version skew:** the matrix records reedline 0.52 snapshot
  capabilities; niubash builds against 0.50. Rows that say "reedline ships
  it" still require wiring work on our side (e.g. D01-12 transient prompt
  exists in 0.50 already and is simply unwired).
- **No live differential probing was used for this matrix** (the original
  abbr-lane plan had ConPTY transcripts as acceptance criteria). fish 3.7.0
  and zsh 5.9 are installed in WSL on this machine and pywinpty is
  available; the L2 abbr lane should capture the fish transcripts
  (abbr add -> type gp+SPACE -> inline expansion -> history stores expanded
  form) as its acceptance spec before implementation.
