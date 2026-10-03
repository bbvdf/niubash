# Hooks

Status (2026-10, niubash#161): the **niu-native named-hook registry described
by the previous version of this page is not implemented**. The
`niubash_add_*_hook` registration helpers were part of the `oh-my-niu`
framework, which retired with the built-in plugin/theme stack
(niubash#145); there is currently no runtime registration path — the host
loads an empty hook configuration, so the internal `run_*_hooks` runners
have nothing to run. Whether (and how) hooks return for the
external-ecosystem world — e.g. registration builtins over the shell's own
function namespace — is an open owner ruling.

What exists today is the **bash-native hook surface**. External frameworks
(oh-my-bash, starship, your own rc) already use it, and it behaves exactly
as in GNU bash:

| Lifecycle point | Bash-native mechanism | Notes |
| --- | --- | --- |
| before every prompt | `PROMPT_COMMAND` (function or string) | runs each prompt cycle, before the prompt is rendered |
| before a command executes | `PS0` (expanded, printed) | after you press Enter, before execution |
| prompt identity | `PS1` assignment | claims the prompt slot (defaults-as-floor: the product floor never fights an active claim; `unset PS1` restores it) |
| directory change | `chpwd`-style: `cd` hooks via `PROMPT_COMMAND` + `PWD` diffing in your own function | no dedicated `chpwd` hook yet |
| signals / exit | `trap 'handler' INT TERM EXIT DEBUG ERR` | full bash trap semantics, via the engine |
| terminal title | write OSC escapes from `PROMPT_COMMAND` (or set `NIU_TITLE`) | `NIU_TITLE`, when set, is the value title logic sees |

Example — the starship shape (a `PROMPT_COMMAND` hook that claims `PS1` in
the same render cycle):

```bash
# ~/.niubashrc
starship_precmd() { PS1="$(starship prompt)"; }
PROMPT_COMMAND="starship_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
eval "$(starship init bash)"
```

Example — time every command from `PROMPT_COMMAND` (start-of-command stamp
plus end-of-command report):

```bash
# ~/.niubashrc
__niu_t0=$SECONDS
__niu_timer_report() {
  echo "last command took $((SECONDS - __niu_t0))s"
  __niu_t0=$SECONDS
}
PROMPT_COMMAND="__niu_timer_report${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
```

Example — `PS0` is expanded and printed right after you press Enter, before
execution (same expansion rules as `PS1`):

```bash
PS0='[running...] '
```

The internal runners (`precmd`/`preexec`/`title`/… cycle calls in
`shell.rs`) are retained: they carry the live machinery above
(`PROMPT_COMMAND` execution, `PS0` rendering, `PS1` claim/release sync,
title resolution) and keep the call sites for the eventual
external-ecosystem hook model. They are intentionally not a user-facing
surface until that model is ruled on.
