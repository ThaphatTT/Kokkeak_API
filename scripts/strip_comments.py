"""Strip comments per §22.2 of AGENTS.md.

Removes Rust `///`, `//!`, `//`, `/* */`; SQL `--`, `/* */`; TOML/YAML `#`.
Preserves string literals, char literals, lifetimes, raw strings, attributes.

Tokenizes by tracking `in_string`/`in_char`/`in_block_comment` state — avoids
regex pitfalls (URLs in strings, `://` false positives, attribute `#[...]`).

Run:
    python scripts/strip_comments.py            # apply
    python scripts/strip_comments.py --dry-run  # report only
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path
from typing import Callable


def _is_rust_char_literal(content: str, start: int) -> int:
    """If content[start:] starts a Rust char literal, return its length.

    Otherwise return 0. Handles:
      - '\"'  (single non-identifier char)
      - '\\'', '\\\\', '\\n', '\\t', '\\r', '\\0', '\\xHH', '\\u{...}'
    Returns 0 for lifetimes ('a, 'static) since the char after `'` is a letter.
    """
    n = len(content)
    if start >= n or content[start] != "'":
        return 0
    if start + 1 >= n:
        return 0
    c1 = content[start + 1]
    if c1.isalpha() or c1 == "_":
        return 0
    if c1 == "\\":
        m = re.match(
            r"\\(['\"\\nrt0abfv]|x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f]+\})",
            content[start + 1 :],
        )
        if not m:
            return 0
        end_quote = start + 1 + m.end()
        if end_quote < n and content[end_quote] == "'":
            return end_quote + 1 - start
        return 0
    if start + 2 < n and content[start + 2] == "'":
        return 3
    return 0


def strip_rust(content: str) -> str:
    out: list[str] = []
    i = 0
    n = len(content)

    while i < n:
        c = content[i]
        nxt = content[i + 1] if i + 1 < n else ""

        if c == '"':
            out.append(c)
            i += 1
            while i < n:
                out.append(content[i])
                if content[i] == "\\" and i + 1 < n:
                    out.append(content[i + 1])
                    i += 2
                    continue
                if content[i] == '"':
                    i += 1
                    break
                i += 1
            continue

        if c == "/" and nxt == "/":
            i += 2
            while i < n and content[i] != "\n":
                i += 1
            continue

        if c == "/" and nxt == "*":
            i += 2
            while i + 1 < n:
                if content[i] == "*" and content[i + 1] == "/":
                    i += 2
                    break
                i += 1
            else:
                i = n
            continue

        if c == "'":
            clen = _is_rust_char_literal(content, i)
            if clen:
                out.append(content[i : i + clen])
                i += clen
                continue
            out.append(c)
            i += 1
            continue

        out.append(c)
        i += 1

    cleaned = "".join(out)
    cleaned = re.sub(r"\n{3,}", "\n\n", cleaned)
    cleaned = "\n".join(line.rstrip() for line in cleaned.split("\n"))
    return cleaned


def strip_sql(content: str) -> str:
    out: list[str] = []
    i = 0
    n = len(content)

    while i < n:
        c = content[i]
        nxt = content[i + 1] if i + 1 < n else ""

        if c == "'":
            out.append(c)
            i += 1
            while i < n:
                out.append(content[i])
                if content[i] == "'" and i + 1 < n and content[i + 1] == "'":
                    out.append(content[i + 1])
                    i += 2
                    continue
                if content[i] == "'":
                    i += 1
                    break
                i += 1
            continue

        if c == "-" and nxt == "-":
            i += 2
            while i < n and content[i] != "\n":
                i += 1
            continue

        if c == "/" and nxt == "*":
            i += 2
            while i + 1 < n:
                if content[i] == "*" and content[i + 1] == "/":
                    i += 2
                    break
                i += 1
            else:
                i = n
            continue

        out.append(c)
        i += 1

    return "".join(out)


def strip_hash_comments(content: str) -> str:
    """TOML / shell-style # comments (not inside strings)."""
    out: list[str] = []
    i = 0
    n = len(content)

    while i < n:
        c = content[i]
        nxt = content[i + 1] if i + 1 < n else ""

        if c in ('"', "'"):
            quote = c
            out.append(c)
            i += 1
            while i < n:
                out.append(content[i])
                if content[i] == "\\" and i + 1 < n:
                    out.append(content[i + 1])
                    i += 2
                    continue
                if content[i] == quote:
                    i += 1
                    break
                i += 1
            continue

        if c == "#":
            i += 1
            while i < n and content[i] != "\n":
                i += 1
            continue

        out.append(c)
        i += 1

    return "".join(out)


def strip_yaml(content: str) -> str:
    """Strip YAML `#` comments.

    Line-based: walks each line, tracks whether we're inside a double-quoted or
    single-quoted scalar that might span lines (rare in this project).
    """
    out_lines: list[str] = []

    for line in content.split("\n"):
        result: list[str] = []
        i = 0
        n = len(line)
        in_double = False
        in_single = False
        while i < n:
            c = line[i]
            if not in_double and not in_single:
                if c == '"':
                    in_double = True
                    result.append(c)
                    i += 1
                    continue
                if c == "'":
                    if i + 1 < n and line[i + 1] == "'":
                        in_single = True
                        result.append(c)
                        result.append(c)
                        i += 2
                        continue
                if c == "#":
                    break
                result.append(c)
                i += 1
                continue

            if in_double:
                result.append(c)
                if c == "\\" and i + 1 < n:
                    result.append(line[i + 1])
                    i += 2
                    continue
                if c == '"':
                    in_double = False
                i += 1
                continue

            if in_single:
                if c == "'" and i + 1 < n and line[i + 1] == "'":
                    result.append("'")
                    result.append("'")
                    i += 2
                    continue
                if c == "'":
                    in_single = False
                result.append(c)
                i += 1
                continue

        stripped_line = "".join(result).rstrip()
        out_lines.append(stripped_line)

    cleaned = "\n".join(out_lines)
    cleaned = re.sub(r"\n{3,}", "\n\n", cleaned)
    return cleaned


def process_file(
    path: Path,
    stripper: Callable[[str], str],
    dry_run: bool,
) -> bool:
    try:
        original = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        print(f"  SKIP (read error): {path}", file=sys.stderr)
        return False

    cleaned = stripper(original)
    if cleaned == original:
        return False

    if dry_run:
        print(f"  WOULD CLEAN: {path}")
    else:
        path.write_text(cleaned, encoding="utf-8")
        print(f"  CLEANED:    {path}")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description="Strip comments per §22.2")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print files that would change without modifying them",
    )
    parser.add_argument(
        "--base",
        default=r"C:\Users\crybo\Desktop\Develop\Kokkeak_API",
        help="project root to scan",
    )
    args = parser.parse_args()

    base = Path(args.base)
    if not base.exists():
        print(f"base path does not exist: {base}", file=sys.stderr)
        return 1

    skip_dirs = {"target", ".git", "graphify-out", "node_modules", ".vscode", ".zed"}

    rust_files: list[Path] = []
    sql_files: list[Path] = []
    toml_files: list[Path] = []
    yml_files: list[Path] = []

    for root, dirs, files in os.walk(base):
        dirs[:] = sorted(d for d in dirs if d not in skip_dirs)
        for name in files:
            p = Path(root) / name
            if name.endswith(".rs"):
                rust_files.append(p)
            elif name.endswith(".sql"):
                sql_files.append(p)
            elif name.endswith(".toml"):
                toml_files.append(p)
            elif name.endswith((".yml", ".yaml")):
                yml_files.append(p)

    plan = [
        ("Rust (.rs)", rust_files, strip_rust),
        ("SQL (.sql)", sql_files, strip_sql),
        ("TOML (.toml)", toml_files, strip_hash_comments),
        ("YAML (.yml)", yml_files, strip_yaml),
    ]

    total_changed = 0
    total_scanned = 0

    for label, paths, stripper in plan:
        changed_here = 0
        for p in paths:
            total_scanned += 1
            if process_file(p, stripper, args.dry_run):
                changed_here += 1
                total_changed += 1
        print(f"[{label}] scanned={len(paths)} changed={changed_here}")

    print()
    mode = "WOULD CHANGE" if args.dry_run else "CHANGED"
    print(f"{mode}: {total_changed} / {total_scanned} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
