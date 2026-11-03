#!/usr/bin/env python3
"""Fail when the tests of this repository left folders or files of their own in the temp dir.

Tests that make their own folder in the temp dir and do not remove it (a failed test, a panic, a
forgotten cleanup) fill it for good. Every test gets its folder from a helper that removes it
(`appkit_scratch::Scratch` for the Rust tests, scripts/test-install.* for the install tests); this is
the check that none escaped.

    python tools/check-temp-leftovers.py [--dir DIR] [--prefix PREFIX]... [--strict [--allow NAME]...]

DIR is the temp dir to look in; by default the one of this process (TEMP/TMP on Windows, TMPDIR
elsewhere). It must exist: a typo in the job that points TEMP at a folder of its own would otherwise
read as "nothing left".

Counted are the entries whose name starts with one of the PREFIXes; by default `hub-desktop-` (the
install tests: `hub-desktop-test-install-*`) and `appkit-` (the folders of `Scratch` in the Rust tests).
Naming --prefix replaces that list.

On a machine that has run other things the shared temp dir holds old leftovers and other jobs'
folders, so CI points TEMP, TMP and TMPDIR of a job at a folder of its own, empty at the start. With
--strict every entry in it counts except the ones named by --allow, whatever a test called its folder.

Exit 0: nothing left. Exit 1: the names are listed (at most 30, then the number). Exit 2: wrong
arguments, or DIR could not be read (a missing DIR included).
"""

import argparse
import os
import sys
import tempfile

SHOWN = 30
DEFAULT_PREFIXES = ["hub-desktop-", "appkit-"]


def leftovers(directory, prefixes, strict, allow):
    """The entries of `directory` that count, sorted. `directory` must exist."""
    names = os.listdir(directory)
    if strict:
        found = [n for n in names if n not in allow]
    else:
        found = [n for n in names if any(n.startswith(p) for p in prefixes)]
    return sorted(found)


class _Parser(argparse.ArgumentParser):
    def error(self, message):
        self.print_usage(sys.stderr)
        print(message, file=sys.stderr)
        raise SystemExit(2)


def main(argv):
    parser = _Parser(prog="check-temp-leftovers.py")
    parser.add_argument("--dir", default=None)
    parser.add_argument("--prefix", action="append", default=[])
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--allow", action="append", default=[])
    args = parser.parse_args(argv)
    if args.allow and not args.strict:
        parser.error("--allow only goes with --strict")
    if args.strict and args.prefix:
        parser.error("--prefix does not go with --strict (it counts every entry)")
    directory = args.dir if args.dir is not None else tempfile.gettempdir()
    prefixes = args.prefix or DEFAULT_PREFIXES
    try:
        found = leftovers(directory, prefixes, args.strict, args.allow)
    except OSError as e:
        print(f"cannot read {directory}: {e}", file=sys.stderr)
        return 2
    what = "entries" if args.strict else ", ".join(f"{p}*" for p in prefixes)
    if not found:
        print(f"nothing ({what}) left in {directory}")
        return 0
    print(f"{len(found)} {what} left in {directory}:", file=sys.stderr)
    for name in found[:SHOWN]:
        print(f"  {name}", file=sys.stderr)
    if len(found) > SHOWN:
        print(f"  ... and {len(found) - SHOWN} more", file=sys.stderr)
    print(
        "a test made a folder in the temp dir and did not remove it; get the folder from the test helper",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
