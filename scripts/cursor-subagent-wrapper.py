#!/usr/bin/env python3
"""Thin wrapper for Cursor local subagent backend (Phase C2).

Invoked from BobaClaw sandbox exec as:
  python3 scripts/cursor-subagent-wrapper.py --workspace /path --model composer-2.5 --task '...'

Prints JSON to stdout: {"status": "ok"|"error", "result": "...", "error": null|"..."}

Requires: pip install cursor-sdk (or cursor_sdk) and CURSOR_API_KEY in the environment.
"""

from __future__ import annotations

import argparse
import json
import os
import sys


def main() -> int:
    parser = argparse.ArgumentParser(description="Cursor local subagent wrapper")
    parser.add_argument("--workspace", required=True, help="Local workspace directory")
    parser.add_argument("--model", default="composer-2.5", help="Cursor model slug")
    parser.add_argument("--task", required=True, help="Self-contained subtask prompt")
    args = parser.parse_args()

    api_key = os.environ.get("CURSOR_API_KEY", "").strip()
    if not api_key:
        emit("error", error="missing CURSOR_API_KEY")
        return 1

    try:
        from cursor_sdk import Agent  # type: ignore
    except ImportError:
        try:
            from cursor_sdk.agent import Agent  # type: ignore
        except ImportError:
            emit(
                "error",
                error="cursor_sdk not installed (pip install cursor-sdk)",
            )
            return 1

    workspace = os.path.abspath(args.workspace)
    if not os.path.isdir(workspace):
        emit("error", error=f"workspace not found: {workspace}")
        return 1

    try:
        agent = Agent(api_key=api_key)
        result = agent.prompt(
            args.task,
            local={"cwd": workspace},
            model=args.model,
        )
        text = result if isinstance(result, str) else str(result)
        emit("ok", result=text)
        return 0
    except Exception as exc:  # noqa: BLE001 — subprocess boundary
        emit("error", error=str(exc))
        return 1


def emit(status: str, result: str | None = None, error: str | None = None) -> None:
    payload = {"status": status, "result": result, "error": error}
    print(json.dumps(payload, ensure_ascii=False))


if __name__ == "__main__":
    sys.exit(main())
