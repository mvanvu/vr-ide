#!/usr/bin/env bash
# Measures time to first frame and max RSS of a release build of vr-ide under a pty.
# Usage: scripts/bench-startup.sh [project-dir]   (default: current directory)
#
# Two cases:
#  1. the project opened empty: first frame and max RSS;
#  2. a generated 200 KB Rust file opened with `vr-ide <file>`: first frame (the parse runs in
#     the background, so it is not waited for) and max RSS after the colours landed (the run
#     stays open for a few seconds, then quits with ctrl+q).
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
target_dir="$(cd "${1:-.}" && pwd)"
cargo build --release -p vr-ide --manifest-path "$repo/Cargo.toml" >/dev/null
bin="$repo/target/release/vr-ide"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

max_rss_mb() {
  local kb
  kb=$(awk -F': ' '/Maximum resident set size/ {print $2}' "$1")
  if [ -z "$kb" ]; then
    echo "bench-startup: no max RSS found in $1" >&2
    exit 1
  fi
  echo $(( kb / 1024 ))
}

start=$(date +%s%N)
(cd "$target_dir" && VR_IDE_EXIT_AFTER_FIRST_FRAME=1 \
  script -qec "$(printf '%q ' /usr/bin/time -v -o "$work/plain.log" "$bin")" /dev/null >/dev/null)
end=$(date +%s%N)
echo "first_frame_ms=$(( (end - start) / 1000000 ))  (target < 100, includes pty setup)"
echo "max_rss_mb=$(max_rss_mb "$work/plain.log")  (target < 60)"

# ~200 KB of Rust: the same 12-line block repeated.
big="$work/big.rs"
block='// A block of code.
pub struct Point { x: f64, y: f64 }

impl Point {
    pub fn norm(&self) -> f64 {
        let label = "point";
        let _ = label.len() as u32 + 42;
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

'
: > "$big"
while [ "$(stat -c %s "$big")" -lt 204800 ]; do printf '%s' "$block" >> "$big"; done

start=$(date +%s%N)
(cd "$work" && VR_IDE_EXIT_AFTER_FIRST_FRAME=1 \
  script -qec "$(printf '%q ' /usr/bin/time -v -o "$work/frame.log" "$bin" "$big")" /dev/null >/dev/null)
end=$(date +%s%N)
echo "rust_200kb_first_frame_ms=$(( (end - start) / 1000000 ))  (target < 100, includes pty setup)"
echo "rust_200kb_rss_at_first_frame_mb=$(max_rss_mb "$work/frame.log")"

# Stay open long enough for the background parse to land and be drawn, then quit.
(cd "$work" && { sleep 4; printf '\021'; } | \
  script -qec "$(printf '%q ' /usr/bin/time -v -o "$work/coloured.log" "$bin" "$big")" /dev/null >/dev/null)
echo "rust_200kb_max_rss_after_colours_mb=$(max_rss_mb "$work/coloured.log")  (target < 60)"
