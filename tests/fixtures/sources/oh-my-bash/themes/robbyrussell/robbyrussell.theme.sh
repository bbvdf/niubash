# Fixture theme in the corpus layout (themes/<name>/<name>.theme.sh).
# The load shape mirrors the byte-verified rubash regression canary
# eco-omb-theme-ps1.sh: a theme function composes PS1 with a lib helper.
_omb_branch=''
omb_theme_robbyrussell() {
  _omb_util_print "loading robbyrussell"
  PS1="➜ ${_omb_branch}prompt-robbyrussell "
}
omb_theme_robbyrussell
