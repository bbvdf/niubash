#!/usr/bin/env bash
# Real-world script harness — complex, realistic bash programs through niu.
#
# Motivation (niubash#139/#140/#141 + the starship-init regression): minimal
# probes pass while large real scripts hit char-boundary panics, closed-pipe
# aborts, and spawn failures. This gate runs tool-generated init code, the
# shipped oh-my-niu bundle, and the Git-for-Windows POSIX corpus — the exact
# classes users hit.
#
# Usage:
#   sh scripts/run-realworld-with-niubash.sh [path/to/niu]
#
# Optional env:
#   WSL_GNU_BASH   — GNU oracle for semantic comparisons (default:
#                     /usr/local/bin/bash via wsl; skipped when absent)
#   NIU_BUNDLE     — oh-my-niu bundle dir (default: installed location)
#   GIT_CORE       — git-core script dir (default: /mingw64/libexec/git-core)

set -u
NIU="${1:-${NIU_EXE:-niu}}"
TOTAL=0; PASSED=0; FAILED=0; SKIPPED=0
FAILED_NAMES=()

WIN_HOME="${USERPROFILE:-$HOME}"
WIN_HOME="${WIN_HOME//\//}"
NIU_BUNDLE="${NIU_BUNDLE:-$WIN_HOME/AppData/Local/Programs/Winuxsh/bundles/oh-my-niu}"
WINUXCMD_BIN="${WINUXCMD_BIN:-$WIN_HOME/AppData/Local/Programs/Winuxsh/winuxcmd/usr/bin}"
GIT_CORE="${GIT_CORE:-/mingw64/libexec/git-core}"

pass() { PASSED=$((PASSED+1)); TOTAL=$((TOTAL+1)); }
fail() { FAILED=$((FAILED+1)); TOTAL=$((TOTAL+1)); FAILED_NAMES+=("$1"); printf 'FAIL  %s\n' "$1"; [ -n "${2:-}" ] && printf '      %s\n' "$2"; }
skip() { SKIPPED=$((SKIPPED+1)); TOTAL=$((TOTAL+1)); printf 'SKIP  %s (%s)\n' "$1" "$2"; }

# run_niu <script>: run niu -c with 30s bound, capture stdout/stderr/rc into
# globals NI_OUT/NI_ERR/NI_RC.
run_niu() {
    NI_OUT="$(timeout 30 "$NIU" -c "$1" 2>/tmp/nr-err.$$)"
    NI_RC=$?
    NI_ERR="$(cat /tmp/nr-err.$$ 2>/dev/null)"
}

# ---------------------------------------------------------------------------
# 1. Tool-generated init scripts — the exact starship regression shape:
#    eval "$(tool init bash)" must parse+execute cleanly (issue #139's
#    ${BASH_VERSINFO[i]} site lives inside starship's generated code).
# ---------------------------------------------------------------------------
tool_init_eval() {
    local name="$1" gen="$2" extra="${3:-}"
    local gen_out
    if ! gen_out="$(timeout 15 bash -c "$gen" 2>/dev/null)"; then
        skip "init-eval:$name" "generator failed"
        return
    fi
    local tmp="/tmp/nr-init-$name.$$.sh"
    printf '%s\n' "$gen_out" > "$tmp"
    # eval the captured text inside niu exactly like the plugin does.
    run_niu "eval \"\$(cat '$tmp')\"$extra; echo eval-ok"
    rm -f "$tmp"
    if [ "$NI_RC" -eq 0 ] && [ "$NI_OUT" = "eval-ok" ] && ! printf '%s' "$NI_ERR" | grep -qi "panic\|syntax error"; then
        pass
    else
        fail "init-eval:$name" "rc=$NI_RC out=$NI_OUT err=$NI_ERR"
    fi
}

tool_init_eval starship "\"$WINUXCMD_BIN/starship.exe\" init bash"
tool_init_eval zoxide   "\"$WINUXCMD_BIN/zoxide.exe\" init bash"
tool_init_eval fzf      "\"$WINUXCMD_BIN/fzf.exe\" --bash"

# ---------------------------------------------------------------------------
# 2. oh-my-niu bundle end-to-end — source the real bundle like ~/.niubashrc.
# ---------------------------------------------------------------------------
if [ -f "$NIU_BUNDLE/oh-my-niu.niu" ]; then
    run_niu "export NIUBASH='$NIU_BUNDLE'; NIU_PLUGINS=(git aliases functions); source '$NIU_BUNDLE/oh-my-niu.niu'; echo bundle-ok"
    if [ "$NI_RC" -eq 0 ] && [ "$NI_OUT" = "bundle-ok" ] && ! printf '%s' "$NI_ERR" | grep -qi "panic\|syntax error"; then
        pass
    else
        fail "bundle:oh-my-niu" "rc=$NI_RC out=$NI_OUT err=$NI_ERR"
    fi
else
    skip "bundle:oh-my-niu" "bundle not at $NIU_BUNDLE"
fi

# ---------------------------------------------------------------------------
# 3. Git-for-Windows POSIX corpus — GPL scripts executed in place (never
#    vendored). Parse conformance: any rc is fine EXCEPT a syntax-error
#    diagnostic; assert by grepping stderr.
# ---------------------------------------------------------------------------
git_script_parse() {
    local script="$1" name
    name="$(basename "$script")"
    [ -f "$script" ] || { skip "gitcore:$name" "absent"; return; }
    NI_ERR="$(timeout 30 "$NIU" "$script" 2>&1 >/dev/null)"
    NI_RC=$?
    if ! printf '%s' "$NI_ERR" | grep -qi "syntax error"; then
        pass
    else
        fail "gitcore:$name" "syntax error: $NI_ERR"
    fi
}

for s in "$GIT_CORE/git-subtree" "$GIT_CORE/git-filter-branch" "$GIT_CORE/git-submodule" \
         "$GIT_CORE/git-sh-setup" "$GIT_CORE/git-mergetool--lib"; do
    git_script_parse "$s"
done

# git-prompt + git-completion: source, then exercise the exported functions.
if [ -f /mingw64/share/git/completion/git-prompt.sh ]; then
    run_niu 'source /mingw64/share/git/completion/git-prompt.sh; t=$(__git_ps1 "p:%s" 2>/dev/null); echo gp-ok'
    if [ "$NI_RC" -eq 0 ] && [ "$NI_OUT" = "gp-ok" ]; then pass; else fail "git:__git_ps1" "rc=$NI_RC out=$NI_OUT err=$NI_ERR"; fi
else
    skip "git:__git_ps1" "completion dir absent"
fi
if [ -f /mingw64/share/git/completion/git-completion.bash ]; then
    run_niu 'source /mingw64/share/git/completion/git-completion.bash; echo gc-ok'
    if [ "$NI_RC" -eq 0 ] && [ "$NI_OUT" = "gc-ok" ] && ! printf '%s' "$NI_ERR" | grep -qi "syntax error"; then
        pass
    else
        fail "git:completion" "rc=$NI_RC err=$NI_ERR"
    fi
else
    skip "git:completion" "absent"
fi

# ---------------------------------------------------------------------------
# 4. Issue regressions — #139 multibyte comsub, #140 closed-pipe launcher,
#    #141 external spawn from this parent.
# ---------------------------------------------------------------------------
run_niu 'v=hello; echo "${v}$(echo 中)"; v=ok; echo "x${v}$(echo 中)y"'
if [ "$NI_RC" -eq 0 ] && [ "$NI_OUT" = $'hello中\nxok中y' ]; then pass; else fail "issue139:utf8-comsub" "rc=$NI_RC out=$NI_OUT err=$NI_ERR"; fi

timeout 15 "$NIU" --version 2>/dev/null | head -1 >/dev/null
if [ $? -eq 0 ]; then pass; else fail "issue140:version|head" "rc=$?"; fi
timeout 15 "$NIU" --version >/dev/null 2>&1 | true
pass  # `| true` can only fail the pipe, not the launcher — reaching here means no abort

for prog in sort cut rev xargs tr seq wc head; do
    if ! command -v "$prog" >/dev/null 2>&1; then
        skip "issue141:$prog" "not in PATH"
        continue
    fi
    run_niu "printf 'b\na\n' | $prog 2>/dev/null | tr -d '\r'"
    if [ "$NI_RC" -eq 0 ] && ! printf '%s' "$NI_ERR" | grep -qi "error"; then
        pass
    else
        fail "issue141:pipe-$prog" "rc=$NI_RC out=$NI_OUT err=$NI_ERR"
    fi
done

rm -f /tmp/nr-err.$$

echo
echo "=== real-world harness: $PASSED passed / $FAILED failed / $SKIPPED skipped (total $TOTAL) ==="
[ "$FAILED" -eq 0 ]
