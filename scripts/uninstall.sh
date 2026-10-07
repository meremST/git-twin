#!/bin/sh
set -eu

usage() {
    printf '%s\n' \
        'Usage: uninstall.sh [--cargo | --help]' \
        'Remove gitt and git-tui from ~/bin (manual installation by default).' \
        'Use --cargo for an installation made with cargo install --root "$HOME".' \
        'Configuration, drafts, caches and repositories are preserved. Do not use sudo.'
}

mode=manual
if [ "$#" -gt 1 ]; then
    usage >&2
    exit 2
fi
case "${1:-}" in
    '') ;;
    --cargo) mode=cargo ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
esac

case "${HOME:-}" in
    ''|/|//*) printf '%s\n' 'Error: HOME must be an absolute user home directory, not /.' >&2; exit 1 ;;
    /*) ;;
    *) printf '%s\n' 'Error: HOME must be an absolute user home directory.' >&2; exit 1 ;;
esac

if [ "$mode" = cargo ]; then
    if ! command -v cargo >/dev/null 2>&1; then
        printf '%s\n' 'Error: Cargo is required to remove its installation records. Restore Cargo and retry.' >&2
        exit 1
    fi
    cargo uninstall --root "$HOME" git-twin
else
    for name in gitt git-tui; do
        binary="$HOME/bin/$name"
        if [ -d "$binary" ] && [ ! -L "$binary" ]; then
            printf 'Error: refusing to remove directory %s\n' "$binary" >&2
            exit 1
        fi
    done
    for name in gitt git-tui; do
        binary="$HOME/bin/$name"
        if [ -e "$binary" ] || [ -L "$binary" ]; then
            rm -- "$binary"
            printf 'Removed %s\n' "$binary"
        else
            printf 'Not installed: %s\n' "$binary"
        fi
    done
fi
printf '%s\n' 'Configuration, drafts, caches and repositories were preserved.'
