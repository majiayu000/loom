#!/bin/sh
set -eu

dmg=${1:?usage: notarize-dmg.sh <dmg>}

if [ ! -f "$dmg" ]; then
  echo "notarize-dmg: missing DMG: $dmg" >&2
  exit 1
fi
if [ -z "${APPLE_API_KEY:-}" ] || [ -z "${APPLE_API_ISSUER:-}" ] || [ -z "${APPLE_API_KEY_PATH:-}" ]; then
  echo "notarize-dmg: APPLE_API_KEY, APPLE_API_ISSUER, and APPLE_API_KEY_PATH are required" >&2
  exit 1
fi
if [ ! -f "$APPLE_API_KEY_PATH" ]; then
  echo "notarize-dmg: API key file is missing: $APPLE_API_KEY_PATH" >&2
  exit 1
fi

echo "Submitting $(basename "$dmg") to notarytool..."
submission=$(
  xcrun notarytool submit "$dmg" \
    --key "$APPLE_API_KEY_PATH" \
    --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" \
    --wait \
    --timeout 20m \
    --output-format json
)
printf '%s\n' "$submission"

parsed=$(
  printf '%s\n' "$submission" | python3 -c '
import json, re, sys
raw = sys.stdin.read()
status = ""
ident = ""
try:
    data = json.loads(raw)
    if isinstance(data, dict):
        status = str(data.get("status") or "")
        ident = str(data.get("id") or "")
except json.JSONDecodeError:
    for line in raw.splitlines():
        stripped = line.strip()
        match = re.match(r"status:\s*(\S+)", stripped, re.I)
        if match:
            status = match.group(1)
        match = re.match(r"id:\s*(\S+)", stripped, re.I)
        if match:
            ident = match.group(1)
print(status)
print(ident)
'
)
status=$(printf '%s\n' "$parsed" | sed -n '1p')
ident=$(printf '%s\n' "$parsed" | sed -n '2p')

if [ "$status" != "Accepted" ]; then
  echo "notarize-dmg: notarization status was '${status:-unknown}', expected Accepted" >&2
  if [ -n "$ident" ]; then
    xcrun notarytool log "$ident" \
      --key "$APPLE_API_KEY_PATH" \
      --key-id "$APPLE_API_KEY" \
      --issuer "$APPLE_API_ISSUER" || true
  fi
  exit 1
fi

attempt=1
while [ "$attempt" -le 5 ]; do
  if xcrun stapler staple "$dmg"; then
    break
  fi
  if [ "$attempt" -eq 5 ]; then
    echo "notarize-dmg: stapler staple failed after $attempt attempts" >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 15
done

xcrun stapler validate "$dmg"
echo "Notarized and stapled $dmg"
