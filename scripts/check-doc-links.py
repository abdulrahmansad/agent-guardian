from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[1]
LINK = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
FENCE = re.compile(r"^\s*(```+|~~~+)")


def check_file(markdown_file: Path) -> list[str]:
    errors: list[str] = []
    fence_marker: str | None = None
    for line_number, line in enumerate(markdown_file.read_text(encoding="utf-8").splitlines(), 1):
        fence = FENCE.match(line)
        if fence:
            marker = fence.group(1)[0]
            fence_marker = None if fence_marker == marker else marker
            continue
        if fence_marker:
            continue
        for match in LINK.finditer(line):
            destination = match.group(1).strip()
            if destination.startswith("<") and ">" in destination:
                destination = destination[1 : destination.index(">")]
            else:
                destination = destination.split(maxsplit=1)[0]
            if destination.startswith(("http://", "https://", "mailto:", "codex:")):
                continue
            local_path = unquote(destination.split("#", 1)[0].split("?", 1)[0])
            if not local_path:
                continue
            target = (markdown_file.parent / local_path).resolve()
            if not target.exists():
                errors.append(f"{markdown_file.relative_to(ROOT)}:{line_number}: missing {destination}")
    return errors


def main() -> int:
    generated_directories = {".git", "target", "dist"}
    markdown_files = sorted(
        path
        for path in ROOT.rglob("*.md")
        if not generated_directories.intersection(path.parts)
    )
    errors = [error for markdown_file in markdown_files for error in check_file(markdown_file)]
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Checked local Markdown links in {len(markdown_files)} files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
