#! bash oh-my-bash.module
# Fixture bashmarks-shaped plugin (wt44/niu365, loader-fidelity guard).
# Mirrors the non-deprecated surface of the real
# plugins/bashmarks/bashmarks.plugin.sh from the corpus checkout at
# D:/repo/rubash/target-ecosys/repos/oh-my-bash (BSD-3): the `bm`
# dispatcher plus the current-name functions. The real plugin's
# deprecated-interface block (`_omb_deprecate_declare 20000 SDIRS …`,
# `_omb_deprecate_function 20000 _echo_usage …`, `_omb_util_print`) is
# deliberately NOT reproduced here: those are framework-lib calls that the
# REAL oh-my-bash loader provides through its own lib chain, and niubash
# ships no shim layer for them (owner ruling 2026-10-02, design doc
# appendix D — framework assets load through the framework loader; a
# manually sourced framework plugin fails with the same missing-function
# errors as under GNU bash). This fixture stays lib-free so the guard test
# proves the loader path itself, independent of framework-lib coverage.

function _bashmarks_usage {
  printf '%s\n' 'USAGE:'
  printf '%s\n' 'bm -a <name> - Saves the current directory as "name"'
  printf '%s\n' 'bm -l         - Lists all available bookmarks'
}

function _bashmarks_save {
  local name=$1
  BASHMARKS_STORE="${BASHMARKS_STORE-}${name}=$PWD"$'\n'
}

function _bashmarks_list_names {
  [[ ${BASHMARKS_STORE-} ]] && printf '%s\n' "${BASHMARKS_STORE%$'\n'}"
}

function bm {
  local option=$1
  case $option in
    -a) _bashmarks_save "$2" ;;
    -l) _bashmarks_list_names ;;
    -h) _bashmarks_usage ;;
    *)  _bashmarks_usage >&2; return 1 ;;
  esac
}
