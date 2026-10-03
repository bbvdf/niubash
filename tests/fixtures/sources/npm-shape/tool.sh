#!/usr/bin/env bash
# Fixture ordinary shell file inside an npm-shaped tree. The tree must NOT
# match the bpkg fingerprint (package.json "scripts" is an object); with no
# manager fingerprint it falls through to the wild file-source layer.
npm_shape_tool() { printf 'npm-shape-tool\n'; }
