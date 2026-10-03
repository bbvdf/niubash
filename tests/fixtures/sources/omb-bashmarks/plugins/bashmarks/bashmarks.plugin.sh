#!bash
# Fixture modeled on oh-my-bash plugins/bashmarks (corpus shape): the
# plugin declares its framework dependency up front — sourcing it without
# the oh-my-bash loader must fail exactly like it would under GNU bash
# (command not found: _omb_module_require). No shim may mask that (§14.4).
_omb_module_require 'util'

export BOOKMARKS_HOME="${BOOKMARKS_HOME:-$HOME/.bookmarks}"

_mark_save() { printf '%s\n' "$2" > "${BOOKMARKS_HOME}/$1"; }
mark() { mkdir -p "${BOOKMARKS_HOME}"; _mark_save "$1" "$PWD"; }
jump() { cd "$(cat "${BOOKMARKS_HOME}/$1")" || return 1; }
marks() { ls -1 "${BOOKMARKS_HOME}"; }
unmark() { rm -f "${BOOKMARKS_HOME}/$1"; }
