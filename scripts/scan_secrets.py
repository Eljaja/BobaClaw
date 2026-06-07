#!/usr/bin/env python3
"""Lightweight secret-pattern scan for repository text files.

Dependency-free for minimal CI. Not a replacement for gitleaks.
"""

from __future__ import annotations

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]

SKIP_DIRS = {
    ".git",
    ".venv",
    "node_modules",
    "dist",
    "build",
    ".pytest_cache",
    "__pycache__",
    "target",
    "references",
}

SKIP_FILE_NAMES = {
    "config.local.yaml",
    ".env",
}

# Assignments to function calls (e.g. `let api_key = config.resolve_api_key()?`)
CODE_ASSIGNMENT = re.compile(r"=\s*[\w.]+\(")

# Rust parameter/type annotations (e.g. `token: CancellationToken`)
RUST_TYPE_ANNOTATION = re.compile(r":\s*[&]?[A-Z][A-Za-z0-9_]*")

SECRET_PATTERNS = [
    re.compile(r"-----BEGIN (RSA |DSA |EC |OPENSSH |PGP )?PRIVATE KEY-----"),
    re.compile(
        r"(?i)(api[_-]?key|secret|token|password)\s*[:=]\s*['\"]?[A-Za-z0-9_./+=:-]{16,}"
    ),
    re.compile(r"sk-[A-Za-z0-9]{20,}"),
    re.compile(r"ghp_[A-Za-z0-9_]{20,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
]

TEXT_SUFFIXES = {
    ".md",
    ".py",
    ".yml",
    ".yaml",
    ".json",
    ".toml",
    ".txt",
    ".sh",
    ".env",
    ".ini",
    ".cfg",
    ".rs",
}


def should_scan(path: Path) -> bool:
    if path.name in SKIP_FILE_NAMES:
        return False
    if any(part in SKIP_DIRS for part in path.parts):
        return False
    return path.suffix in TEXT_SUFFIXES or path.name in {"Makefile", "AGENTS.md"}


def main() -> int:
    findings: list[str] = []

    for path in ROOT.rglob("*"):
        if not path.is_file() or not should_scan(path):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue

        for lineno, line in enumerate(text.splitlines(), start=1):
            if CODE_ASSIGNMENT.search(line) or RUST_TYPE_ANNOTATION.search(line):
                continue
            for pattern in SECRET_PATTERNS:
                if pattern.search(line):
                    findings.append(
                        f"{path.relative_to(ROOT)}:{lineno}: possible secret pattern"
                    )
                    break

    if findings:
        print("Possible secrets found:")
        for finding in findings:
            print(f"- {finding}")
        return 1

    print("Secret scan OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
