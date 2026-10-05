# Eco-harvest resume + gold triage — wt97b/hres (2026-10-05)

Lane wt97b/harvest-resume. Baseline: niubash master 04d7b7f, rubash master
8ff0d051, WSL GNU Bash 5.3.0. Evidence root: `D:/eco-harvest/`
(results, state, logs, scratch drivers); repro files under
`E:/eco-harvest/tmp/`.

## 1. Stall diagnosis

The pipeline stalled 2026-10-04T19:43:52Z (state stale ~398 min). The last
`rate-budget.jsonl` entries show **remaining 3714/5000 core** — NOT rate-limit
exhaustion. No process survived; the tester (separate process) kept writing
results until 20:38Z. Diagnosis: the harvester ran attached to the previous
agent's session and died with it (console teardown). Code review confirmed
three genuine overnight-survival gaps that would have killed the run later
anyway:

1. `Github.get()` counted rate-limit sleeps against the 4-attempt retry
   budget; a primary window >47 min away (core windows run a full hour) or a
   sustained secondary limit raised after ~4 sleeps and killed the run.
2. `t3_t5_run`'s search-collection loop had no try/except — one failed query
   (e.g. `IncompleteRead`, seen live in this resume) aborted the whole tier.
3. No tier-level supervisor: any escaped exception aborted the remaining
   tiers and the census write.

## 2. Script fixes (this branch)

`scripts/harvest/eco-harvest.py`:

- Rate-limit responses (403/429 with exhausted window / "rate limit" body)
  now sleep until reset **without consuming retries** (primary: until the
  reset header, per-sleep cap 3500s; secondary: bounded exponential backoff,
  gives up only after 40 consecutive waits). Spec'd "sleep until reset" is
  now literally true.
- t3/t5 search-collection loop: per-query try/except, continues with repos
  gathered so far (proved live: `topic:bash-script` IncompleteRead absorbed).
- Tier supervisor: each tier retried up to 3x (60s/120s backoff), failures
  logged with traceback and skipped, census still written.
- t4 code-search: skip paths with non-printable characters (18 permanent
  FETCH-FAILEDs had accumulated; raw URLs can never fetch them).
- Enumeration heartbeat: `save_state` every 25 repos so state-file age stays
  a meaningful liveness signal.

`scripts/harvest/eco-test.py`:

- GNU runtime oracle gate: rc != 0 with **any** error-pattern line is a GNU
  failure (was `>= STORM_LINES` = 100 lines, which stamped "ok" on rc=2 with
  1-2 syntax-error lines and manufactured false gold).
- SYNTAX-REJECT ladder: when GNU -n ALSO rejects AND niu runtime sources
  clean, stamp OK (`ok:nn-reject-both-runtimes-clean`) instead of a false
  SYNTAX-REJECT-RUBASH-ONLY (the extglob-before-shopt class: `bash -n` never
  executes the `shopt -s extglob`, both engines' -n reject, both runtimes OK).
- Permanent `UNFETCHABLE-URL` verdict (non-transient) for control-character
  raw URLs instead of burning retries every resume.
- FETCH_RETRIES 5→6, 403 added to the retryable CDN set, longer backoff; and
  sqlite timeout=30 so harvest+test can run concurrently.

## 3. Resume status

- Harvester relaunched detached (PowerShell `Start-Process`, survives this
  session): first pass COMPLETED cleanly (wall 894s, 3980 calls, 75s of
  secondary-limit waits absorbed, IncompleteReads logged per-repo and
  skipped); second pass running (PID 31860) from the persisted cursors.
  Counters at last check: repos t3 132→797+, t5 30→721+, assets
  2597→6941+; state age 0.1 min.
- Tester relaunched detached with the fresh-master niu build and the fixed
  oracle (PID 2372): ~0.4 assets/s, ETA ~3 h for the then-pending 4803.
- FETCH-FAILED retry (mission item): 318 assets failed a fetch at least
  once; **216 converted to real verdicts** (199 OK, 10 SLOW, 4 HANG,
  3 GNU-ALSO-FAILS); 102 still failing on CDN bursts (retried every pass);
  the 18 control-character URLs will be badged UNFETCHABLE-URL permanently.

## 4. Gold triage (verdict-audit table)

Gold = SYNTAX-REJECT-RUBASH-ONLY + HANG, last-wins over
`results/test-results.jsonl`. Every SYNTAX-REJECT row was re-run through the
4-way ladder (GNU -n / rubash -n / GNU runtime / niu runtime) on fresh
master; every HANG group got a ConPTY repro attempt + GNU parity.

| group (assets) | fresh-master result | classification |
| --- | --- | --- |
| vscode.theme.sh HANG (1) | still wedges: `syntax error near '&&'` in PS1 backtick-comsub; GNU renders the theme fully | **known open #425** — fresh-master evidence commented |
| bash-completion extglob `-n` rejects, 19 files (`@(...)`, `?(-)`, `!(-*)` in completions-core) | GNU -n rejects the same lines; GNU runtime rc=2 on these files too (no in-file shopt); niu runtime rc=2 parity | GNU-ALSO-FAILS (old-oracle artifact; re-stamped). The `bash -n`-vs-runtime strictness class is documented, not a divergence |
| oh-my-bash lib/cli.bash (1) | GNU -n rejects, GNU runtime rc=0, niu runtime rc=0 | OK — both engines behave identically at both stages (false gold from -n pre-filter; tester now stamps OK) |
| ohmyzsh tools (changelog.sh, check_for_upgrade.sh), oh-my-zsh.sh, nobility web-recon.sh, cmake-bats, shai pipeline_essay.sh, completions/README.md | GNU -n rc=2 AND GNU runtime rc=2; niu runtime rc=2 parity | GNU-ALSO-FAILS (zsh content / non-shell file) |
| **ble.sh 6 files (core-syntax, init-term, keymap.vi, canvas, color, edit)** | GNU -n **accepts all 6** (rc=0), GNU runtime rc=0 on canvas/color/edit; rubash -n rejects all 6; niu runtime rc=2 there | **NEW engine bug — unixwin/rubash#435** (multi-line `(( ))` + `(...)`-grouped assign + comma-newline) |
| **nox.bash (argcomplete loader) HANG (1)** | niu -i source wedges deterministically (echo-only session); rubash -n rc=0; niu runtime rc=0; GNU sources fine | **NEW engine bug — unixwin/rubash#436** (eval + nested comsub with empty-output pipeline wedges interactive execution) |
| stdin-readers: bats-format-cat, lintorama-stop.sh, statusline.sh, bash-cat-with-cat cat.sh, doitlive walkthrough.sh, awg_common.sh (5+1) | hang in `cat`/`python` REPL/`bc` reading the ConPTY (never EOF); GNU non-interactive rc=0 with stdin=/dev/null; GNU interactive blocks identically | harness class: interactive-stdin, not an engine bug |
| menu/wizard loops: share_menu.sh (`translate` cnf loop), rename_user.sh, setup.sh, simple-interest.sh x2, gen-man-html.sh | upstream scripts missing tools or looping on `read`; GNU runtime rc=1/127 | upstream content, not an engine bug |

Bottom line: of the "45 gold candidates" in the mission briefing (actual
file state at resume: 16; the running tester surfaced more), every row is
now accounted for: 2 confirmed engine bugs — both NEW (#435, #436; #425
re-confirmed still open on fresh master) — and the rest reclassified to
GNU-ALSO-FAILS, OK, or the interactive-stdin harness class with evidence.

## 5. Issues

- **#435** [P1][parse] multi-line arithmetic command + grouped assign +
  comma-newline — GNU accepts, breaks 6 ble.sh files. GNU anchors:
  `parse.y:4904 parse_dparen()`, `parse.y:4963 parse_arith_cmd()` (scan to
  matching `))`, newlines are body characters), `parse.y:1203 arith_command`.
- **#436** [P0][interactive] eval + nested multi-line comsub with
  empty-output pipeline wedges the session. GNU anchor:
  `subst.c:7143 command_substitute()` → `parse_and_execute`; `eval -- ""`
  is a no-op. Cross-ref #425 layer-②.
- **#425 comment**: fresh-master (04d7b7f) still-repro evidence + layer-②
  independence note.
