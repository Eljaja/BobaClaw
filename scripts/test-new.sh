#!/usr/bin/env bash
cd /mnt/c/Users/ilya/Documents/BobaClaw/bobaClaw
printf '/new\n/quit\n' | ./target/release/bobaclaw chat 2>&1
