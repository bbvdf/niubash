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
# Consume the niu-managed selection arrays the way the real loader's
# _omb_module_require_{plugin,alias,completion} chain does (corpus
# oh-my-bash.sh), so an enabled asset actually loads interactively.
for __niu_plug in ${plugins[@]+"${plugins[@]}"}; do
  if [ -r "${OSH}/plugins/${__niu_plug}/${__niu_plug}.plugin.sh" ]; then
    . "${OSH}/plugins/${__niu_plug}/${__niu_plug}.plugin.sh"
  fi
done
for __niu_alias in ${aliases[@]+"${aliases[@]}"}; do
  if [ -r "${OSH}/aliases/${__niu_alias}.aliases.sh" ]; then
    . "${OSH}/aliases/${__niu_alias}.aliases.sh"
  fi
done
for __niu_comp in ${completions[@]+"${completions[@]}"}; do
  if [ -r "${OSH}/completions/${__niu_comp}.completion.sh" ]; then
    . "${OSH}/completions/${__niu_comp}.completion.sh"
  fi
done
unset -v __niu_plug __niu_alias __niu_comp
