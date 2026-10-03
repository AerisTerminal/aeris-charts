#!/usr/bin/env bash
set -euo pipefail

title=$1
output=$2
snapshot=$(mktemp --suffix=.xwd)
trap 'rm -f -- "$snapshot"' EXIT

# Xvfb presents the client window without compositor decorations. xwd reads the actual
# X11 window pixels after GPUI has painted its warmup frames.
sleep 2
for attempt in {1..60}; do
  if xwd -silent -name "$title" -out "$snapshot" 2>/dev/null; then
    convert "$snapshot" -strip "$output"
    # Mesa's software swapchain can expose an unpresented black buffer between frames.
    # Every parity fixture has a light pixel at its top-left corner, so wait for an actual
    # presented frame without accepting a blank capture as a passing sample.
    pixel=$(identify -format '%[pixel:p{0,0}]' "$output")
    if [[ "$pixel" != 'gray(0)' && "$pixel" != 'srgb(0,0,0)' ]]; then
      echo "OK X11 client capture"
      exit 0
    fi
  fi
  sleep 0.1
done
echo "ERR X11 window has no presented fixture pixels: $title"
exit 1
