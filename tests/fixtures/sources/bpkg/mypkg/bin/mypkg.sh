#!/usr/bin/env bash
# Fixture bpkg package script (listed in bpkg.json "scripts").
. "${BPKG_ROOT:-.}/lib/helper.sh"
mypkg() { printf 'mypkg:%s\n' "$(mypkg_helper "$@")"; }
