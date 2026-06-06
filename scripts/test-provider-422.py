#!/usr/bin/env python3
"""Reproduce provider 422 validation errors against configured BobaClaw provider."""
from __future__ import annotations

import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

try:
    import yaml
except ImportError:
    print("pip install pyyaml", file=sys.stderr)
    sys.exit(1)


def load_cfg() -> dict:
    home = Path("/root/.bobaclaw")
    if not home.exists():
        home = Path.home() / ".bobaclaw"
    return yaml.safe_load((home / "config.yaml").read_text())


def post(base: str, key: str, payload: dict) -> None:
    req = urllib.request.Request(
        f"{base.rstrip('/')}/chat/completions",
        data=json.dumps(payload).encode(),
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            body = r.read().decode()
            print(f"STATUS {r.status}")
            print(body[:600])
    except urllib.error.HTTPError as e:
        print(f"STATUS {e.code}")
        print(e.read().decode()[:1200])


def main() -> None:
    cfg = load_cfg()
    prov = cfg["provider"]
    base = prov["base_url"]
    key = prov["api_key"]
    model = prov["model"]
    print(f"provider={base} model={model}\n")

    cases = [
        ("simple user", {"model": model, "messages": [{"role": "user", "content": "hi"}]}),
        (
            "system + user + tools",
            {
                "model": model,
                "messages": [
                    {"role": "system", "content": "You are helpful"},
                    {"role": "user", "content": "hello"},
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "exec",
                            "description": "run shell",
                            "parameters": {
                                "type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"],
                            },
                        },
                    }
                ],
                "tool_choice": "auto",
            },
        ),
        (
            "tool loop shape",
            {
                "model": model,
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "run ls"},
                    {
                        "role": "assistant",
                        "content": None,
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {"name": "exec", "arguments": '{"command":"ls"}'},
                            }
                        ],
                    },
                    {"role": "tool", "tool_call_id": "call_1", "content": "file.txt"},
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "exec",
                            "description": "run shell",
                            "parameters": {
                                "type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"],
                            },
                        },
                    }
                ],
                "tool_choice": "auto",
            },
        ),
        (
            "consecutive user messages (compaction pattern)",
            {
                "model": model,
                "messages": [
                    {"role": "system", "content": "You are BobaClaw"},
                    {
                        "role": "user",
                        "content": "[CONTEXT COMPACTION] Earlier turns were summarized.\n\n## Goal\nTest",
                    },
                    {"role": "user", "content": "Погугли топ решений для памяти"},
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "exec",
                            "description": "run shell",
                            "parameters": {
                                "type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"],
                            },
                        },
                    }
                ],
                "tool_choice": "auto",
            },
        ),
        (
            "assistant tool_calls without content field",
            {
                "model": model,
                "messages": [
                    {"role": "user", "content": "run ls"},
                    {
                        "role": "assistant",
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {"name": "exec", "arguments": '{"command":"ls"}'},
                            }
                        ],
                    },
                    {"role": "tool", "tool_call_id": "call_1", "content": "file.txt"},
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "exec",
                            "description": "run shell",
                            "parameters": {
                                "type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"],
                            },
                        },
                    }
                ],
                "tool_choice": "auto",
            },
        ),
        (
            "tool message missing content field",
            {
                "model": model,
                "messages": [
                    {"role": "user", "content": "run ls"},
                    {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {"name": "exec", "arguments": '{"command":"ls"}'},
                            }
                        ],
                    },
                    {"role": "tool", "tool_call_id": "call_1"},
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "exec",
                            "description": "run shell",
                            "parameters": {
                                "type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"],
                            },
                        },
                    }
                ],
            },
        ),
        (
            "assistant tool_calls content empty array",
            {
                "model": model,
                "messages": [
                    {"role": "user", "content": "run ls"},
                    {
                        "role": "assistant",
                        "content": [],
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {"name": "exec", "arguments": '{"command":"ls"}'},
                            }
                        ],
                    },
                    {"role": "tool", "tool_call_id": "call_1", "content": "file.txt"},
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "exec",
                            "description": "run shell",
                            "parameters": {
                                "type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"],
                            },
                        },
                    }
                ],
            },
        ),
    ]

    for name, payload in cases:
        print(f"=== {name} ===")
        post(base, key, payload)
        print()


if __name__ == "__main__":
    main()
