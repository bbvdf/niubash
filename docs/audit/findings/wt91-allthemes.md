# wt91/allthemes — ALL-THEMES INTERACTIVE HANG SWEEP (full matrix, ConPTY, hard timeouts)

- **Lane:** wt91/allthemes (branch `wt91/allthemes`, base `b4b87f1`, niu 1.3.4 release built fresh in the worktree)
- **Mandate:** the owner's 2026-10-05 escalation — interactive testing had covered 3/166 themes while bash-it's codeword/gitline hang the live session (niu#182). Scope upgraded mid-lane: **full matrix, all cells, no sampling** — every theme × HISTCONTROL {unset, autosave, auto, noauto} × HISTFILE scale {0, 10, 2000, 20000} × {fresh, aged} + re-source cells + controls, every cell with hard timeouts and a verdict.
- **Harness (committed this lane):** `scripts/perf/theme-interactive-sweep.py` — parameterized (source roots, framework, timeouts, matrix subsets), machine-readable (cells.jsonl + summary JSON), resume-capable, `--gate` exit semantics ready to become the third release gate (release.yml wire-up deliberately NOT in this lane).
- **Raw artifacts:** `E:/niubash-sweep/full-1/` — `cells.final.jsonl` (8064 records, one per cell, evidence tails for every non-OK cell), `cells.jsonl.pass1` (pre-adjudication snapshot), `run.log` (full progress log), `summary.json` (= committed JSON), plus probe scripts `probe3.py`/`probe4.py`/`probe3.rc`/`probe4.rc` backing the engine findings below. Keep on E: (repo drive near-full).

---

## 1. Coverage and headline numbers

**Every theme from BOTH fixtures, no sampling:**

| Fixture | Root | Themes |
| --- | --- | --- |
| oh-my-bash | `D:/repo/rubash/target/ecosweep-corpus/oh-my-bash-lf` | 83 (82 `<name>.theme.sh` + `random` `.theme.bash`) |
| bash-it | `D:/repo/rubash/target/bit-real` (full clone incl. base.theme.bash, githelpers, p4helpers, per-theme `*.base.bash`) | 84 |
| **Total** | | **167** |

**Matrix per theme:** 4 HISTCONTROL × 4 HISTFILE scales × {fresh, aged} = 32 interactive cells + 16 re-source cells (`. <theme file>` typed into the live session, one per HISTCONTROL × HISTFILE scale) = **48 cells/theme → 8016**, plus 48 control cells (bare niu, omb-no-theme, bash-it-no-theme over the same grid) = **8064 cells, all run, all with a verdict**. Fresh and aged share one sandbox home (the aged session is a REAL second session on the history/state a previous session left). Every bash-it cell loads through the NATIVE loader (`source $BASH_IT/bash_it.sh`, empty `enabled/`, per-worker writable clone) — exactly the real-user path, including `lib/history.bash`'s per-prompt `_bash-it-history-auto-load` hook.

**Steps per session (each with its own hard bound, default 10s):** boot (rc bootstrap sentinels) → first prompt → type `echo T1` → ENTER → type `echo T2` → ENTER → (re-source cells: `. <theme>` + ENTER + one more round trip) → `exit`. Key echoes and prompt advances are verified by three witnesses (raw sentinel, ANSI-stripped output, prompt-row movement) because niu's readline re-echoes with per-keystroke repaints and some themes/frameworks rebuild PROMPT_COMMAND and drop the harness sentinel.

**Totals: 8064/8064 cells completed with verdicts — no crashes anywhere (CRASH=0).**

| Verdict | Cells | Themes involved |
| --- | --- | --- |
| OK-SLOW | 7552 | 149 themes all-cells-clean + the good cells of the 18 below |
| ERROR-STORM | 400 | 15 themes (see F1, F6, F10) |
| HANG | 96 | exactly 2 themes × all 48 cells each: `omb/iterate`, `bashit/iterate` (see F2) |
| RENDER-BROKEN | 16 | `bashit/inretio` — all 48 of its cells OK otherwise (see F9) |
| OK-FAST | 0 | — the 2s OK-SLOW bar sits below the measured **transport floor** (controls, below); this is expected on this machine and is not theme cost |
| CRASH | 0 | no theme killed niu — no P0 crash note required |

**Controls (what every cell pays before any theme runs):** bare `niu -i` boot ≈ 3170ms, `+bash_it.sh` ≈ 3590ms, `+oh-my-bash.sh` ≈ 5160ms — flat across all HISTCONTROL × HISTSCALE cells. The universal OK-SLOW verdicts are therefore transport+loader floor, not theme cost; the comparative signal lives in the per-step latencies of `t1/t2` (type ≈ 12-20ms, ENTER ≈ 660-940ms median everywhere, flat across histcontrol and histscale — see `summary.json`).

**Cell-conditionality analysis (the coordinator's "which verdicts CHANGED between cells"):** after adjudication (every HANG cell re-run under the final detection harness), **no theme's verdict is HISTCONTROL- or HISTFILE-conditional**. The remaining shape-conditional verdicts are pure arithmetic: a re-source session walks 5 prompt cycles vs 3 for fresh/aged, so a theme emitting ≥2 error lines per prompt crosses the 10-line ERROR-STORM threshold in resrc cells only (16-cell storms: gitline, powerline, powerline-naked, redline, lambda, roderik, sirup, modern-t, hawaii50, omb/developer). `bad_only_in_*` fields are per theme in the committed JSON.

---

## 2. niu#182 baseline (the mandate's anchor)

**On this base (b4b87f1, 1.3.4), the #182 hard hang does NOT reproduce — for any theme.** codeword, gitline and the whole `_save-and-reload-history` family (codeword, doubletime×3, nwinkler×2, pete, rainbowbrite hardcoded `1`; powerline family via `${HISTORY_AUTOSAVE:-0}`) complete every step in every one of their 48 cells; worst steps are boot-driven (~3.6s), first_prompt ~0.7s, ENTER ~0.7s.

The reason the old trigger is inert on 1.3.4 is engine-side and is itself findings F3+F5 below: the per-prompt `history -a && history -c && history -r` chain never does meaningful work in this binary. wt90's hang repro must be re-baselined against the current binary before chasing the loop — the mechanism it was chasing has changed shape.

---

## 3. Findings — engine (fix lanes own these)

### F1 (P1, engine) — a variable assigned via `${VAR:=default}` never word-splits again

**Minimal repro (script mode, no interactivity needed — `/tmp/repro2.sh`, also probe12):**

```bash
: "${V:="a b c"}"
for x in $V; do echo "[$x]"; done
: "${W:="p q"}"
for x in ${W-"z"}; do echo "[$x]"; done
```

- GNU bash 5.2 (Git Bash `bash.exe`), same file: `[a]` `[b]` `[c]` then `[p]` `[q]`
- niu 1.3.4 (`target/release/niu.exe`), same file: `[a b c]` then `[p q]`

A variable set by direct assignment (`V="a b c"`) splits normally in every context tested (interactive rc, script, `-c`); the SAME name assigned through a `${VAR:=word}` parameter-expansion assignment is afterwards expanded **as one word** in unquoted context. This is a quoting-state leak in the expansion layer (the carrier/CTLESC class this codebase flags as highest-risk), not a general splitting gap.

**Blast radius (measured, whole class):** bash-it's powerline family builds its segment loop on such a variable — `gitline.theme.bash:69` `: "${POWERLINE_PROMPT:="user_info scm python_venv ruby node cwd"}"` feeding `powerline.base.bash:315` `for segment in ${POWERLINE_PROMPT-"user_info" "scm" ...}`. Under niu the loop runs ONCE with `segment="user_info scm python_venv ruby node cwd"`, producing exactly the observed per-prompt error `niu: __powerline_user_info scm python_venv ruby node cwd_prompt: command not found`, and every segment after the first is lost from the prompt. Same shape in powerline, powerline-multiline, powerline-naked, redline (their own base copies), and atomic's `___atomic_prompt_*` entries.

**Fix direction owner:** wherever a `${VAR:=word}` expansion assigns, the resulting variable's quoting state must be identical to a plain assignment (GNU `subst.c` expansion discipline; `${VAR:=word}` is just an assignment with side effect — the assigned value carries no quotation). Verification bar: the minimal repro byte-for-byte + the 9 affected themes' ERROR-STORM cells dropping to 0 errors per prompt + no regression in the full matrix (re-run `--frameworks bashit --gate`).

### F2 (P1, engine) — nested interactive `niu -i` under ConPTY ignores stdin redirection/EOF: the only true hangs in the whole matrix

`omb/iterate` and `bashit/iterate` hang at **first_prompt in 48/48 cells each — unconditional across every HISTCONTROL, HISTFILE scale and session shape.** Both files carry the same per-prompt pattern (omb `themes/iterate/iterate.theme.sh`, bash-it `themes/iterate/iterate.theme.bash:49`):

```bash
new_prompt=$(PS1="$new_PS1" "$BASH" --norc -i </dev/null 2>&1 | sed -n '...')
```

niu sets `BASH=` to itself, so every prompt spawns a nested interactive niu with stdin redirected from the null device. In GNU bash (verified: `bash --norc -i </dev/null` exits immediately, exit 0) the nested shell sees EOF and exits; under niu **in a ConPTY session the nested interactive reader never observes the redirect/EOF and blocks forever**, so PROMPT_COMMAND never returns and no prompt renders. In piped mode the nested niu exits fine — the failure is ConPTY-specific, same modality as the original #182 report. Harness evidence: `cells.final.jsonl` keys `*/iterate/*/hc=*` — stall_step `first_prompt`, session alive, zero further output after the rc bootstrap sentinel.

**Classification: nested-interactive-shell family (the #182 hang shape, but NOT the history mechanism).** Fix direction owner: the interactive input loop must honor stdin redirection (read from fd 0 as redirected, exit on EOF) — `parse.y`/`execute_cmd.c` redir discipline for the interactive reader; wt90's ConPTY harness is the natural owner.

### F3 (P1, engine) — env-imported HISTFILE is broken under ConPTY (and silently disarms the #182 history path)

Sandbox probes (scripts in `E:/niubash-sweep/full-1/`):

1. With `HISTFILE` coming from the **environment**: rc-time `history -a` **silently aborts the rest of the rc** (no error, no rc=`echo` afterwards) under ConPTY; the same rc piped returns 0 and continues. At PROMPT_COMMAND time `history -a` errors `niu: history: filename not specified` even though `printf '%q' "${HISTFILE}"` in the same shell prints the correct path — the builtin cannot see a variable that printf can.
2. With `HISTFILE` set **inside the rc** (shell assignment, the way bash-it docs tell users to configure it): everything works — `history -a` rc=0 at load and prompt time, `history -c && history -r` succeeds against a 2000-entry file.

The sweep seeds HISTFILE rc-side for exactly this reason (documented in the harness docstring): with env-HISTFILE the niu#182 per-prompt reload chain short-circuits at its first `history -a` and the hang mechanism is unreachable — an earlier harness draft proved this by NOT reproducing the family's pathology until the rc-set form was adopted.

**Fix direction owner:** the engine must import HISTFILE from the environment into the shell variable table at startup exactly like GNU bash, and the history builtin must resolve it consistently in interactive-ConPTY, piped-interactive and script modes.

### F4 (P2, engine) — a failing PROMPT_COMMAND element aborts the whole array; GNU runs the remaining elements

Repro (rcfile, interactive, both shells): `PROMPT_COMMAND=('printf "ELEM1\n"; echo ${prompt_color?}' 'printf "ELEM2\n"')` →

- GNU bash: `ELEM1`, `bash: prompt_color: parameter not set`, **`ELEM2`**
- niu 1.3.4: `ELEM1`, `niu: prompt_color: parameter not set`, **no ELEM2**

This is what turned three themes (emperor `${prompt_color?}`, gallifrey `${bold?}`, modern `${python_venv?}` — variables that are undefined in upstream bash-it itself) into pass-1 HANG false-positives: the theme's prompt_command dies mid-chain and takes the harness sentinel (and any later hook a user appended) with it. After the harness learned to accept prompt-row evidence, all three adjudicate OK-\* with the per-prompt `parameter not set` lines in `error_samples`. Engine fix: element isolation per PROMPT_COMMAND array element (GNU `eval`-per-element semantics). Note the theme-side half (undefined color vars) is upstream bash-it's own bug — see F11.

### F5 (P2, engine) — `history -a` never persists anything: zero growth across all 8064 cells

Every cell records `histfile_lines_before`/`histfile_lines_after`. Across every framework, theme, HISTCONTROL and scale — including cells where `history -a` returns 0 with an rc-set, pre-seeded HISTFILE and the session ran `echo T1`, `echo T2`, `exit` — the file never grows (uniform 2000→2000 etc.). bash-it's history autosave (the `_bash-it-history-auto-save` hook) is therefore a no-op for real users on 1.3.4: sessions never share history. Piped probe confirms `history -a` reports success while writing nothing.

### F6 (P3, engine) — `hash` builtin writes an error to a closed stderr: `hash: write error: Bad file descriptor`

`bashit/rjorgenson` storms in all 48 cells with `└─niu: hash: write error: Bad file descriptor➞` painted into the prompt: the theme runs `hash todo.sh 2>&-` per prompt (`rjorgenson.theme.bash:55`). Repro (`/tmp/hash.sh`): `hash todo.sh 2>&-` → GNU bash: silent, rc=1; niu: `hash: write error: Bad file descriptor` printed, rc=1. A builtin must not diagnose its own attempt to report on a deliberately closed fd (GNU's `hash` tolerates EBADF silently). Cosmetic severity, but it lands inside PS1-adjacent output on every prompt.

### F8 (P2, parser) — omb/vscode: theme file fails to parse at load (`syntax error near unexpected token '&&'`)

Every vscode cell shows `niu: line 17: syntax error near unexpected token '&&'` at framework load; the theme then degrades to a minimal prompt. The file builds PS1 from a backtick template full of `&&` chains (vscode-dev-container-derived). GNU bash parses the same file fine (upstream theme). The byte-identical error text also appears when the file is re-sourced. Parser-side repro: `niu -c` sourcing the theme; fix lane owns the exact construct (backtick command substitution containing `&&` inside a double-quoted assignment).

### F9 (P2, engine) — PS1 non-printing markers `\[` / `\]` render literally

`bashit/inretio` = the only RENDER-BROKEN theme (16/48 cells over the escape-fragment threshold, all cells affected): the rendered prompt shows literal `[32;1m✓[0m`. The bash-it `lib/colors.bash` variables themselves contain `\[`/`\]` (`purple="\[\e[0;35m\]"`), GNU readline treats them as non-printing markers in PS1 and strips them; niu's PS1 renderer passes them through as text. Most bash-it themes leak ≤4 fragments per screen (under the harness threshold); inretio's layout crosses it. Engine direction: honor (strip) `\[`/`\]` when rendering PS1.

### F10 (env-bound, not engine) — themes shelling out to Unix-only externals per prompt

`omb/developer` → `top` (per-prompt CPU load), `bashit/duru` → `rev` ×2 per prompt, `bashit/hawaii50` → `ips`, `bashit/modern-t` → `t`. These fail on ANY Windows bash (no such binaries); they are environment noise in the ledger, not niu bugs. AGENTS.md discipline: do not fix by patching niu.

### F11 (upstream theme bugs, not engine) — bash-it themes referencing undefined variables under `${var?}`

emperor (`${prompt_color?}`), gallifrey (`${bold?}`), modern (`${python_venv?}`) reference variables that no bash-it file defines (verified by tree-wide grep of the pinned clone) — upstream aborts the same way under GNU bash (message text differs). Combined with F4 the abort additionally kills every later PROMPT_COMMAND element. Candidates for an upstream bash-it patch, not a niu fix.

### F12 (flaky-by-design) — `omb/random`

The random theme sources a RANDOM member theme at load; its cells inherit that theme's verdict (hung in the cells where it picked iterate). Its 2 pass-1 hang cells and 1 resrc hang resolve accordingly. Not a product bug; noted so nobody chases non-reproducibility.

### Boot-stall flakes (measurement noise, no finding)

Pass 1 recorded 7 boot-step HANGs (powerline-icon ×1, powerline-light ×2, bashit/agnoster ×4) at `--workers 8`. Re-run at workers=6 under the final harness: all 48+48+48 cells of those themes are OK-SLOW — the bound was breached by machine load (8 concurrent ConPTY boots + 20k-line histfile copies), not by the product. Gate guidance: run the gate at workers ≤6, or raise only the boot-step bound.

### False-hang post-mortem (why pass 1 over-reported, for the record)

Pass 1's 204 HANG cells reduced to 96 true ones. Three detection gaps were found and fixed IN the harness this lane (echo witness needed ANSI-strip + whitespace-collapse; PROMPT_COMMAND rebuilders drop the sentinel — bash-it `safe_append_prompt_command` rebuilds the array; a failing PROMPT_COMMAND element aborts the chain — F4). The final dataset re-ran every HANG and every resrc cell under the fixed detection; the surviving 96 are real, unconditional, and mechanism-explained (F2).

---

## 4. The #182-relevant history matrix — what the cells say

* All 8064 cells: **no hang is HISTCONTROL-conditional and none is HISTFILE-scale-conditional** (per-theme `bad_only_in_histcontrol/histscale` are empty in the committed JSON; the only conditional dimension is shape, and that is the resrc prompt-cycle arithmetic in §1).
* The per-prompt history chain (`history -a && history -c && history -r` under `*auto`) is reachable ONLY with rc-set HISTFILE (F3) and, when reachable, costs ~0.7s per ENTER at hs=2000 with **no hang** and **no persistence** (F5).
* ENTER-step latency is flat across histcontrol and histscale for both frameworks (medians 863-940ms across all 16 control cells and theme cells; the cost is the engine's own per-prompt prompt rebuild + git, not history).

**Conclusion for #182:** on 1.3.4 the codeword/gitline hang is gone as a side effect of F3/F5 disarming the history path. Fixing F3/F5 (correct behavior!) may RE-OPEN the original #182 hang — wt90 must re-run its ConPTY repro after those two land, with this harness (`--gate --frameworks bashit`) as the regression net.

---

## 5. Full verdict table (167 themes × 48 cells)

Worst-ok-ms = the slowest completed step across the theme's cells (boot-dominated; the no-theme floors are ~3.17s bare / ~3.59s bash-it loader / ~5.03s omb loader). Family = static per-prompt pattern classification from the theme source.

| Theme | Cells | Verdicts | worst-ok-ms | Family |
| --- | --- | --- | --- | --- |
| omb/vscode | 48 | OK-SLOW:48 | 10010.5 | heavy-subshell |
| omb/axin | 48 | OK-SLOW:48 | 10002.8 | heavy-subshell |
| omb/powerline-light | 48 | OK-SLOW:48 | 8721.9 | clock-redraw |
| omb/clean | 48 | OK-SLOW:48 | 6984.5 | none |
| omb/cooperkid | 48 | OK-SLOW:48 | 6944.2 | none |
| omb/cupcake | 48 | OK-SLOW:48 | 6562.6 | none |
| bashit/essential | 48 | OK-SLOW:48 | 6440.9 | none |
| omb/agnoster | 48 | OK-SLOW:48 | 6118.3 | clock-redraw |
| omb/binaryanomaly | 48 | OK-SLOW:48 | 6105.3 | heavy-subshell |
| omb/mairan | 48 | OK-SLOW:48 | 5940.9 | heavy-subshell |
| bashit/liquidprompt | 48 | OK-SLOW:48 | 5871.2 | clock-redraw |
| omb/random | 48 | OK-SLOW:48 | 5830.4 | none |
| omb/demula | 48 | OK-SLOW:48 | 5782.4 | heavy-subshell |
| omb/copied-duru | 48 | OK-SLOW:48 | 5767.1 | clock-redraw |
| omb/bobby-python | 48 | OK-SLOW:48 | 5644.4 | heavy-subshell |
| omb/brainy | 48 | OK-SLOW:48 | 5578.9 | clock-redraw |
| omb/dulcie | 48 | OK-SLOW:48 | 5570.3 | heavy-subshell |
| omb/sexy | 48 | OK-SLOW:48 | 5537.0 | heavy-subshell |
| omb/bobby | 48 | OK-SLOW:48 | 5515.0 | clock-redraw |
| omb/ht | 48 | OK-SLOW:48 | 5492.1 | none |
| omb/powerline-icon | 48 | OK-SLOW:48 | 5458.3 | clock-redraw |
| omb/bakke | 48 | OK-SLOW:48 | 5429.3 | heavy-subshell |
| omb/dos | 48 | OK-SLOW:48 | 5428.5 | none |
| omb/doubletime | 48 | OK-SLOW:48 | 5423.2 | history-reload (niu#182 family) |
| omb/luan | 48 | OK-SLOW:48 | 5417.1 | clock-redraw |
| omb/garo | 48 | OK-SLOW:48 | 5405.4 | history-reload (niu#182 family) |
| omb/developer | 48 | ERROR-STORM:16 OK-SLOW:32 | 5403.7 | clock-redraw |
| omb/brunton | 48 | OK-SLOW:48 | 5392.3 | clock-redraw |
| omb/doubletime_multiline_pyonly | 48 | OK-SLOW:48 | 5388.0 | history-reload (niu#182 family) |
| omb/robbyrussell | 48 | OK-SLOW:48 | 5380.0 | none |
| omb/90210 | 48 | OK-SLOW:48 | 5359.1 | none |
| omb/nwinkler_random_colors | 48 | OK-SLOW:48 | 5354.0 | history-reload (niu#182 family) |
| omb/powerline-multiline | 48 | OK-SLOW:48 | 5349.4 | clock-redraw |
| omb/powerline | 48 | OK-SLOW:48 | 5335.9 | clock-redraw |
| omb/morris | 48 | OK-SLOW:48 | 5333.6 | none |
| omb/roderik | 48 | OK-SLOW:48 | 5333.2 | none |
| omb/absimple | 48 | OK-SLOW:48 | 5333.1 | clock-redraw |
| omb/modern | 48 | OK-SLOW:48 | 5332.4 | heavy-subshell |
| omb/hawaii50 | 48 | OK-SLOW:48 | 5328.8 | network-touching |
| omb/kitsune | 48 | OK-SLOW:48 | 5327.6 | none |
| omb/nekonight_moon | 48 | OK-SLOW:48 | 5327.2 | none |
| omb/rana | 48 | OK-SLOW:48 | 5325.8 | heavy-subshell |
| omb/powerline-naked | 48 | OK-SLOW:48 | 5324.5 | clock-redraw |
| omb/nekonight | 48 | OK-SLOW:48 | 5323.2 | none |
| omb/emperor | 48 | OK-SLOW:48 | 5317.5 | clock-redraw |
| omb/pete | 48 | OK-SLOW:48 | 5312.0 | history-reload (niu#182 family) |
| omb/pro | 48 | OK-SLOW:48 | 5307.6 | none |
| omb/candy | 48 | OK-SLOW:48 | 5303.8 | clock-redraw |
| omb/nwinkler | 48 | OK-SLOW:48 | 5302.2 | none |
| omb/lucky | 48 | OK-SLOW:48 | 5300.8 | none |
| omb/tylenol | 48 | OK-SLOW:48 | 5298.9 | none |
| omb/font | 48 | OK-SLOW:48 | 5291.0 | history-reload (niu#182 family) |
| omb/powerline-plain | 48 | OK-SLOW:48 | 5290.1 | clock-redraw |
| omb/rr | 48 | OK-SLOW:48 | 5285.8 | none |
| omb/half-life | 48 | OK-SLOW:48 | 5280.2 | none |
| omb/primer | 48 | OK-SLOW:48 | 5277.1 | clock-redraw |
| omb/edsonarios | 48 | OK-SLOW:48 | 5276.9 | none |
| omb/rainbowbrite | 48 | OK-SLOW:48 | 5273.4 | history-reload (niu#182 family) |
| omb/doubletime_multiline | 48 | OK-SLOW:48 | 5269.2 | history-reload (niu#182 family) |
| omb/lambda | 48 | OK-SLOW:48 | 5268.3 | none |
| omb/minimal-gh | 48 | OK-SLOW:48 | 5266.3 | clock-redraw |
| omb/powerbash10k | 48 | OK-SLOW:48 | 5265.7 | clock-redraw |
| omb/powerline-wizard | 48 | OK-SLOW:48 | 5263.2 | clock-redraw |
| omb/mbriggs | 48 | OK-SLOW:48 | 5262.2 | none |
| omb/purity | 48 | OK-SLOW:48 | 5260.8 | none |
| omb/modern-t | 48 | OK-SLOW:48 | 5259.6 | heavy-subshell |
| omb/gallifrey | 48 | OK-SLOW:48 | 5259.1 | heavy-subshell |
| omb/zork | 48 | OK-SLOW:48 | 5257.4 | heavy-subshell |
| omb/sirup | 48 | OK-SLOW:48 | 5255.0 | none |
| omb/envy | 48 | OK-SLOW:48 | 5250.2 | none |
| omb/simple | 48 | OK-SLOW:48 | 5248.7 | none |
| omb/nekolight | 48 | OK-SLOW:48 | 5247.9 | none |
| omb/slick | 48 | OK-SLOW:48 | 5247.0 | heavy-subshell |
| omb/n0qorg | 48 | OK-SLOW:48 | 5246.4 | none |
| omb/tonka | 48 | OK-SLOW:48 | 5245.0 | clock-redraw |
| omb/zitron | 48 | OK-SLOW:48 | 5242.7 | none |
| omb/pzq | 48 | OK-SLOW:48 | 5242.3 | clock-redraw |
| omb/rjorgenson | 48 | OK-SLOW:48 | 5241.0 | heavy-subshell |
| omb/wanelo | 48 | OK-SLOW:48 | 5239.9 | none |
| omb/tonotdo | 48 | OK-SLOW:48 | 5236.9 | none |
| omb/minimal | 48 | OK-SLOW:48 | 5236.2 | none |
| omb/pure | 48 | OK-SLOW:48 | 5232.1 | heavy-subshell |
| omb/standard | 48 | OK-SLOW:48 | 5229.7 | none |
| bashit/envy | 48 | OK-SLOW:48 | 4684.1 | none |
| bashit/sexy | 48 | OK-SLOW:48 | 4299.2 | heavy-subshell |
| bashit/mairan | 48 | OK-SLOW:48 | 4288.9 | heavy-subshell |
| bashit/axin | 48 | OK-SLOW:48 | 4287.8 | heavy-subshell |
| bashit/modern | 48 | OK-SLOW:48 | 4118.8 | none |
| bashit/binaryanomaly | 48 | OK-SLOW:48 | 4088.5 | heavy-subshell |
| bashit/kitsune | 48 | OK-SLOW:48 | 3962.1 | heavy-subshell |
| bashit/luan | 48 | OK-SLOW:48 | 3948.4 | clock-redraw |
| bashit/tokyonight | 48 | OK-SLOW:48 | 3881.7 | heavy-subshell |
| bashit/nwinkler | 48 | OK-SLOW:48 | 3822.3 | history-reload (niu#182 family) |
| bashit/morris | 48 | OK-SLOW:48 | 3819.5 | none |
| bashit/lambda | 48 | ERROR-STORM:16 OK-SLOW:32 | 3802.4 | clock-redraw |
| bashit/barbuk | 48 | OK-SLOW:48 | 3796.2 | network-touching |
| bashit/robbyrussell | 48 | OK-SLOW:48 | 3782.6 | none |
| bashit/candy | 48 | OK-SLOW:48 | 3782.2 | clock-redraw |
| bashit/parrot | 48 | OK-SLOW:48 | 3761.7 | heavy-subshell |
| bashit/90210 | 48 | OK-SLOW:48 | 3748.5 | none |
| bashit/powerline-plain | 48 | OK-SLOW:48 | 3748.0 | history-reload (niu#182 family) |
| bashit/brainy | 48 | OK-SLOW:48 | 3722.9 | clock-redraw |
| bashit/cooperkid | 48 | OK-SLOW:48 | 3718.0 | heavy-subshell |
| bashit/gallifrey | 48 | OK-SLOW:48 | 3712.8 | none |
| bashit/agnoster | 48 | OK-SLOW:48 | 3710.3 | clock-redraw |
| bashit/pro | 48 | OK-SLOW:48 | 3708.6 | none |
| bashit/brunton | 48 | OK-SLOW:48 | 3707.9 | clock-redraw |
| bashit/inretio | 48 | OK-SLOW:32 RENDER-BROKEN:16 | 3706.4 | clock-redraw |
| bashit/bira | 48 | OK-SLOW:48 | 3706.2 | none |
| bashit/slick | 48 | OK-SLOW:48 | 3704.0 | heavy-subshell |
| bashit/emperor | 48 | OK-SLOW:48 | 3701.9 | clock-redraw |
| bashit/mbriggs | 48 | OK-SLOW:48 | 3698.8 | none |
| bashit/gitline | 48 | ERROR-STORM:16 OK-SLOW:32 | 3694.0 | history-reload (niu#182 family) |
| bashit/radek | 48 | OK-SLOW:48 | 3694.0 | none |
| bashit/powerline | 48 | ERROR-STORM:16 OK-SLOW:32 | 3693.6 | history-reload (niu#182 family) |
| bashit/font | 48 | OK-SLOW:48 | 3692.9 | history-reload (niu#182 family) |
| bashit/tonka | 48 | OK-SLOW:48 | 3692.8 | clock-redraw |
| bashit/nwinkler_random_colors | 48 | OK-SLOW:48 | 3692.1 | history-reload (niu#182 family) |
| bashit/powerline-naked | 48 | ERROR-STORM:16 OK-SLOW:32 | 3691.7 | history-reload (niu#182 family) |
| bashit/primer | 48 | OK-SLOW:48 | 3691.1 | clock-redraw |
| bashit/bobby | 48 | OK-SLOW:48 | 3689.6 | clock-redraw |
| bashit/zork | 48 | OK-SLOW:48 | 3689.1 | heavy-subshell |
| bashit/rana | 48 | OK-SLOW:48 | 3688.7 | heavy-subshell |
| bashit/redline | 48 | ERROR-STORM:16 OK-SLOW:32 | 3688.6 | clock-redraw |
| bashit/dulcie | 48 | OK-SLOW:48 | 3688.5 | heavy-subshell |
| bashit/elixr | 48 | OK-SLOW:48 | 3688.5 | none |
| bashit/ramses | 48 | OK-SLOW:48 | 3687.2 | heavy-subshell |
| bashit/doubletime | 48 | OK-SLOW:48 | 3685.4 | history-reload (niu#182 family) |
| bashit/metal | 48 | OK-SLOW:48 | 3685.2 | heavy-subshell |
| bashit/easy | 48 | OK-SLOW:48 | 3683.1 | none |
| bashit/bobby-python | 48 | OK-SLOW:48 | 3681.7 | none |
| bashit/demula | 48 | OK-SLOW:48 | 3679.2 | heavy-subshell |
| bashit/newin | 48 | OK-SLOW:48 | 3679.2 | none |
| bashit/hawaii50 | 48 | ERROR-STORM:16 OK-SLOW:32 | 3678.7 | network-touching |
| bashit/powerturk | 48 | OK-SLOW:48 | 3678.3 | heavy-subshell |
| bashit/norbu | 48 | OK-SLOW:48 | 3678.0 | none |
| bashit/purity | 48 | OK-SLOW:48 | 3677.2 | none |
| bashit/tonotdo | 48 | OK-SLOW:48 | 3676.7 | none |
| bashit/wanelo | 48 | OK-SLOW:48 | 3676.6 | none |
| bashit/clean | 48 | OK-SLOW:48 | 3676.1 | none |
| bashit/modern-t | 48 | ERROR-STORM:16 OK-SLOW:32 | 3675.7 | heavy-subshell |
| bashit/minimal | 48 | OK-SLOW:48 | 3675.1 | none |
| bashit/modern-time | 48 | OK-SLOW:48 | 3674.9 | clock-redraw |
| bashit/rainbowbrite | 48 | OK-SLOW:48 | 3674.7 | history-reload (niu#182 family) |
| bashit/simple | 48 | OK-SLOW:48 | 3673.5 | none |
| bashit/cupcake | 48 | OK-SLOW:48 | 3673.1 | heavy-subshell |
| bashit/doubletime_multiline_pyonly | 48 | OK-SLOW:48 | 3672.7 | history-reload (niu#182 family) |
| bashit/oh-my-posh | 48 | OK-SLOW:48 | 3671.4 | none |
| bashit/sirup | 48 | ERROR-STORM:16 OK-SLOW:32 | 3671.2 | none |
| bashit/codeword | 48 | OK-SLOW:48 | 3670.5 | history-reload (niu#182 family) |
| bashit/doubletime_multiline | 48 | OK-SLOW:48 | 3670.1 | history-reload (niu#182 family) |
| bashit/pete | 48 | OK-SLOW:48 | 3670.1 | history-reload (niu#182 family) |
| bashit/pure | 48 | OK-SLOW:48 | 3668.6 | none |
| bashit/dos | 48 | OK-SLOW:48 | 3668.4 | none |
| bashit/bakke | 48 | OK-SLOW:48 | 3667.5 | heavy-subshell |
| bashit/standard | 48 | OK-SLOW:48 | 3667.1 | none |
| bashit/tylenol | 48 | OK-SLOW:48 | 3666.7 | none |
| bashit/n0qorg | 48 | OK-SLOW:48 | 3666.2 | none |
| bashit/zitron | 48 | OK-SLOW:48 | 3663.1 | none |
| bashit/roderik | 48 | ERROR-STORM:16 OK-SLOW:32 | 3661.4 | none |
| bashit/atomic | 48 | ERROR-STORM:48 | None | clock-redraw |
| bashit/duru | 48 | ERROR-STORM:48 | None | none |
| bashit/powerline-multiline | 48 | ERROR-STORM:48 | None | history-reload (niu#182 family) |
| bashit/rjorgenson | 48 | ERROR-STORM:48 | None | heavy-subshell |
| omb/duru | 48 | ERROR-STORM:48 | None | none |
| bashit/iterate | 48 | HANG:48 | None | heavy-subshell |
| omb/iterate | 48 | HANG:48 | None | heavy-subshell |

---

## 6. Issue drafts (new buckets, ready to file)

### Draft A — `[P1][compat] ${VAR:=default}`-assigned variables never word-split afterwards (powerline family storms)
- Repro: the 6-line script in F1 (byte-exact GNU vs niu outputs quoted there).
- Observed: 15-theme ERROR-STORM bucket, ~400 storm cells; every powerline-family theme prints `niu: __powerline_<seg> scm python_venv ruby node cwd_prompt: command not found` once per prompt and renders only one segment.
- Classification: engine expansion/quoting-state (F1). Labels: `bug`, `compat-gap` semantics (repo label set per AGENTS.md).
- Verification bar: F1's; plus re-run of this sweep (`--gate --frameworks bashit --workers 6`).

### Draft B — `[P1][hang] nested interactive niu under ConPTY ignores stdin redirect/EOF` (omb+bashit iterate, 96 cells)
- Repro: enable `iterate`, open `niu -i` on a real terminal; OR minimal: from a ConPTY niu session run `$BASH --norc -i </dev/null`.
- Observed: first_prompt stall, session alive, zero output; 48/48 cells per framework; piped mode exits cleanly (modality difference).
- Classification: nested-interactive-shell family (F2). This is the only true interactive hang class in the entire 167-theme corpus; same ConPTY modality as the original #182 report.
- Fix owner: wt90/themehang (ConPTY repro + reader/EOF discipline).

### Draft C — `[P1][compat] env-imported HISTFILE unusable under ConPTY; history builtin inconsistent across modes` (F3)
- Repro: probes in `E:/niubash-sweep/full-1/probe{3,4}.rc` (+ the rc-abort variant); symptom line `niu: history: filename not specified` while `$HISTFILE` prints set.
- Consequences: bash-it autosave silently dead; rc execution silently truncated when HISTFILE is env-set (boot-time `history -a` aborts the rest of the rc).
- Related: F5 (`history -a` never persists — zero growth in 8064 cells).

### Draft D — `[P2][compat] PROMPT_COMMAND array: failing element aborts remaining elements` (F4) — with the GNU-side cite and the emperor repro.

### Draft E — `[P3][compat] hash builtin: write error on closed stderr` (F6, rjorgenson).

### Draft F — `[P2][parser] backtick-PS1 theme file fails: syntax error near unexpected token '&&'` (F8, omb/vscode) + `[P2][render] PS1 \[ \] non-printing markers leak as literal text` (F9, inretio and all bash-it themes using colors.bash vars).

---

## 7. Harness notes (for the release-gate wire-up)

* `scripts/perf/theme-interactive-sweep.py --gate` exits 1 iff any theme cell is HANG/CRASH/ERROR-STORM/RENDER-BROKEN. On THIS base the gate fails on: HANG 96 cells (F2), ERROR-STORM 400 cells (F1/F6/F10), RENDER-BROKEN 16 cells (F9). After F1+F2 land, expected gate-remaining: storms from F6/F10 (rjorgenson, duru×2, developer, hawaii50, modern-t) and F9's inretio — i.e. the gate becomes green only when engine AND upstream-theme noise are separated (configurable `--error-storm-threshold`, or a theme-side known-noise list; decision left to the wire-up lane).
* Resumability and crash-hygiene are built in (JSONL resume, taskkill tree-kill on every path, strays verified 0 at lane end). Note for CI: this lane's background runs were externally killed at ~55-60 min three times; the resume path recovered cleanly each time — schedule the gate run under 55 min or rely on resume.
* Static theme-feature inventory (no-PROMPT_COMMAND themes like omb/random, per-prompt subshell counts, network touches, CJK, minified files, history-reload callers) is embedded per theme in the committed JSON (`themes[].features`) — the coordinator's edge-case enumeration.
