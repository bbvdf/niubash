#!/usr/bin/env bash
# Fixture oh-my-bash-shaped tree carrying a framework-dependent plugin
# (§14.6.4 bashmarks row). Layout mirrors the corpus checkout at
# D:/repo/rubash/target-ecosys/repos/oh-my-bash: root oh-my-bash.sh is the
# fingerprint, plugins/<name>/<name>.plugin.sh are the assets.
case $- in
  *i*) ;;
  *) return ;;
esac
if [ -z "${BASH_VERSION-}" ]; then
  printf '%s\n' 'oh-my-bash: This is not a Bash.' >&2
  return 1
fi

# Corpus semantics: the loader defines _omb_module_require and every
# module (lib, plugin, theme) is loaded through it.
_omb_module_require() {
  case "$1" in
    util) . "${OSH}/lib/utils.sh" ;;
    plugin) . "${OSH}/plugins/$2/$2.plugin.sh" ;;
    *) return 0 ;;
  esac
}

_omb_module_require util
if [ -n "${OSH_THEME:-}" ]; then
  . "${OSH}/themes/${OSH_THEME}/${OSH_THEME}.theme.sh"
fi
for __omb_plugin in ${plugins[@]-}; do
  _omb_module_require plugin "$__omb_plugin"
done
unset __omb_plugin
