#!/bin/bash

input=$(cat)

DIR=$(jq -r '.workspace.current_dir // "~"' <<< "$input")
DIR_NAME=$(basename "$DIR")
CURRENT_VERSION=$(jq -r '.agent.version // "?"' <<< "$input")
UPDATE_VERSION=$(jq -r '.update.latest_version // empty' <<< "$input")
MODEL=$(jq -r '.model.display_name // "?"' <<< "$input")
EFFORT=$(jq -r '.model.reasoning_effort // empty' <<< "$input")
SERVICE_TIER=$(jq -r '.service_tier // empty' <<< "$input")
PROFILE=$(jq -r '.profile // empty' <<< "$input")
PERSONALITY=$(jq -r 'if .personality.id == "none" then empty else .personality.display_name // empty end' <<< "$input")
PCT=$(jq -r '.context_window.used_percentage // 0' <<< "$input" | cut -d. -f1)
RATE_5H=$(jq -r '.rate_limits.five_hour.used_percentage // empty' <<< "$input")
RATE_7D=$(jq -r '.rate_limits.seven_day.used_percentage // empty' <<< "$input")
RESET_5H=$(jq -r '.rate_limits.five_hour.resets_at // empty' <<< "$input" | cut -d. -f1)
RESET_7D=$(jq -r '.rate_limits.seven_day.resets_at // empty' <<< "$input" | cut -d. -f1)
NOW=$(date +%s)

fmt_remaining() {
  local diff=$(( $1 - NOW ))
  if (( diff <= 0 )); then echo "now"; return; fi
  local d=$((diff / 86400)) h=$(((diff % 86400) / 3600)) m=$(((diff % 3600) / 60))
  if (( d > 0 )); then echo "${d}d${h}h"
  elif (( h > 0 )); then echo "${h}h${m}m"
  else echo "${m}m"; fi
}

# Above budget, compare usage with elapsed time. Below budget, compare remaining
# allowance with remaining time. Return a signed deviation from the normal rate:
# zero is on pace, positive is overspending, negative is spare allowance.
# Keep fractional usage and seconds until rounding the final percentage.
calc_pace() {
  local used=$1 reset=$2 window=$3
  local remaining=$((reset - NOW))
  if (( remaining <= 0 )); then return; fi
  (( remaining > window )) && remaining=$window
  jq -nr --argjson used "$used" --argjson remaining "$remaining" --argjson window "$window" \
    '([$used, 0] | max) as $usage |
     ($window - $remaining) as $elapsed |
     if $usage * $window > 100 * $elapsed then
       if $elapsed > 0 then ($usage * $window / $elapsed - 100 | round) else empty end
     else
       (100 - (100 - $usage) * $window / $remaining | round)
     end'
}

# Context progress bar.
FILLED=$((PCT * 10 / 100))
EMPTY=$((10 - FILLED))
printf -v FILL "%${FILLED}s"
printf -v PAD "%${EMPTY}s"
BAR="${FILL// /▓}${PAD// /░}"

CYAN='\033[36m'
WHITE='\033[37m'
RESET='\033[0m'
GRAY='\033[38;2;153;153;153m'
UPDATE_COLOR='\033[38;2;255;193;7m'

# Gradient saturation/lightness for the usage colors (HSL). Lightness 55 keeps
# every hue bright enough to read on a dark terminal.
GRAD_SAT=95
GRAD_LIGHT=55

# Severity 0 is pinned to #3084ff, the calm end of the gradient.
DEEP_BLUE="48;132;255"

# Control points mapping severity to hue, piecewise-linear between each pair.
HUE_V=(15 25 55 75 85 100)
HUE_H=(200 165 90 45 18 0)

# HSL to "R;G;B" using fixed-point integer math multiplied by 1000.
hsl_to_rgb() {
  local h=$(( $1 % 360 )) s=$2 l=$3
  (( h < 0 )) && h=$(( h + 360 ))
  local abs=$(( 2*l - 100 )); (( abs < 0 )) && abs=$(( -abs ))
  local c=$(( (100 - abs) * s / 10 ))
  local hp=$(( h * 1000 / 60 ))
  local hmod=$(( hp % 2000 ))
  local d=$(( hmod - 1000 )); (( d < 0 )) && d=$(( -d ))
  local x=$(( c * (1000 - d) / 1000 ))
  local m=$(( l * 10 - c / 2 ))
  local r g b seg=$(( h / 60 ))
  case $seg in
    0) r=$c; g=$x; b=0 ;;
    1) r=$x; g=$c; b=0 ;;
    2) r=0;  g=$c; b=$x ;;
    3) r=0;  g=$x; b=$c ;;
    4) r=$x; g=0;  b=$c ;;
    *) r=$c; g=0;  b=$x ;;
  esac
  echo "$(( (r + m) * 255 / 1000 ));$(( (g + m) * 255 / 1000 ));$(( (b + m) * 255 / 1000 ))"
}

hue_from_value() {
  local value=$1 n=${#HUE_V[@]} i
  for (( i=0; i<n-1; i++ )); do
    if (( value <= HUE_V[i+1] )); then
      local v0=${HUE_V[i]} v1=${HUE_V[i+1]} h0=${HUE_H[i]} h1=${HUE_H[i+1]}
      echo $(( h0 + (h1 - h0) * (value - v0) / (v1 - v0) )); return
    fi
  done
  echo "${HUE_H[n-1]}"
}

lerp_rgb() {
  local n=$3 d=$4 r1 g1 b1 r2 g2 b2
  IFS=';' read -r r1 g1 b1 <<< "$1"
  IFS=';' read -r r2 g2 b2 <<< "$2"
  echo "$(( r1 + (r2-r1)*n/d ));$(( g1 + (g2-g1)*n/d ));$(( b1 + (b2-b1)*n/d ))"
}

usage_color() {
  local value=$1 rgb
  (( value < 0 )) && value=0
  (( value > 100 )) && value=100
  if (( value < HUE_V[0] )); then
    rgb=$(lerp_rgb "$DEEP_BLUE" "$(hsl_to_rgb "${HUE_H[0]}" "$GRAD_SAT" "$GRAD_LIGHT")" "$value" "${HUE_V[0]}")
  else
    rgb=$(hsl_to_rgb "$(hue_from_value "$value")" "$GRAD_SAT" "$GRAD_LIGHT")
  fi
  echo "\033[38;2;${rgb}m"
}

# Context severity stays blue below 10% and reaches red at 90%.
BAR_COLOR=$(usage_color $(( (PCT - 10) * 5 / 4 )))

# Rate-limit color takes the worse signal between pace and proximity to 100% used.
rate_color() {
  local used=$1 pace=$2
  local used_val val
  used_val=$(jq -nr --argjson used "$used" '($used - 50) * 5 / 2 | floor')
  val=$used_val
  if [[ -n "$pace" ]]; then
    local pace_val=$(( 50 + pace * 5 / 4 ))
    (( pace_val > val )) && val=$pace_val
  fi
  usage_color "$val"
}

# ▲ needs a slower rate, ▼ allows a faster rate, and • is on pace.
# Display deviation through 99%, then the total rate as a multiplier: 100% is 2.00x.
fmt_pace() {
  if [[ -z "$1" ]]; then echo "--"; return; fi
  local delta=$1 marker="•"
  if (( delta > 0 )); then marker="▲"
  elif (( delta < 0 )); then marker="▼"; fi
  local percent=${delta#-}
  if (( percent >= 100 )); then
    printf '%s%d.%02dx\n' "$marker" "$((1 + percent / 100))" "$((percent % 100))"
  else
    printf '%s%d%%\n' "$marker" "$percent"
  fi
}

fmt_rate() {
  jq -nr --argjson used "$1" 'if $used >= 100 then "100%+" else "\($used | floor)%" end'
}

rate_segment() {
  local label=$1 used=$2 reset_at=$3 window_seconds=$4
  if [[ -z "$used" ]]; then
    printf "%b | %s: --" "$GRAY" "$label"
    return
  fi

  local pace="" color rate_fmt pace_fmt
  if [[ -n "$reset_at" ]]; then
    pace=$(calc_pace "$used" "$reset_at" "$window_seconds")
  fi
  color=$(rate_color "$used" "$pace")
  rate_fmt=$(fmt_rate "$used")
  pace_fmt=$(fmt_pace "$pace")
  printf "%b | %s: %b%s %s%b" "$GRAY" "$label" "$color" "$rate_fmt" "$pace_fmt" "$GRAY"
  if [[ -n "$reset_at" ]]; then
    printf " %b(%s)%b" "$WHITE" "$(fmt_remaining "$reset_at")" "$GRAY"
  fi
}

# One marker per available reload. Omitted credit details have unknown expiration;
# an explicit null expires_at means the reload does not expire. Expired entries
# disappear between account reads. Bound the display to six markers plus a count.
reloads_segment() {
  local markers
  markers=$(jq -r --argjson now "$NOW" '
    def remaining:
      if . == null then "∞"
      else (. - $now) as $seconds |
        if $seconds >= 86400 then "\($seconds / 86400 | floor)d"
        elif $seconds >= 3600 then "\($seconds / 3600 | floor)h"
        elif $seconds >= 60 then "\($seconds / 60 | floor)m"
        else "<1m" end
      end;
    (.reloads.available_count // 0 | floor | [., 0] | max) as $count |
    (.reloads.credits // [] | .[:$count]) as $credits |
    ($count - ($credits | length)) as $unknown |
    ($credits | map(select(.expires_at == null or .expires_at > $now)) |
      sort_by(.expires_at // 1e30)) as $active |
    (($active | length) + $unknown) as $available |
    ([$active[:6][] | "↻\(.expires_at | remaining)"] +
      [range(0; ([6 - ($active | length), $unknown] | min | [., 0] | max)) | "↻?"] +
        (if $available > 6 then ["+\($available - 6)"] else [] end)) | join(", ")
  ' <<< "$input")
  if [[ -n "$markers" ]]; then
    printf "%b | %b%s%b" "$GRAY" "$WHITE" "$markers" "$GRAY"
  fi
}

printf "%b%s\$ %bctx: %b%s %s%%%b" "$CYAN" "$DIR_NAME" "$GRAY" "$BAR_COLOR" "$BAR" "$PCT" "$GRAY"
rate_segment "5h" "$RATE_5H" "$RESET_5H" 18000
rate_segment "7d" "$RATE_7D" "$RESET_7D" 604800
reloads_segment
printf "%b | %s" "$GRAY" "$MODEL"
if [[ -n "$EFFORT" ]]; then
  printf " %s" "$EFFORT"
fi
if [[ "$SERVICE_TIER" == "fast" || "$SERVICE_TIER" == "priority" ]]; then
  printf " ⚡"
fi
if [[ -n "$PROFILE" ]]; then
  printf " · %s" "$PROFILE"
elif [[ -n "$PERSONALITY" ]]; then
  printf " · %s" "$PERSONALITY"
fi
if [[ -n "$UPDATE_VERSION" ]]; then
  printf " %b| Update available (%s => %s)%b" "$UPDATE_COLOR" "$CURRENT_VERSION" "$UPDATE_VERSION" "$GRAY"
fi
printf "%b\n" "$RESET"
