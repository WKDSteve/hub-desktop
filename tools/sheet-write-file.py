#!/usr/bin/env python3
"""Write one file for a test sheet: `sheet-write-file.py <path> <text>`.

The `prepare` step of an app in `.testing.lua` (testing.nvim docs/SHEETS.md) runs it with a path in the folder of the
run (`${temp}`), so the app finds that file when it starts, for example a config.json it cannot use. The folders of the
path are made; the text is written as it is, UTF-8, without a line break added.
"""

import sys
from pathlib import Path


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: sheet-write-file.py <path> <text>", file=sys.stderr)
        return 2
    path = Path(argv[1])
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(argv[2].encode("utf-8"))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
