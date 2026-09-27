#!/usr/bin/env bash
# Imperatorium, from nothing to running, on Linux and macOS.
#
# Checks for Python 3.9+ (with venv), git and Claude Code, offers to install
# Claude Code with Anthropic's own installer, clones the repository - or
# updates it if it is already there - and hands over to setup_machine.py,
# which asks the rest. Python and git come from your package manager; this
# says which command to run rather than calling sudo for you.
#
#   ./bootstrap.sh                 # from a checkout you already have
#   curl -fsSL https://raw.githubusercontent.com/hypertetrahedron/imperatorium/main/bootstrap.sh | bash
#   IMPERATORIUM_DIR=~/code/imperatorium bash bootstrap.sh --yes
#
# Any arguments are passed on to setup_machine.py (e.g. --yes, --voice).
set -euo pipefail

REPO="https://github.com/hypertetrahedron/imperatorium.git"
DIR="${IMPERATORIUM_DIR:-$HOME/imperatorium}"
YES=0
for a in "$@"; do
    case "$a" in --yes|-y) YES=1 ;; esac
done

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
# -r is not enough: with no controlling terminal /dev/tty exists but will not open.
have_tty() { (: < /dev/tty) 2>/dev/null; }

confirm() {
    [ "$YES" = 1 ] && return 0
    # Piped through `curl | bash`, stdin is the script: ask the terminal.
    have_tty || return 1
    printf '%s [Y/n] ' "$1" > /dev/tty
    read -r answer < /dev/tty || return 1
    case "$answer" in ""|y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

pkg_hint() {
    if command -v apt-get >/dev/null 2>&1; then say "  sudo apt-get install -y $1"
    elif command -v dnf >/dev/null 2>&1; then say "  sudo dnf install -y $2"
    elif command -v pacman >/dev/null 2>&1; then say "  sudo pacman -S $2"
    elif command -v brew >/dev/null 2>&1; then say "  brew install $3"
    else say "  install $3 with your package manager"
    fi
}

say "Imperatorium bootstrap"
say ""

# 1. Python 3.9+ with venv
PY=""
for c in python3 python; do
    if command -v "$c" >/dev/null 2>&1 &&
       "$c" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 9) else 1)' 2>/dev/null; then
        PY="$c"; break
    fi
done
if [ -z "$PY" ]; then
    say "Python 3.9+ is required. Install it, then run this again:"
    pkg_hint "python3 python3-venv" "python3" "python@3.12"
    exit 1
fi
if ! "$PY" -c 'import venv, ensurepip' 2>/dev/null; then
    say "Python's venv module is missing. Install it, then run this again:"
    pkg_hint "python3-venv" "python3" "python@3.12"
    exit 1
fi
say "ok  python: $("$PY" --version 2>&1)"

# 2. git
if ! command -v git >/dev/null 2>&1; then
    say "git is required. Install it, then run this again:"
    pkg_hint "git" "git" "git"
    exit 1
fi
say "ok  git"

# 3. Claude Code
export PATH="$HOME/.local/bin:$PATH"
if ! command -v claude >/dev/null 2>&1; then
    if confirm "Claude Code is missing. Install it with Anthropic's installer (curl -fsSL https://claude.ai/install.sh | bash)?"; then
        curl -fsSL https://claude.ai/install.sh | bash
        command -v claude >/dev/null 2>&1 || die "Claude Code installed but not on PATH; open a new shell and run this again"
        say "Run \`claude\` once and log in before using Imperatorium."
    else
        die "Claude Code is required: https://claude.com/claude-code"
    fi
fi
say "ok  claude"

# 4. The checkout
HERE="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)"
if [ -n "$HERE" ] && [ -f "$HERE/setup_machine.py" ]; then
    DIR="$HERE"
    say "ok  checkout: $DIR"
elif [ -d "$DIR/.git" ]; then
    say "updating $DIR"
    git -C "$DIR" pull --ff-only
else
    say "cloning into $DIR"
    git clone "$REPO" "$DIR"
fi

# 5. Everything else is setup_machine.py's job. Give it the terminal back so
#    it can ask its questions even when this script arrived through a pipe.
cd "$DIR"
if [ ! -t 0 ] && have_tty; then
    exec "$PY" setup_machine.py "$@" < /dev/tty
fi
exec "$PY" setup_machine.py "$@"
