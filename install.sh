#!/bin/sh
# Install Imperatorium on Linux or macOS in one line:
#
#   curl -fsSL https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.sh | sh
#
# Downloads the imperatorium binary for this machine from the release, checks
# its SHA-256 against the release's SHA256SUMS, puts it in ~/.local/bin, and
# runs `imperatorium install`. No root needed. Arguments go on to
# `imperatorium install` (curl ... | sh -s -- --dry-run).
#
#   IMPERATORIUM_CHANNEL   stable (default) or preview
#   IMPERATORIUM_RELEASES  another release folder (a mirror, or a test server)
set -eu

repo="https://github.com/hypertetrahedron/imperatorium/releases"
channel="${IMPERATORIUM_CHANNEL:-stable}"
if [ -n "${IMPERATORIUM_RELEASES:-}" ]; then
    base="${IMPERATORIUM_RELEASES%/}"
elif [ "$channel" = "preview" ]; then
    base="$repo/download/preview"
else
    base="$repo/latest/download"
fi

case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) echo "imperatorium: no build for $(uname -m)" >&2; exit 1 ;;
esac
case "$(uname -s)" in
    Linux) triple="$arch-unknown-linux-musl" ;;
    Darwin) triple="$arch-apple-darwin" ;;
    *) echo "imperatorium: no build for $(uname -s)" >&2; exit 1 ;;
esac
file="imperatorium-$triple"

fetch() {
    if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
    else wget -qO "$2" "$1"; fi
}
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

dir="$HOME/.local/bin"
mkdir -p "$dir"
tmp="$dir/.imperatorium.download"
sums="$dir/.imperatorium.SHA256SUMS"
echo "Imperatorium: reading $base/SHA256SUMS"
fetch "$base/SHA256SUMS" "$sums"
want=$(grep " $file\$" "$sums" | cut -d' ' -f1 || true)
rm -f "$sums"
if [ -z "$want" ]; then
    echo "imperatorium: this release has no build for $triple" >&2
    exit 1
fi
echo "Imperatorium: downloading $file"
fetch "$base/$file" "$tmp"
got=$(sha256 "$tmp")
if [ "$got" != "$want" ]; then
    rm -f "$tmp"
    echo "imperatorium: SHA-256 mismatch for $file: got $got, expected $want. Nothing was installed." >&2
    exit 1
fi
echo "   SHA-256 checked: $got"
chmod 755 "$tmp"
mv -f "$tmp" "$dir/imperatorium"

# Piped into sh, stdin is this script: the questions are asked on the
# terminal instead, when there is one.
if [ -t 0 ]; then
    exec "$dir/imperatorium" install "$@"
elif (exec < /dev/tty) 2>/dev/null; then
    exec "$dir/imperatorium" install "$@" < /dev/tty
else
    # No terminal at all (a script, CI): the defaults and the flags decide.
    exec "$dir/imperatorium" install "$@" < /dev/null
fi
