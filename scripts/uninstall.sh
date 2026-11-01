#!/bin/sh
# Remove hub-desktop for the current user (Linux, macOS), without administrator rights.
#
#   scripts/uninstall.sh [--purge-settings]
#
# It asks the installed program to take its place in the system away (`hub-desktop uninstall`:
# stop the hub, the login entry, the launcher), then removes the program file from the install
# folder ($HUB_DESKTOP_INSTALL_DIR, else ${XDG_DATA_HOME:-~/.local/share}/hub-desktop/bin).
# Your settings stay unless you ask: --purge-settings removes the files of the app in the
# settings folder. Other apps are never touched. A second run finds nothing to do
# and says so; it is not an error.
#
# If taking the system entries away fails, the program is NOT removed (a login entry that points
# at a missing program is worse than a program that is still there): read the message and run it
# again.

set -eu

PURGE=""

say() { printf '%s\n' "$*"; }
fail() {
    printf 'uninstall.sh: %s\n' "$*" >&2
    exit 1
}

usage() {
    sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --purge-settings)
            PURGE="--purge-settings"
            shift
            ;;
        --help | -h)
            usage
            exit 0
            ;;
        *)
            fail "unknown option: $1 (try --help)"
            ;;
    esac
done

if [ "$(id -u)" = "0" ]; then
    fail "do not run this as root: it removes the installation of one user"
fi

if [ -n "${HUB_DESKTOP_INSTALL_DIR:-}" ]; then
    dir="$HUB_DESKTOP_INSTALL_DIR"
else
    dir="${XDG_DATA_HOME:-$HOME/.local/share}/hub-desktop/bin"
fi

program=""
for candidate in "$dir/hub-desktop" "$dir/hub-desktop.exe"; do
    if [ -f "$candidate" ]; then
        program="$candidate"
        break
    fi
done

if [ -z "$program" ]; then
    say "nothing to remove: no hub-desktop in $dir"
    exit 0
fi

# The program takes the system entries away itself (it knows where they are). A program from before
# the command existed answers with its usage text: then the older commands do what they can.
# shellcheck disable=SC2086 # the flag is one word or empty
if out="$("$program" uninstall $PURGE 2>&1)"; then
    printf '%s
' "$out"
else
    printf '%s
' "$out"
    case "$out" in
        *usage:*)
            say "this program is older than the uninstall command: stopping it and taking away the login entry the older way"
            "$program" stop || fail "hub-desktop stop failed (see above): the program stays in $dir"
            "$program" autostart off || fail "hub-desktop autostart off failed (see above): the program stays in $dir"
            say "(a link or launcher it made, and its settings, are not touched by an older program: delete them by hand)"
            ;;
        *)
            fail "hub-desktop uninstall failed (see above): the program stays in $dir"
            ;;
    esac
fi

# Only the files the installer puts there; a folder that holds anything else stays.
# A file that cannot be removed is an error, not a "removed" (rule RS-07).
rm -f "$program" "$dir/hub-desktop.new" "$dir/hub-desktop.exe.new" || fail "cannot remove $program"
[ ! -e "$program" ] || fail "$program is still there"
rmdir "$dir" 2>/dev/null || true
# The parent only when it is the folder the installer makes (never a folder you chose).
parent="$(dirname "$dir")"
if [ "$(basename "$dir")" = "bin" ] && [ "$(basename "$parent")" = "hub-desktop" ]; then
    rmdir "$parent" 2>/dev/null || true
fi

say ""
say "removed $program"
if [ -z "$PURGE" ]; then
    say "your settings were kept (--purge-settings removes them)"
fi
