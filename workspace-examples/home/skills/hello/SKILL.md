---
name: hello
description: Say hello from a sandboxed script
version: 1.0.0
metadata:
  bobaclaw:
    tags: [demo, greeting]
---

# Hello Skill

## When to Use

User asks for a greeting or mentions `hello` skill.

## Procedure

Run `scripts/hello.sh` via executor profile `bwrap-default`.

## Verification

Script prints `hello from bobaclaw`.
