#!/usr/bin/env bash
# Fixture oh-my-bash-shaped loader tree for the source adapter smoke
# (tests/plugin_sources.rs). The layout mirrors the corpus checkout at
# D:/repo/rubash/target-ecosys/repos/oh-my-bash: this file at the tree root
# is the adapter's layout fingerprint; themes live in
# themes/<name>/<name>.theme.sh; lib helpers in lib/.
case $- in
  *i*) ;;
  *) return ;;
esac
if [ -z "${BASH_VERSION-}" ]; then
  printf '%s\n' 'oh-my-bash: This is not a Bash.' >&2
  return 1
fi
_omb_module_require() { return 0; }
. "${OSH}/lib/utils.sh"
if [ -n "${OSH_THEME:-}" ]; then
  . "${OSH}/themes/${OSH_THEME}/${OSH_THEME}.theme.sh"
fi
