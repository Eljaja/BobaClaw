#!/usr/bin/env bash
cd /mnt/c/Users/ilya/Documents/BobaClaw/bobaClaw
BC=./target/release/bobaclaw
echo '=== run: ls -la ==='
"$BC" agent --message 'run: ls -la' 2>&1 | head -15
