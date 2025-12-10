#!/bin/bash
# Quick script to verify a Matrix room has encryption enabled
# Usage: ./verify_room_encryption.sh <room_id> <access_token>

set -e

if [[ $# -lt 2 ]]; then
    echo "Usage: $0 <room_id> <access_token>"
    echo ""
    echo "Example:"
    echo "  $0 '!abc123:matrix.org' 'syt_xxxxx'"
    echo ""
    echo "To get your access token:"
    echo "  1. Element → Settings → Help & About"
    echo "  2. Scroll to bottom, click 'Access Token'"
    exit 1
fi

ROOM_ID="$1"
ACCESS_TOKEN="$2"
HOMESERVER="https://matrix.org"

echo "Checking encryption for room: $ROOM_ID"
echo ""

# URL-encode the room ID
ENCODED_ROOM_ID=$(echo -n "$ROOM_ID" | jq -sRr @uri)

# Query the room encryption state
RESPONSE=$(curl -s -X GET \
  "${HOMESERVER}/_matrix/client/v3/rooms/${ENCODED_ROOM_ID}/state/m.room.encryption" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}")

echo "Response:"
echo "$RESPONSE" | jq '.' 2>/dev/null || echo "$RESPONSE"
echo ""

# Check if encryption is enabled
if echo "$RESPONSE" | jq -e '.algorithm' > /dev/null 2>&1; then
    ALGORITHM=$(echo "$RESPONSE" | jq -r '.algorithm')
    echo "✅ ENCRYPTION ENABLED"
    echo "   Algorithm: $ALGORITHM"

    if [[ "$ALGORITHM" == "m.megolm.v1.aes-sha2" ]]; then
        echo "   ✅ Using recommended Megolm algorithm"
    fi
else
    echo "❌ ENCRYPTION NOT ENABLED"
    echo "   This room is not encrypted!"
    exit 1
fi
