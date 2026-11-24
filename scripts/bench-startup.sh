#!/usr/bin/env bash
# Measures time to first frame and max RSS of a release build of vr-ide under a pty.
# Usage: scripts/bench-startup.sh [project-dir]   (default: current directory)
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
target_dir="$(cd "${1:-.}" && pwd)"
cargo build --release -p vr-ide --manifest-path "$repo/Cargo.toml" >/dev/null
bin="$repo/target/release/vr-ide"

time_log="$(mktemp)"
trap 'rm -f "$time_log"' EXIT

start=$(date +%s%N)
(cd "$target_dir" && VR_IDE_EXIT_AFTER_FIRST_FRAME=1 \
  script -qec "/usr/bin/time -v -o '$time_log' '$bin'" /dev/null >/dev/null)
end=$(date +%s%N)

rss_kb=$(awk -F': ' '/Maximum resident set size/ {print $2}' "$time_log")
echo "first_frame_ms=$(( (end - start) / 1000000 ))  (target < 100, includes pty setup)"
echo "max_rss_mb=$(( rss_kb / 1024 ))  (target < 60)"
