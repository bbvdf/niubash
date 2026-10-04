#!/usr/bin/env bash
# Fixture modeled on junegunn/fzf-git.sh (MIT): single script at the
# upstream root (`fzf-git.sh`, audited 2026-10-02, wt61 pattern). Widget
# functions only — upstream binds them through fzf's own key-bindings
# channel; nothing binds at source time.
fzf_git_fixture_imported=1

fzf_git_files() {
  # Upstream pipes git ls-files through fzf; the binary comes from the
  # user's package manager (download retraction).
  return 0
}
