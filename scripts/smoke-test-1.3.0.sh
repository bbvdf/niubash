#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Niubash 1.3.0 release smoke suite (wt49/smokesweep) — one repeatable
# end-to-end pass over the release checklist, run against a real niu binary
# in a throwaway sandbox. Nothing in here touches the real HOME, the real
# plugin sources/spec/tool registry, the real font state, or the real
# Windows Terminal settings.
#
# Legs (release checklist order):
#   A  install chain   plugin add --path → trust → enable → fresh-shell
#                      effect (oh-my-bash OSH_THEME, bash-it PS1), spec↔rc
#                      sync idempotency, clean --bootstrap
#   B  download chain  recipe list non-empty · empty tool list · real
#                      download-driver install+remove (network, skippable)
#                      · `niu font` end-to-end (network + PTY, skippable)
#   C  floor           external theme claims PS1 across a boot; disable
#                      releases the slot back to the shell floor
#   D  setup wizard    `niu setup --preset recommended` → rc parses clean
#   E  basics          --version · echo · `seq 1 3 | wc -l` = 3 · cat -n
#   F  offline suite   cargo test --test smoke_1_3_0 (the CI mirror of A–E)
#
# Usage:
#   scripts/smoke-test-1.3.0.sh              # build debug niu, then run
#   NIU_SMOKE_BINARY=<exe> scripts/smoke-test-1.3.0.sh   # prebuilt binary
#   NIU_SMOKE_SKIP_FONT=1 ...                # force-skip the font leg
#   NIU_SMOKE_SKIP_NET=1  ...                # force-skip network legs
#
# Exit code: 0 iff every non-skipped leg passed. Skips are reported with
# their reason; a skip never fails the run (release checklist: network legs
# are "可跳过如无网络").
# ─────────────────────────────────────────────────────────────────────────────
set -u

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO_ROOT"

# Git Bash ships coreutils timeout at /usr/bin/timeout; System32's timeout.exe
# is a different tool ("wait N seconds"), so pin the coreutils one.
TIMEOUT_BIN=/usr/bin/timeout
if [ ! -x "$TIMEOUT_BIN" ]; then
    TIMEOUT_BIN=$(command -v timeout || true)
fi
run_timed() { # run_timed <seconds> <cmd...>
    local secs=$1; shift
    if [ -n "${TIMEOUT_BIN:-}" ]; then
        "$TIMEOUT_BIN" --foreground "$secs" "$@"
    else
        "$@"
    fi
}

PASS=0; FAIL=0; SKIP=0
declare -a REPORT=()
record() { # record <PASS|FAIL|SKIP> <id> <detail>
    REPORT+=("$1|$2|$3")
    case $1 in
        PASS) PASS=$((PASS + 1)); printf '  PASS  %-34s %s\n' "$2" "$3" ;;
        FAIL) FAIL=$((FAIL + 1)); printf '  FAIL  %-34s %s\n' "$2" "$3" ;;
        SKIP) SKIP=$((SKIP + 1)); printf '  SKIP  %-34s %s\n' "$2" "$3" ;;
    esac
}

section() { printf '\n== %s ==\n' "$1"; }

# ── binary under test ────────────────────────────────────────────────────────
if [ -n "${NIU_SMOKE_BINARY:-}" ]; then
    NIU=$NIU_SMOKE_BINARY
else
    section "building debug niu"
    if [ "${NIU_SMOKE_NO_BUILD:-0}" != "1" ]; then
        if ! cargo build 2>&1 | tail -1; then
            echo "cargo build failed — refusing to smoke a stale binary"
            exit 1
        fi
    fi
    NIU=$REPO_ROOT/target/debug/niu.exe
    [ -x "$NIU" ] || [ -f "$NIU" ] || { echo "no binary at $NIU"; exit 1; }
fi
echo "niu under test: $NIU"

# ── sandbox ──────────────────────────────────────────────────────────────────
# Remember the real HOME for legs that must run outside the sandbox (cargo).
ORIG_HOME=${HOME:-}
ORIG_USERPROFILE=${USERPROFILE:-}
SB=$(mktemp -d)
trap 'rm -rf "$SB"' EXIT
mkdir -p "$SB/home/.niubash" "$SB/sources" "$SB/tools" "$SB/local-appdata"
export HOME="$SB/home" USERPROFILE="$SB/home"
export NIU_PLUGIN_SOURCES_ROOT="$SB/sources"
export NIU_PLUGIN_TOOLS_ROOT="$SB/tools"
export NIU_PLUGIN_SPEC="$SB/home/.niubash/plugins.toml"
unset NIU_ENV BASH_ENV || true

FIXTURES=$REPO_ROOT/tests/fixtures/sources
nu() { run_timed 120 "$NIU" "$@"; } # every CLI call is timeout-guarded

# ── A. install chain ─────────────────────────────────────────────────────────
section "A. install chain"

if nu plugin add oh-my-bash --path "$FIXTURES/oh-my-bash" >"$SB/a1.out" 2>&1 \
    && nu plugin trust oh-my-bash >>"$SB/a1.out" 2>&1 \
    && nu plugin enable oh-my-bash >>"$SB/a1.out" 2>&1 \
    && nu plugin enable agnoster >>"$SB/a1.out" 2>&1; then
    theme=$(nu -c '. ~/.niubashrc; echo $OSH_THEME' 2>/dev/null | tr -d '\r\n')
    if [ "$theme" = "agnoster" ]; then
        record PASS a1-omb-theme "OSH_THEME=agnoster in a fresh -c shell"
    else
        record FAIL a1-omb-theme "OSH_THEME was '$theme', expected agnoster"
    fi
else
    record FAIL a1-omb-theme "add/trust/enable chain failed: $(tail -2 "$SB/a1.out" | tr '\n' ' ')"
fi

if nu plugin add bash-it --path "$FIXTURES/bash-it" >"$SB/a2.out" 2>&1 \
    && nu plugin trust bash-it >>"$SB/a2.out" 2>&1 \
    && nu plugin enable base >>"$SB/a2.out" 2>&1 \
    && nu plugin enable demox >>"$SB/a2.out" 2>&1; then
    ps1=$(nu -c '. ~/.niubashrc; printf "%s" "$PS1"' 2>/dev/null | tr -d '\r\n')
    if [ "$ps1" = "demox> " ]; then
        record PASS a2-bash-it-theme "bash-it theme claims PS1 (demox> )"
    else
        record FAIL a2-bash-it-theme "PS1 was '$ps1', expected 'demox> '"
    fi
else
    record FAIL a2-bash-it-theme "add/trust/enable chain failed: $(tail -2 "$SB/a2.out" | tr '\n' ' ')"
fi

# spec write/read/sync idempotency on the oh-my-bash sandbox state
if nu plugin sync >"$SB/a3.out" 2>&1; then
    cp "$NIU_PLUGIN_SPEC" "$SB/spec.once" 2>/dev/null || : >"$SB/spec.once"
    cp "$HOME/.niubashrc" "$SB/rc.once"
    if nu plugin sync >"$SB/a3b.out" 2>&1 \
        && cmp -s "$SB/spec.once" "$NIU_PLUGIN_SPEC" \
        && cmp -s "$SB/rc.once" "$HOME/.niubashrc"; then
        boot_out=$(nu plugin sync --bootstrap 2>&1)
        if [ -z "$(printf '%s' "$boot_out" | tr -d '[:space:]')" ]; then
            record PASS a3-sync-idempotent "spec+rc byte-identical; --bootstrap silent"
        else
            record FAIL a3-sync-idempotent "--bootstrap on a clean spec printed: $boot_out"
        fi
    else
        record FAIL a3-sync-idempotent "second sync changed spec or rc"
    fi
else
    record FAIL a3-sync-idempotent "first sync failed: $(tail -2 "$SB/a3.out" | tr '\n' ' ')"
fi

# ── B. download chain ────────────────────────────────────────────────────────
section "B. download chain"

if nu plugin recipe list >"$SB/b1.out" 2>&1; then
    count=$(sed -n 's/^\([0-9][0-9]*\) recipes$/\1/p' "$SB/b1.out" | tail -1)
    if [ -n "${count:-}" ] && [ "$count" -ge 2 ] 2>/dev/null; then
        record PASS b1-recipe-list "$count recipes in the compiled-in index"
    else
        record FAIL b1-recipe-list "unexpected index output: $(tail -1 "$SB/b1.out")"
    fi
else
    record FAIL b1-recipe-list "recipe list exited non-zero"
fi

if out=$(nu plugin tool list 2>&1); then
    case $out in
        *"no tools installed"*) record PASS b2-tool-list-empty "empty registry lists cleanly" ;;
        *) record FAIL b2-tool-list-empty "unexpected output: $out" ;;
    esac
else
    record FAIL b2-tool-list-empty "tool list exited non-zero"
fi

# Real download-driver install (pure-Rust transport). Network legs degrade
# to SKIP when the CDN is unreachable — offline is an environment state,
# not a product failure.
if [ "${NIU_SMOKE_SKIP_NET:-0}" = "1" ]; then
    record SKIP b3-download-driver "forced by NIU_SMOKE_SKIP_NET=1"
else
    installed=""
    for attempt in 1 2 3; do
        if run_timed 300 "$NIU" plugin recipe add fzf >"$SB/b3.out" 2>&1; then
            installed=yes; break
        fi
        grep -qE "download failed|niu: reading|timed out|tls|Connection Failed" "$SB/b3.out" \
            || break # non-transport error: stop retrying
        sleep 5
    done
    if [ -n "$installed" ]; then
        if [ -d "$NIU_PLUGIN_TOOLS_ROOT/fzf" ] \
            && nu plugin enable fzf >/dev/null 2>&1 \
            && grep -q "niu tool path: fzf" "$HOME/.niubashrc" \
            && nu plugin tool remove fzf >/dev/null 2>&1 \
            && [ ! -d "$NIU_PLUGIN_TOOLS_ROOT/fzf" ]; then
            record PASS b3-download-driver "fzf installed (sha256-pinned), enabled, removed"
        else
            record FAIL b3-download-driver "install landed but enable/remove leg failed"
        fi
    else
        reason=$(tail -1 "$SB/b3.out" 2>/dev/null | tr -d '\r')
        case $reason in
            *"download failed"*|*"niu: reading"*|*tls*|*"timed out"*|*"Connection Failed"*)
                record SKIP b3-download-driver "asset transport unreachable: $reason" ;;
            *) record FAIL b3-download-driver "${reason:-recipe add fzf failed}" ;;
        esac
    fi
fi

# `niu font` end-to-end: needs a real interactive terminal (its own guard),
# so this leg drives a ConPTY with LOCALAPPDATA redirected into the sandbox
# — TTFs land in the sandbox, one HKCU Fonts entry per file is written and
# removed again afterwards (it pointed at the deleted sandbox anyway).
if [ "${NIU_SMOKE_SKIP_FONT:-0}" = "1" ]; then
    record SKIP b4-font-install "forced by NIU_SMOKE_SKIP_FONT=1"
elif [ "${NIU_SMOKE_SKIP_NET:-0}" = "1" ]; then
    record SKIP b4-font-install "network legs disabled (NIU_SMOKE_SKIP_NET=1)"
elif ! command -v python >/dev/null 2>&1; then
    record SKIP b4-font-install "python not available for the PTY harness"
elif ! python -c "import winpty, pyte" >/dev/null 2>&1; then
    record SKIP b4-font-install "pywinpty/pyte not installed (PTY harness)"
elif ! curl -fsI --max-time 15 https://github.com >/dev/null 2>&1; then
    record SKIP b4-font-install "github.com unreachable"
else
    run_timed 420 python - "$NIU" "$SB" >"$SB/b4.out" 2>&1 <<'PYEOF'
import os, sys, time
from pathlib import Path
from winpty import PtyProcess

exe, sb = sys.argv[1], Path(sys.argv[2])
home = sb / "home"
sysroot = os.environ.get("SystemRoot", r"C:\Windows")
env = {
    "SystemRoot": sysroot,
    "COMSPEC": sysroot + r"\System32\cmd.exe",
    "HOME": str(home),
    "USERPROFILE": str(home),
    "LOCALAPPDATA": str(sb / "local-appdata"),
    "NIU_PLUGIN_SOURCES_ROOT": str(sb / "sources"),
    "NIU_PLUGIN_TOOLS_ROOT": str(sb / "tools"),
    "NIU_PLUGIN_SPEC": str(home / ".niubash" / "plugins.toml"),
    "WINDIR": sysroot,
    "PATH": sysroot + r"\System32;" + sysroot,
}
proc = PtyProcess.spawn(f'"{exe}" font', dimensions=(120, 36), env=env)
buf = ""
deadline = time.time() + 380
sent = False
while time.time() < deadline:
    try:
        buf += proc.read(65536)
    except Exception:
        break # process closed the pty
    if not sent and "Choose a Nerd Font" in buf:
        time.sleep(0.5)
        proc.write("\r") # default selection: JetBrainsMono
        sent = True
    if "Installed JetBrainsMono" in buf or "download failed" in buf \
       or "needs an interactive terminal" in buf or proc.exited:
        break
    time.sleep(0.2)
if proc.isalive():
    proc.terminate(force=True)
print(buf[-2000:])
fonts_dir = sb / "local-appdata" / "Microsoft" / "Windows" / "Fonts"
ttfs = sorted(p.name for p in fonts_dir.glob("*NerdFontMono*.ttf")) if fonts_dir.is_dir() else []
print("TTF_COUNT", len(ttfs))
for name in ttfs:
    print("REGNAME", name.rsplit(".", 1)[0] + " (TrueType)")
PYEOF
    ttf_count=$(sed -n 's/^TTF_COUNT //p' "$SB/b4.out" | tail -1)
    if [ "${ttf_count:-0}" -ge 1 ] 2>/dev/null; then
        # Clean the sandbox-pointed HKCU font registrations this leg wrote.
        sed -n 's/^REGNAME //p' "$SB/b4.out" | while IFS= read -r regname; do
            [ -n "$regname" ] && reg delete \
                'HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts' \
                /v "$regname" /f >/dev/null 2>&1
        done
        record PASS b4-font-install "$ttf_count NerdFontMono TTFs landed in the sandbox fonts dir"
    else
        tail_line=$(grep -E "download failed|timed out|interactive" "$SB/b4.out" | tail -1 | tr -d '\r')
        if [ -n "$tail_line" ]; then
            record SKIP b4-font-install "font download unavailable: $tail_line"
        else
            record FAIL b4-font-install "no TTFs installed; PTY log: $(tail -3 "$SB/b4.out" | tr '\n' ' ' | cut -c1-160)"
        fi
    fi
fi

# ── C. defaults-as-floor ─────────────────────────────────────────────────────
section "C. defaults-as-floor"

claimed=$(printf 'echo CLAIM=[$PS1]\nexit\n' | run_timed 120 "$NIU" -i 2>/dev/null | tr -d '\r')
# Fixed-string matching: the payloads contain glob metacharacters ([ ]),
# so case patterns would misread them. This sandbox enabled oh-my-bash AND
# bash-it (legs A1/A2); whichever external theme owns PS1 after the last
# block runs proves the claim — the product must never fight back.
if printf '%s' "$claimed" | grep -qF 'CLAIM=[agnoster-fixture-face ]'; then
    record PASS c1-theme-claims-slot "oh-my-bash theme owns PS1 across an interactive boot"
elif printf '%s' "$claimed" | grep -qF 'CLAIM=[demox> ]'; then
    record PASS c1-theme-claims-slot "bash-it theme owns PS1 across an interactive boot"
elif printf '%s' "$claimed" | grep -qF 'CLAIM=['; then
    record FAIL c1-theme-claims-slot "no external theme claim in boot output: $(printf '%s' "$claimed" | tail -1 | cut -c1-120)"
else
    record FAIL c1-theme-claims-slot "no CLAIM marker in boot output"
fi

# Release every external claim, then the shell-default floor must render.
if nu plugin disable oh-my-bash >/dev/null 2>&1 \
    && nu plugin disable bash-it >/dev/null 2>&1; then
    released=$(printf 'echo CLAIM=[$PS1]\nexit\n' | run_timed 120 "$NIU" -i 2>/dev/null | tr -d '\r')
    if printf '%s' "$released" | grep -qE 'agnoster-fixture-face|demox'; then
        record FAIL c2-floor-released "an external PS1 claim survived its disable"
    elif printf '%s' "$released" | grep -qF 'CLAIM=[\s-\v\$ ]'; then
        # On the piped -i path the engine initializes PS1 to GNU's default
        # `\s-\v\$ ` — the floor of the shell itself; the product floor's
        # restore is pinned by the niubash-runtime unit tests (see
        # tests/defaults_floor.rs for the mapping).
        record PASS c2-floor-released "slot released; shell-default floor renders"
    else
        record FAIL c2-floor-released "unexpected PS1 after disable: $(printf '%s' "$released" | tail -1 | cut -c1-120)"
    fi
else
    record FAIL c2-floor-released "plugin disable of the external sources failed"
fi

# ── D. setup wizard ──────────────────────────────────────────────────────────
section "D. setup wizard"

SB2=$(mktemp -d)
mkdir -p "$SB2/home"
# Subshell assignments (NOT the `env` command — this tool shell ships a
# shimmed env that swallows Windows spawns) keep the isolated HOME inside
# the leg while MSYS still converts the POSIX path for the native exe.
if (HOME="$SB2/home" USERPROFILE="$SB2/home" run_timed 120 "$NIU" setup --preset recommended \
    >"$SB/d1.out" 2>&1); then
    rc_file="$SB2/home/.niubashrc"
    if [ ! -f "$rc_file" ]; then
        record FAIL d1-preset-rc "setup wrote no rc"
    elif grep -q "NIU_THEME=\|NIU_PLUGINS=" "$rc_file"; then
        record FAIL d1-preset-rc "rc sells retired stack fields"
    elif (HOME="$SB2/home" USERPROFILE="$SB2/home" run_timed 120 "$NIU" -n "$rc_file" \
        >"$SB/d1n.out" 2>&1); then
        journal="$SB2/home/.niubash/setup-journal.toml"
        if grep -q "preset = 'recommended'" "$journal" 2>/dev/null; then
            record PASS d1-preset-rc "rc written, parses clean, journal records the preset"
        else
            record FAIL d1-preset-rc "journal missing the preset record"
        fi
    else
        record FAIL d1-preset-rc "generated rc does not parse: $(tail -2 "$SB/d1n.out" | tr '\n' ' ')"
    fi
else
    record FAIL d1-preset-rc "setup --preset recommended failed: $(tail -2 "$SB/d1.out" | tr '\n' ' ')"
fi
rm -rf "$SB2"

# ── E. basics ────────────────────────────────────────────────────────────────
section "E. basics"

ver=$(nu --version 2>/dev/null | head -1 | tr -d '\r')
case $ver in
    *Niubash*|*niubash*) record PASS e1-version "$ver" ;;
    *) record FAIL e1-version "unexpected --version output: $ver" ;;
esac

hello=$(nu -c 'echo hello' 2>/dev/null | tr -d '\r\n')
[ "$hello" = "hello" ] && record PASS e2-echo "echo hello" \
    || record FAIL e2-echo "got '$hello'"

wc_out=$(nu -c 'seq 1 3 | wc -l' 2>/dev/null | tr -d '[:space:]')
[ "$wc_out" = "3" ] && record PASS e3-pipeline "seq 1 3 | wc -l = 3" \
    || record FAIL e3-pipeline "got '$wc_out'"

printf 'one\ntwo\nthree\n' >"$SB/lines.txt"
# cat -n pads the number column; assert the numbers are present in order on
# the right lines without depending on the exact tab/space padding.
if nu -c "cat -n '$SB/lines.txt'" 2>/dev/null | tr -d '\r' | awk '
    $0 ~ /^[[:space:]]*1[[:space:]]+one$/ { seen1 = 1; next }
    seen1 && $0 ~ /^[[:space:]]*2[[:space:]]+two$/ { seen2 = 1; next }
    seen2 && $0 ~ /^[[:space:]]*3[[:space:]]+three$/ { ok = 1 }
    END { exit ok ? 0 : 1 }'; then
    record PASS e4-cat-n "cat -n numbers the lines"
else
    record FAIL e4-cat-n "unexpected cat -n output: $(nu -c "cat -n '$SB/lines.txt'" 2>/dev/null | head -1 | cut -c1-40)"
fi

# ── F. offline CI mirror ─────────────────────────────────────────────────────
section "F. offline CI mirror (cargo test --test smoke_1_3_0)"

if [ "${NIU_SMOKE_SKIP_CARGO:-0}" = "1" ]; then
    record SKIP f1-cargo-mirror "forced by NIU_SMOKE_SKIP_CARGO=1"
# cargo needs the REAL home (CARGO_HOME registry), not the smoke sandbox —
# the test file sandboxes every niu invocation itself.
elif (if [ -n "$ORIG_HOME" ]; then export HOME="$ORIG_HOME"; else unset HOME; fi
      if [ -n "$ORIG_USERPROFILE" ]; then export USERPROFILE="$ORIG_USERPROFILE"; else unset USERPROFILE; fi
      run_timed 600 cargo test --test smoke_1_3_0 >"$SB/f1.out" 2>&1); then
    got=$(sed -n 's/^test result: ok\. \([0-9]*\) passed.*/\1/p' "$SB/f1.out" | tail -1)
    record PASS f1-cargo-mirror "${got:-?} offline legs green (b3 auto-skips without NIU_SMOKE_NETWORK=1)"
else
    record FAIL f1-cargo-mirror "$(grep -E '^test result:|^error' "$SB/f1.out" | tail -1)"
fi

# ── summary ──────────────────────────────────────────────────────────────────
section "smoke summary"
printf '  PASS %d · FAIL %d · SKIP %d\n\n' "$PASS" "$FAIL" "$SKIP"
for line in "${REPORT[@]}"; do
    IFS='|' read -r state id detail <<<"$line"
    printf '  %-4s %-34s %s\n' "$state" "$id" "$detail"
done
echo
if [ "$FAIL" -gt 0 ]; then
    echo "SMOKE RESULT: FAIL ($FAIL failed legs)"
    exit 1
fi
echo "SMOKE RESULT: PASS ($PASS passed, $SKIP skipped)"
