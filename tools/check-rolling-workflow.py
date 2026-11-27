#!/usr/bin/env python3
"""Fail when the rolling build loses one of the guards of its Linux and macOS legs.

The rolling build (.github/workflows/rolling.yml) builds Windows on the self-hosted runner and
Linux and macOS on hosted runners, the macOS leg experimental. Three things went wrong or nearly so
and are checked here as text, since a workflow cannot be run locally:

  * publish must run when the experimental leg was cancelled (a timeout makes the result of the
    matrix `cancelled`) but not when a required leg failed, and never from a cancelled run:
    its `if` holds `!cancelled()` and `needs.build.result != 'failure'`, and no `always()`;
  * the artifacts are named by the commit (`needs.prepare.outputs.short`) on upload and download:
    a re-run whose prepare picks a newer commit must not publish an older leg's file;
  * an archive listing is never piped into `grep -q`: GNU tar dies of SIGPIPE when a line follows
    the match, and pipefail turns that into a red Linux leg (the first run failed so).

    python tools/check-rolling-workflow.py [PATH]

Exit 0: all guards are there. Exit 1: the missing ones are listed. Exit 2: the file cannot be read.
"""

import re
import sys


def job(text: str, name: str) -> str:
    """The lines of one job: from `  <name>:` up to the next job or the end."""
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)", text, re.M | re.S)
    return match.group(1) if match else ""


def problems(text: str) -> list[str]:
    found = []
    publish = job(text, "publish")
    if not publish:
        return ["no publish job"]
    cond = re.search(r"^    if: (.+)$", publish, re.M)
    cond = cond.group(1) if cond else ""
    if "!cancelled()" not in cond:
        found.append("publish: its if lacks !cancelled()")
    if "needs.build.result != 'failure'" not in cond:
        found.append("publish: its if lacks needs.build.result != 'failure'")
    if "always()" in cond:
        found.append("publish: its if uses always() (that includes a cancelled run)")
    build = job(text, "build")
    upload = re.search(r"^\s+name: (rolling-.*)$", build, re.M)
    if not upload or "needs.prepare.outputs.short" not in upload.group(1):
        found.append("build: the artifact name is not bound to needs.prepare.outputs.short")
    download = re.search(r"^\s+pattern: (rolling-.*)$", publish, re.M)
    if not download or "needs.prepare.outputs.short" not in download.group(1):
        found.append("publish: the artifact pattern is not bound to needs.prepare.outputs.short")
    for number, line in enumerate(text.splitlines(), 1):
        if re.search(r"tar\s+-t\S*.*\|\s*grep\s+-\S*q", line):
            found.append(f"line {number}: an archive listing is piped into grep -q")
    return found


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else ".github/workflows/rolling.yml"
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except OSError as error:
        print(f"check-rolling-workflow: cannot read {path}: {error}", file=sys.stderr)
        return 2
    found = problems(text)
    for problem in found:
        print(f"{path}: {problem}")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
