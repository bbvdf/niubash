#!/usr/bin/env bash
# Fixture bash-it-shaped loader tree for the source adapter smoke
# (tests/plugin_assets.rs). The layout mirrors the corpus checkout at
# D:/repo/rubash/target-ecosys/repos/bash-it: this file plus
# lib/composure.bash is the adapter's layout fingerprint; components live
# in <aliases|plugins|completion>/available/ and activate through
# enabled/<priority>---<file> entries, exactly what scripts/reloader.bash
# reads upstream.
cite() { :; }
about-plugin() { :; }
for _f in "$BASH_IT/enabled"/*.bash; do
  [ -r "$_f" ] && . "$_f"
done
unset _f
if [ -n "${BASH_IT_THEME:-}" ]; then
  . "${BASH_IT}/themes/${BASH_IT_THEME}/${BASH_IT_THEME}.theme.bash"
fi
