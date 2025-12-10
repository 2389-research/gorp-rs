#!/bin/bash
# Clean up Matrix account state - reject invites, leave rooms, delete devices

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

MATRIX_HOME_SERVER="https://matrix.org"

echo -e "${BLUE}=== Matrix Account Cleanup Tool ===${NC}\n"

# Prompt for credentials
read -p "Matrix User ID (e.g., @user:matrix.org): " USER_ID
read -sp "Password: " PASSWORD
echo ""

echo -e "\n${BLUE}Step 1: Login${NC}"
LOGIN_RESPONSE=$(curl -s -X POST "${MATRIX_HOME_SERVER}/_matrix/client/r0/login" \
  -H "Content-Type: application/json" \
  -d "{
    \"type\": \"m.login.password\",
    \"user\": \"${USER_ID}\",
    \"password\": \"${PASSWORD}\"
  }")

ACCESS_TOKEN=$(echo "$LOGIN_RESPONSE" | jq -r '.access_token')
DEVICE_ID=$(echo "$LOGIN_RESPONSE" | jq -r '.device_id')

if [[ -z "$ACCESS_TOKEN" || "$ACCESS_TOKEN" == "null" ]]; then
    echo -e "${RED}✗ Login failed${NC}"
    echo "$LOGIN_RESPONSE" | jq .
    exit 1
fi

echo -e "${GREEN}✓ Logged in as ${USER_ID}${NC}"
echo "Device ID: $DEVICE_ID"

echo -e "\n${BLUE}Step 2: Get account state${NC}"
SYNC_RESPONSE=$(curl -s -X GET "${MATRIX_HOME_SERVER}/_matrix/client/v3/sync?timeout=0" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}")

# Count invites
INVITE_COUNT=$(echo "$SYNC_RESPONSE" | jq '.rooms.invite | length')
echo "Pending invites: $INVITE_COUNT"

# Count joined rooms
JOINED_COUNT=$(echo "$SYNC_RESPONSE" | jq '.rooms.join | length')
echo "Joined rooms: $JOINED_COUNT"

echo -e "\n${BLUE}Step 3: Reject all pending invites${NC}"
if [[ $INVITE_COUNT -gt 0 ]]; then
    echo "$SYNC_RESPONSE" | jq -r '.rooms.invite | keys[]' | while read -r ROOM_ID; do
        echo -n "Rejecting invite to $ROOM_ID... "
        REJECT_RESPONSE=$(curl -s -X POST "${MATRIX_HOME_SERVER}/_matrix/client/v3/rooms/${ROOM_ID}/leave" \
          -H "Authorization: Bearer ${ACCESS_TOKEN}" \
          -H "Content-Type: application/json" \
          -d '{}')

        if echo "$REJECT_RESPONSE" | jq -e '.errcode' > /dev/null 2>&1; then
            echo -e "${YELLOW}⚠ $(echo "$REJECT_RESPONSE" | jq -r '.error')${NC}"
        else
            echo -e "${GREEN}✓${NC}"
        fi
        sleep 0.5
    done
else
    echo "No pending invites"
fi

echo -e "\n${BLUE}Step 4: Leave all joined rooms${NC}"
read -p "Leave all ${JOINED_COUNT} joined rooms? (y/N): " CONFIRM
if [[ "$CONFIRM" == "y" || "$CONFIRM" == "Y" ]]; then
    echo "$SYNC_RESPONSE" | jq -r '.rooms.join | keys[]' | while read -r ROOM_ID; do
        ROOM_NAME=$(echo "$SYNC_RESPONSE" | jq -r ".rooms.join[\"$ROOM_ID\"].state.events[] | select(.type == \"m.room.name\") | .content.name" 2>/dev/null || echo "Unknown")
        echo -n "Leaving room: $ROOM_NAME ($ROOM_ID)... "

        LEAVE_RESPONSE=$(curl -s -X POST "${MATRIX_HOME_SERVER}/_matrix/client/v3/rooms/${ROOM_ID}/leave" \
          -H "Authorization: Bearer ${ACCESS_TOKEN}" \
          -H "Content-Type: application/json" \
          -d '{}')

        if echo "$LEAVE_RESPONSE" | jq -e '.errcode' > /dev/null 2>&1; then
            echo -e "${YELLOW}⚠ $(echo "$LEAVE_RESPONSE" | jq -r '.error')${NC}"
        else
            echo -e "${GREEN}✓${NC}"
        fi
        sleep 0.5
    done
else
    echo "Skipping room leave"
fi

echo -e "\n${BLUE}Step 5: Delete old devices${NC}"
DEVICES_RESPONSE=$(curl -s -X GET "${MATRIX_HOME_SERVER}/_matrix/client/v3/devices" \
  -H "Authorization: Bearer ${ACCESS_TOKEN}")

DEVICE_COUNT=$(echo "$DEVICES_RESPONSE" | jq '.devices | length')
echo "Total devices: $DEVICE_COUNT"
echo "Current device: $DEVICE_ID"

echo "$DEVICES_RESPONSE" | jq -r '.devices[] | "\(.device_id)\t\(.display_name // "No name")\t\(.last_seen_ip // "Never")"' | \
  while IFS=$'\t' read -r DEV_ID DEV_NAME DEV_IP; do
    if [[ "$DEV_ID" != "$DEVICE_ID" ]]; then
        echo -e "  - $DEV_ID: $DEV_NAME (IP: $DEV_IP)"
    fi
done

read -p "Delete all devices except current ($DEVICE_ID)? (y/N): " CONFIRM_DEV
if [[ "$CONFIRM_DEV" == "y" || "$CONFIRM_DEV" == "Y" ]]; then
    # Collect device IDs to delete
    DEVICES_TO_DELETE=$(echo "$DEVICES_RESPONSE" | jq -r --arg current "$DEVICE_ID" '.devices[] | select(.device_id != $current) | .device_id')

    if [[ -n "$DEVICES_TO_DELETE" ]]; then
        DEVICE_IDS_JSON=$(echo "$DEVICES_TO_DELETE" | jq -R . | jq -s .)

        echo -n "Deleting devices... "
        DELETE_RESPONSE=$(curl -s -X POST "${MATRIX_HOME_SERVER}/_matrix/client/v3/delete_devices" \
          -H "Authorization: Bearer ${ACCESS_TOKEN}" \
          -H "Content-Type: application/json" \
          -d "{
            \"devices\": $DEVICE_IDS_JSON,
            \"auth\": {
              \"type\": \"m.login.password\",
              \"user\": \"${USER_ID}\",
              \"password\": \"${PASSWORD}\"
            }
          }")

        if echo "$DELETE_RESPONSE" | jq -e '.errcode' > /dev/null 2>&1; then
            echo -e "${YELLOW}⚠ $(echo "$DELETE_RESPONSE" | jq -r '.error')${NC}"
        else
            echo -e "${GREEN}✓${NC}"
        fi
    else
        echo "No old devices to delete"
    fi
else
    echo "Skipping device deletion"
fi

echo -e "\n${GREEN}╔════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║     Account cleanup complete! ✓        ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════╝${NC}\n"

echo "Summary:"
echo "  - Rejected $INVITE_COUNT invites"
echo "  - Left rooms (if confirmed)"
echo "  - Deleted old devices (if confirmed)"
echo ""
echo "Your account is now clean for testing!"
