#!/bin/sh
# Records docs/demo.gif with vhs (https://github.com/charmbracelet/vhs).
#
#   docs/record-demo.sh
#
# Runs in a throwaway HOME with its own minimal .zshrc and history turned off,
# so it never touches your real shell setup.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
for tool in vhs ttyd ffmpeg; do
  command -v "$tool" >/dev/null || { echo "needs $tool: brew install $tool" >&2; exit 1; }
done

cargo build --release --quiet --manifest-path "$root/Cargo.toml"

demo="$(mktemp -d)"
trap 'rm -rf "$demo"' EXIT INT TERM
mkdir -p "$demo/bin" "$demo/project"
cp "$root/target/release/aka" "$demo/bin/aka"

# A small repo so the git aliases have something to show.
(
  cd "$demo/project"
  git init -q -b main
  git config user.name demo && git config user.email demo@example.com
  echo "# my project" > README.md && git add README.md && git commit -qm "First commit"
  echo "draft" > notes.txt
)

cat > "$demo/.zshrc" <<EOF
unset HISTFILE
PROMPT='%F{blue}~/project%f %F{magenta}❯%f '
autoload -Uz compinit && compinit -i -d "$demo/.zcompdump"
EOF

# The tape is written literally (quotes, backticks and $1 must reach vhs as
# they are), then the two paths are filled in.
cat > "$demo/demo.tape.in" <<'EOF'
Output "@ROOT@/docs/demo.gif"
Set Shell "zsh"
Set FontSize 20
Set Width 1100
Set Height 560
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms
Env HOME "@DEMO@"
Env ZDOTDIR "@DEMO@"
Env AKA_HOME "@DEMO@/.config/aka"
Env PATH "@DEMO@/bin:@TOOLS@:/usr/bin:/bin"
Env HISTFILE ""

Hide
Type "cd @DEMO@/project && aka setup -y --shell zsh --no-completion >/dev/null 2>&1 && source @DEMO@/.config/aka/init.zsh && clear"
Enter
Sleep 500ms
Show

Type `aka add gs "git status -sb" -d "short status"`
Sleep 400ms Enter Sleep 1.2s
Type "gs"
Sleep 400ms Enter Sleep 1.5s

Type `aka add gs "git status"`
Sleep 400ms Enter Sleep 1.5s
Type "c"
Sleep 400ms Enter Sleep 1.2s

Type `aka add mkcd 'mkdir -p "$1" && cd "$1"'`
Sleep 400ms Enter Sleep 1s
Type `aka add gl 'git log --oneline -5' --tag git`
Sleep 400ms Enter Sleep 1s

Type "aka list"
Sleep 400ms Enter Sleep 2.5s

Type "aka rm gl"
Sleep 400ms Enter Sleep 1s
Type "aka undo"
Sleep 400ms Enter Sleep 1s
Type "gl"
Sleep 400ms Enter Sleep 2.5s
EOF
# vhs uses the tape's PATH to start ttyd and ffmpeg, so keep their folders on it.
tools="$(dirname "$(command -v ttyd)"):$(dirname "$(command -v ffmpeg)")"
sed -e "s|@DEMO@|$demo|g" -e "s|@ROOT@|$root|g" -e "s|@TOOLS@|$tools|g" "$demo/demo.tape.in" > "$demo/demo.tape"

vhs "$demo/demo.tape"
echo "Wrote $root/docs/demo.gif"
