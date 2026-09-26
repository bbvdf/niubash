#!/usr/bin/env bash
# niubash#141 reproduction probe — external-command spawning under
# MSYS/Cygwin parents.
#
# Reported: `niu -c 'printf "a\nb\n" | sort'` (and cut/rev/xargs/seq/tr
# pipelines) fail with rc=126 "Unknown error" or silent empty rc=0 when
# niu runs under a Git-Bash/MSYS2/Cygwin parent, while the same commands
# work from Python or WinuxCmd.
#
# Run this script FROM the parent shell being investigated:
#   Git Bash:  bash scripts/probe-msys-spawn.sh path/to/niu.exe
#   MSYS2:     bash scripts/probe-msys-spawn.sh path/to/niu.exe
#   Cygwin:    bash scripts/probe-msys-spawn.sh path/to/niu.exe
#   cmd:       git-bash scripts/probe-msys-spawn.sh C:\...\niu.exe
#
# Each case prints rc + captured stdout/stderr. With the rubash
# posix_errors fix, a failed spawn now reports the real Win32 code
# (e.g. "sort: The parameter is incorrect. (os error 87)") — capture and
# attach to the issue.

NIU="${1:-niu.exe}"

parent_env() {
    echo "=== parent env ==="
    echo "shell=$0"
    env | grep -iE '^(MSYSTEM|MSYS|CYGWIN|CHERE|SHELL|TERM|OSTYPE|COMSPEC|SystemRoot|WINDIR|PATHEXT)=' | sort
    echo "PATH entries:"
    echo "$PATH" | tr ':' '\n' | head -20
    echo "=================="
}

run_case() {
    local label="$1" script="$2"
    local out err rc
    out="$("$NIU" -c "$script" 2>/tmp/niu141-err.$$)"
    rc=$?
    err="$(cat /tmp/niu141-err.$$)"
    printf '%-28s rc=%-3s out=%-22q err=%q\n' "$label" "$rc" "$out" "$err"
}

parent_env
echo "NIU=$NIU"
"$NIU" --version | head -3
echo "--- cases ---"
run_case "printf|sort"        'printf "a\nb\n" | sort'
run_case "printf|cut"         'printf "abc\n" | cut -c1'
run_case "printf|rev"         'printf "a\nb\n" | rev'
run_case "printf|xargs"       'printf "1\n2\n" | xargs -n1 echo'
run_case "herestring-cut"     'cut -d: -f2 <<< "a:b:c"'
run_case "seq|tr"             'seq 1 3 | tr "\n" " "'
run_case "plain-sort-stdin"   'sort <<EOF2
b
a
EOF2'
run_case "builtin-only"       'echo builtin-ok'
run_case "explicit-git-sort"  '"D:/Git/usr/bin/sort.exe" </dev/null && echo spawn-ok'
run_case "type-sort"          'type sort; type cut'
rm -f /tmp/niu141-err.$$
echo "--- done ---"
