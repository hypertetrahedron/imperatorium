#!/bin/sh
# impctl - run impctl.py from anywhere: ./impctl.sh help
#
# Uses the project's venv when there is one (the test suites need its Agent
# SDK), otherwise the first Python on PATH. Follows a symlink back to the
# checkout, so it can be linked into ~/bin as plain `impctl`.

self=$0
while [ -L "$self" ]; do
    link=$(readlink "$self")
    case $link in
        /*) self=$link ;;
        *) self=$(dirname "$self")/$link ;;
    esac
done
here=$(cd "$(dirname "$self")" && pwd)

for py in "$here/.venv/bin/python" "$here/.venv/Scripts/python.exe"; do
    [ -x "$py" ] && exec "$py" "$here/impctl.py" "$@"
done
for py in python3 python; do
    command -v "$py" >/dev/null 2>&1 && exec "$py" "$here/impctl.py" "$@"
done
echo "impctl: no Python found - install Python 3.9+ or run setup_machine.py" >&2
exit 1
