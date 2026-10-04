#!/usr/bin/env bash
# wt82-L01 / audit domain D06: formalized verb-surface sweep.
#
# Promotes the wt74/audit orchestrator's worked example
# (target/audit-results/verb-sweep/sweep.sh, untracked) into a tracked,
# repeatable probe. Every launcher word and every plugin verb is exercised
# with --help / bare / missing-arg / unknown-flag forms in a sandboxed HOME,
# each with a bounded timeout; the plugin usage screen is diffed against the
# dispatch table (extracted live from src/main.rs when a repo checkout is
# available); the docs verb tables are diffed the same way; and the V1
# stale-PATH bootstrap-noise regression runs last.
#
# Usage:
#   scripts/audit/l01-verb-sweep/sweep.sh [path/to/niu.exe] [repo-root]
# Env:
#   STALE_NIU  path to an old-vintage niu.exe for the V1 regression probe
#              (default: the known 1.2.5 install if present, else a cmd.exe
#              copy named niu.exe — any binary that answers to `niu` but
#              predates `plugin sync` reproduces the class)
#   NET=0      skip the network-bound probe (--self-update --check)
#
# Artifacts: per-probe stdout/stderr under
# target/audit-results/wt82-L01/sweep-<stamp>/ (relative to the repo the
# binary lives in, else the working directory).
set -u

NIU="${1:-}"
REPO="${2:-}"
if [ -z "$NIU" ]; then
    echo "usage: sweep.sh <path-to-niu.exe> [repo-root]" >&2
    exit 2
fi
NIU=$(cd "$(dirname "$NIU")" && pwd)/$(basename "$NIU")
if [ -z "$REPO" ]; then
    d=$(cd "$(dirname "$NIU")/../.." 2>/dev/null && pwd)
    [ -f "$d/src/main.rs" ] && REPO="$d"
fi

BASE="${NIU%/target/release/niu.exe}"
OUT="$BASE/target/audit-results/wt82-L01/sweep-$(date +%Y%m%d-%H%M%S)"
[ "$BASE" != "$NIU" ] || OUT="./l01-sweep-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"
SB="$OUT/sandbox"

rm -rf "$SB"
mkdir -p "$SB/home" "$SB/fakepath"
export HOME="$SB/home" USERPROFILE="$SB/home" NIU_LANG=en
export MSYS_NO_PATHCONV=1

PASS=0
FAIL=0
FAILED=""
NET="${NET:-1}"

probe() { # id expect_rc(0|N|nz) needle(- = none) timeout cmd...
    local id="$1" exprc="$2" needle="$3" tmo="$4"
    shift 4
    local o="$OUT/$id.out" e="$OUT/$id.err" rc bad=""
    timeout "$tmo" "$NIU" "$@" >"$o" 2>"$e" </dev/null
    rc=$?
    case "$exprc" in
        0) [ "$rc" -eq 0 ] || bad="rc=$rc want 0" ;;
        nz) [ "$rc" -ne 0 ] || bad="rc=0 want nonzero" ;;
        *) [ "$rc" -eq "$exprc" ] || bad="rc=$rc want $exprc" ;;
    esac
    if [ -z "$bad" ] && [ "$needle" != "-" ]; then
        command grep -q -- "$needle" "$o" "$e" || bad="missing needle: $needle"
    fi
    if command grep -qiE "panicked at|stack backtrace|RUST_BACKTRACE" "$o" "$e"; then
        bad="$bad; PANIC in output"
    fi
    if [ -z "$bad" ]; then
        PASS=$((PASS + 1))
        echo "PASS $id"
    else
        FAIL=$((FAIL + 1))
        FAILED="$FAILED $id"
        echo "FAIL $id :: $bad"
    fi
}

ok_if() { # id 0=want-pass cmd...   (assertion embedded in the command)
    local id="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        PASS=$((PASS + 1))
        echo "PASS $id"
    else
        FAIL=$((FAIL + 1))
        FAILED="$FAILED $id"
        echo "FAIL $id"
    fi
}

# ── Phase A: launcher words ────────────────────────────────────────────────
probe help-long 0 "Usage:  niu" 20 --help
probe help-short 0 "Usage:  niu" 20 -h
probe version-long 0 "Niubash " 20 --version
probe version-short 0 "Niubash " 20 -V
probe dashc 0 "hello-c" 20 -c 'echo hello-c'
probe dashC-ok 0 "hello-C" 20 -C 'echo hello-C'
probe dashC-noarg nz "requires an argument" 20 -C
probe completion-probe 0 "-" 20 --completion-probe "niu plugin "
probe completion-probe-noarg nz "requires an input line" 20 --completion-probe
probe completion-probe-badcur nz "invalid cursor position" 20 --completion-probe x zz
probe wt-profile-badopt nz "" 20 --install-wt-profile --bogus-flag
probe selfupdate-badopt nz "" 20 --self-update --bogus-flag
probe engine-badopt 2 "invalid option" 20 --definitely-not-an-option
probe engine-norc-c 0 "hello-norc" 20 --norc -c 'echo hello-norc'
probe engine-i-c 0 "hello-ic" 20 -i -c 'echo hello-ic'
probe script-missing 127 "No such file or directory" 20 no-such-script-xyz
probe setup-preset-unknown nz "unknown preset" 30 setup --preset no-such-preset
probe doctor 0 "niubash doctor" 30 doctor
probe font 0 "" 30 font
# internal-yes streams forever; cap the capture (head closes the pipe so the
# 8s probe stays a few KB, not 600MB of `yes` output). Accepted exits: 0
# (closed-pipe policy, niubash#140 — same as `--version | head`) or 124
# (timeout kill on the still-streaming writer).
o="$OUT/internal-yes.out" e="$OUT/internal-yes.err"
timeout 8 "$NIU" --internal-yes 2>"$e" | head -c 1000 >"$o"
rc=${PIPESTATUS[0]}
if { [ "$rc" -eq 0 ] || [ "$rc" -eq 124 ]; } && command grep -q "^y$" "$o"; then
    PASS=$((PASS + 1))
    echo "PASS internal-yes"
else
    FAIL=$((FAIL + 1))
    FAILED="$FAILED internal-yes"
    echo "FAIL internal-yes :: rc=$rc, expected 0 (closed-pipe) or 124 (timeout) + y-lines"
fi
if [ "$NET" = 1 ]; then
    # V5 (D11 class): the reason after "Windows error <code>: " must never be
    # empty. Exit code is network truth (either way is acceptable here).
    o="$OUT/v5-selfupdate-check.out" e="$OUT/v5-selfupdate-check.err"
    timeout 60 "$NIU" --self-update --check >"$o" 2>"$e" </dev/null
    if command grep -qE "Windows error [0-9]+: *$" "$o" "$e"; then
        FAIL=$((FAIL + 1))
        FAILED="$FAILED v5-empty-reason"
        echo "FAIL v5-empty-reason :: empty WinHttp reason after the colon"
    else
        PASS=$((PASS + 1))
        echo "PASS v5-empty-reason"
    fi
fi

# stdin-script mode
o="$OUT/stdin-script.out" e="$OUT/stdin-script.err"
echo 'echo hello-stdin' | timeout 20 "$NIU" >"$o" 2>"$e"
ok_if stdin-script command grep -q "hello-stdin" "$o"

# ── Phase B: plugin verbs × bare / unknown-flag / noarg ────────────────────
probe plugin-usage 0 "Usage:  niu plugin" 20 plugin
probe plugin-help 0 "Usage:  niu plugin" 20 plugin --help
probe plugin-unknown nz "unknown plugin subcommand" 20 plugin definitely-bogus
for v in discover list sync update restore clean; do
    probe "plugin-$v" 0 "-" 30 plugin "$v"
done
# ui degrades without a tty: rc 1 + the CLI verb list (wt74 worked example)
probe plugin-ui-nontty nz "needs an interactive terminal" 30 plugin ui
probe plugin-add-noarg nz "" 20 plugin add
probe plugin-enable-noarg nz "" 20 plugin enable
probe plugin-disable-noarg nz "" 20 plugin disable
probe plugin-trust-noarg nz "" 20 plugin trust
probe plugin-rollback-noarg nz "" 20 plugin rollback
probe plugin-sync-badopt nz "unknown" 20 plugin sync --bogus
probe plugin-list-json 0 "" 20 plugin list --json
# Census (recorded, not fixed): list and clean ADMIT unknown flags silently;
# update/restore route the word into their own error surface.
probe plugin-list-badopt 0 "(no sources installed" 20 plugin list --bogus
probe plugin-clean-badopt 0 "nothing to clean" 20 plugin clean --bogus
probe plugin-update-badopt nz "unknown" 20 plugin update --bogus
probe plugin-restore-badopt nz "unknown source" 20 plugin restore --bogus
probe plugin-discover 0 "Niubash plugin ecosystem" 30 plugin discover
probe plugin-discover-badopt nz "unknown plugin option" 20 plugin discover --bogus

# ── Phase C: sub-verb surfaces (source/recipe/distro/mirror) ───────────────
probe psource-usage 0 "Usage:  niu plugin source" 20 plugin source
probe psource-unknown nz "unknown plugin source subcommand" 20 plugin source bogus
probe psource-list 0 "-" 20 plugin source list
for v in add trust remove rollback verify sign; do
    probe "psource-$v-noarg" nz "-" 20 plugin source "$v"
done
# V4: bare update = update-all, accepted, rc 0; usage must document [<id>]
probe psource-update-bare 0 "(no sources installed)" 20 plugin source update
o="$OUT/psource-help.out" e="$OUT/psource-help.err"
timeout 20 "$NIU" plugin source --help >"$o" 2>"$e" </dev/null
ok_if v4-source-usage-update-optional command grep -q "update \[<id>\]" "$o"

probe precipe-usage 0 "Usage:  niu plugin recipe" 20 plugin recipe
probe precipe-list 0 "recipes" 30 plugin recipe list
probe precipe-show-noarg nz "" 20 plugin recipe show
probe precipe-unknown nz "unknown plugin recipe subcommand" 20 plugin recipe bogus
probe pdistro-usage 0 "Usage:  niu plugin distro" 20 plugin distro
probe pdistro-list 0 "" 20 plugin distro list
probe pdistro-apply-noarg nz "" 20 plugin distro apply
probe pdistro-unknown nz "unknown plugin distro subcommand" 20 plugin distro bogus
probe pmirror-usage 0 "Usage:  niu plugin mirror" 20 plugin mirror
probe pmirror-list 0 "" 20 plugin mirror list
probe pmirror-set-noarg nz "" 20 plugin mirror set
probe pmirror-unknown nz "unknown plugin mirror subcommand" 20 plugin mirror bogus

# ── Phase D: retired verbs must retire honestly ────────────────────────────
for v in info search themes bundle doctor review use tool tools; do
    probe "retired-$v" nz "retired" 20 plugin "$v"
done

# ── Phase E: usage-text vs dispatch diff ───────────────────────────────────
# Dispatch table = the match arms of run_plugin_command (src/main.rs).
ACTIVE=""
if [ -n "${REPO:-}" ] && [ -f "$REPO/src/main.rs" ]; then
    ACTIVE=$(sed -n '/fn run_plugin_command/,/^}/p' "$REPO/src/main.rs" |
        command grep -oE '"[a-z|-]+" =>' |
        sed 's/"\([a-z|-]*\)" =>/\1/' |
        command grep -vE '^(--help|sources|recipes|distros|collections|mirrors)$' |
        command grep -vE '^(info|search|themes|bundle|doctor|review|use|tool|tools)$' |
        sort -u)
fi
if [ -n "$ACTIVE" ]; then
    o="$OUT/usage-plugin.out" e="$OUT/usage-plugin.err"
    timeout 20 "$NIU" plugin --help >"$o" 2>"$e" </dev/null
    # Every active dispatch verb must appear in `niu plugin --help` as a
    # two-space-indented command row.
    MISSING_USAGE=""
    for v in $ACTIVE; do
        command grep -qE "^  $v( |\[|$)" "$o" || MISSING_USAGE="$MISSING_USAGE $v"
    done
    ok_if usage-vs-dispatch-plugin test -z "$MISSING_USAGE"
    [ -n "$MISSING_USAGE" ] && echo "  missing from usage:$MISSING_USAGE"

    # V2: every active dispatch verb must be advertised in `niu --help`
    # (combined rows like `plugin update|sync|...` count per verb).
    o="$OUT/usage-toplevel.out" e="$OUT/usage-toplevel.err"
    timeout 20 "$NIU" --help >"$o" 2>"$e" </dev/null
    MISSING_TOP=""
    for v in $ACTIVE; do
        command grep -qE "plugin ([a-z]+[|])*$v( |\||$)" "$o" ||
            MISSING_TOP="$MISSING_TOP $v"
    done
    ok_if v2-toplevel-help-covers-verbs test -z "$MISSING_TOP"
    [ -n "$MISSING_TOP" ] && echo "  missing from top-level help:$MISSING_TOP"
else
    echo "SKIP usage-vs-dispatch (repo root with src/main.rs not found)"
fi

# ── Phase F: docs verb tables vs dispatch (V3 class) ───────────────────────
QUICKREF="${REPO:+$REPO/docs/plugins-quickref.md}"
GUIDE="${REPO:+$REPO/docs/plugins-guide.md}"
if [ -n "${QUICKREF:-}" ] && [ -f "$QUICKREF" ]; then
    MISSING_DOC=""
    for v in add list enable disable trust sync update restore rollback discover source mirror recipe distro ui; do
        command grep -qE "\`$v( |\`|\[)" "$QUICKREF" || MISSING_DOC="$MISSING_DOC $v"
    done
    ok_if docs-quickref-covers-verbs test -z "$MISSING_DOC"
    [ -n "$MISSING_DOC" ] && echo "  missing from quickref:$MISSING_DOC"
    ok_if v3-quickref-sync-adopt command grep -q 'sync \[--prune\] \[--adopt\] \[--bootstrap\]' "$QUICKREF"
fi
if [ -n "${GUIDE:-}" ] && [ -f "$GUIDE" ]; then
    ok_if docs-guide-mentions-adopt command grep -q -- '--adopt' "$GUIDE"
fi

# ── Phase G: V1 stale-PATH bootstrap-noise regression ──────────────────────
# A niu that predates `plugin sync` first on PATH: the pre-fix wizard rc
# (`command -v niu … && niu plugin sync --bootstrap`) resolved and ran it on
# EVERY startup, printing `niu: unknown plugin subcommand 'sync'` (real
# 1.2.5) or its own output/noise (synthetic stand-in). The fixed rc pins the
# reconcile to NIU_SHELL (the running exe; exported at src/main.rs
# run_main from current_exe), so a PATH-shadowed stale install must not run
# — and doctor must surface the mismatch.
STALE="${STALE_NIU:-}"
if [ -z "$STALE" ] && [ -f "/c/Users/Administrator/AppData/Local/Programs/Winuxsh/niu.exe" ]; then
    STALE="/c/Users/Administrator/AppData/Local/Programs/Winuxsh/niu.exe"
fi
if [ -z "$STALE" ]; then
    # Portable fallback: cmd.exe answers "'plugin' is not recognized" on
    # stderr when the old rc line runs it as `niu plugin sync --bootstrap` —
    # visible pollution either way.
    cp /c/Windows/System32/cmd.exe "$SB/fakepath/niu.exe" 2>/dev/null &&
        STALE="$SB/fakepath/niu.exe"
fi
if [ -n "$STALE" ] && [ -f "$STALE" ]; then
    cp "$STALE" "$SB/fakepath/niu.exe"
    # Mixed (Windows-drive) form joined with a colon: MSYS converts the whole
    # PATH to Windows form when spawning the native exe, so BOTH the engine's
    # `command -v` walk and the Rust PATH walks see the fake dir first.
    win_fake=$(cygpath -m "$SB/fakepath" 2>/dev/null || echo "$SB/fakepath")
    export PATH="$win_fake:$PATH"

    timeout 60 "$NIU" setup --preset minimal >"$OUT/v1-wizard.out" 2>"$OUT/v1-wizard.err"
    ok_if v1-wizard-exit-0 test "$?" -eq 0
    ok_if v1-wizard-emits-niu-shell-line command grep -q 'command -v "\${NIU_SHELL:-niu}"' "$SB/home/.niubashrc"

    o="$OUT/v1-startup.out" e="$OUT/v1-startup.err"
    timeout 60 "$NIU" -C 'echo v1-probe-marker' >"$o" 2>"$e" </dev/null
    ok_if v1-startup-has-marker command grep -q "v1-probe-marker" "$o"
    ok_if v1-startup-clean-under-stale-path bash -c \
        "! command grep -qiE 'unknown plugin subcommand|is not recognized' '$o' '$e'"

    o="$OUT/v1-doctor.out" e="$OUT/v1-doctor.err"
    timeout 60 "$NIU" doctor >"$o" 2>"$e" </dev/null
    ok_if v1-doctor-warns-path-mismatch command grep -qi "niu on PATH" "$o" "$e"
else
    echo "SKIP v1-* (no stale niu binary available; set STALE_NIU)"
fi

echo
echo "PASS=$PASS FAIL=$FAIL"
[ -n "$FAILED" ] && echo "failed:$FAILED"
echo "artifacts: $OUT"
exit 0
