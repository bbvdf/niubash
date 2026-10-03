#!/usr/bin/env bash
# Fixture: a single-file wild bash plugin (§14.6.1 base layer) — no
# framework, no manifest, one sourceable file. Modeled on gist-style
# single-file plugins.
spark() { printf 'spark! %s\n' "$*"; }
