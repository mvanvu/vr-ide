#!/bin/sh
# Usage: show.sh [a|b|c]   shows the T17 welcome mockups, Enter for the next one.
d=$(cd "$(dirname "$0")" && pwd)
for o in ${1:-a b c}; do
  for s in 120x40 80x24 40x12; do
    f="$d/$o-$s.ans"
    [ -f "$f" ] || continue
    printf '\033[0m\033[2J\033[H'
    printf 'option %s  %s  (Enter = next)\n' "$o" "$s" "$s"
    cat "$f"
    printf '\033[0m'
    read -r _ || exit 0
  done
done
printf '\033[0m\033[2J\033[H'
