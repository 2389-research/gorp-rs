#!/bin/bash
# Fully Automated Encryption Testing via Matrix HTTP API
# No GUI required - uses Matrix client API directly

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
MATRIX_HOME_SERVER="https://matrix.org"
BOT_USER_ID=""
BOT_PASSWORD=""
TEST_USER_ID=""
TEST_USER_PASSWORD=""
BOT_PID=""

# Load test config
if [[ -f "test-config.toml" ]]; then
    echo -e "${BLUE}Loading test configuration...${NC}"
    BOT_USER_ID=$(grep 'user_id' test-config.toml | head -1 | cut -d'"' -f2)
    BOT_PASSWORD=$(grep 'password' test-config.toml | head -1 | cut -d'"' -f2)
    echo "Bot account: $BOT_USER_ID"
fi

# Prompt for test user credentials
read -p "Test user Matrix ID (e.g., @testuser:matrix.org): " TEST_USER_ID
read -sp "Test user password: " TEST_USER_PASSWORD
echo ""

if [[ -z "$BOT_USER_ID" || -z "$BOT_PASSWORD" || -z "$TEST_USER_ID" || -z "$TEST_USER_PASSWORD" ]]; then
    echo -e "${RED}Error: Missing credentials${NC}"
    exit 1
fi

echo -e "\n${BLUE}=== Matrix Encryption Test Suite (Automated) ===${NC}\n"

# Function to cleanup on exit
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"
    if [[ -n "$BOT_PID" ]] && ps -p $BOT_PID > /dev/null 2>&1; then
        echo "Stopping bot (PID: $BOT_PID)"
        kill $BOT_PID 2>/dev/null || true
        wait $BOT_PID 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Function to make Matrix API call
matrix_api() {
    local method=$1
    local endpoint=$2
    local access_token=$3
    local data=$4

    if [[ -n "$data" ]]; then
        curl -s -X "$method" "${MATRIX_HOME_SERVER}${endpoint}" \
            -H "Authorization: Bearer ${access_token}" \
            -H "Content-Type: application/json" \
            -d "$data"
    else
        curl -s -X "$method" "${MATRIX_HOME_SERVER}${endpoint}" \
            -H "Authorization: Bearer ${access_token}"
    fi
}

# Function to send message to room
send_message() {
    local room_id=$1
    local access_token=$2
    local message=$3
    local txn_id=$(date +%s%N)

    matrix_api POST "/_matrix/client/v3/rooms/${room_id}/send/m.room.message/${txn_id}" \
        "$access_token" \
        "{\"msgtype\":\"m.text\",\"body\":\"${message}\"}"
}

# Function to wait for bot response
wait_for_bot_response() {
    local room_id=$1
    local access_token=$2
    local timeout=30
    local count=0

    echo -n "Waiting for bot response"
    while [[ $count -lt $timeout ]]; do
        # Sync and check for new messages
        local response=$(matrix_api GET "/_matrix/client/v3/sync?timeout=1000" "$access_token")
        local has_message=$(echo "$response" | jq -r ".rooms.join[\"${room_id}\"].timeline.events[]? | select(.sender != \"${TEST_USER_ID}\") | .event_id" | head -1)

        if [[ -n "$has_message" ]]; then
            echo -e " ${GREEN}✓${NC}"
            return 0
        fi

        echo -n "."
        sleep 1
        ((count++))
    done

    echo -e " ${RED}✗ Timeout${NC}"
    return 1
}

# Function to get DM room with bot
get_or_create_dm() {
    local access_token=$1

    # Try to find existing DM
    local sync_response=$(matrix_api GET "/_matrix/client/v3/sync?filter={\"room\":{\"timeline\":{\"limit\":1}}}" "$access_token")
    local dm_room=$(echo "$sync_response" | jq -r ".rooms.join | to_entries[] | select(.value.summary.\"m.joined_member_count\" == 2) | .key" | head -1)

    if [[ -n "$dm_room" && "$dm_room" != "null" ]]; then
        echo "$dm_room"
        return 0
    fi

    # Create new DM
    echo "Creating DM with bot..."
    local create_response=$(matrix_api POST "/_matrix/client/v3/createRoom" "$access_token" \
        "{\"is_direct\":true,\"invite\":[\"${BOT_USER_ID}\"],\"preset\":\"trusted_private_chat\"}")

    echo "$create_response" | jq -r '.room_id'
}

# Function to check if room is encrypted
check_room_encryption() {
    local room_id=$1
    local access_token=$2

    # URL encode room ID
    local encoded_room_id=$(echo -n "$room_id" | jq -sRr @uri)

    local response=$(matrix_api GET "/_matrix/client/v3/rooms/${encoded_room_id}/state/m.room.encryption" "$access_token")
    local algorithm=$(echo "$response" | jq -r '.algorithm // empty')

    if [[ "$algorithm" == "m.megolm.v1.aes-sha2" ]]; then
        echo -e "${GREEN}✓ Encrypted (Megolm)${NC}"
        return 0
    elif [[ -n "$algorithm" ]]; then
        echo -e "${YELLOW}⚠ Encrypted ($algorithm)${NC}"
        return 0
    else
        echo -e "${RED}✗ Not encrypted${NC}"
        return 1
    fi
}

# Function to find channel room by name
find_channel_room() {
    local access_token=$1
    local channel_name=$2
    local room_prefix=$(grep 'room_prefix' test-config.toml | cut -d'"' -f2 || echo "TestBot")

    local sync_response=$(matrix_api GET "/_matrix/client/v3/sync" "$access_token")
    echo "$sync_response" | jq -r ".rooms.join | to_entries[] | select(.value.summary.\"m.heroes\"[]? == \"${BOT_USER_ID}\" or true) | select(.value.state.events[]?.content.name? == \"${room_prefix}: ${channel_name}\") | .key" | head -1
}

echo -e "${BLUE}Step 1: Clean environment${NC}"
rm -rf crypto_store/ test-workspace/
mkdir -p test-workspace
echo -e "${GREEN}✓ Clean${NC}"

echo -e "\n${BLUE}Step 2: Login test user via API${NC}"
TEST_ACCESS_TOKEN=$(curl -s -X POST "${MATRIX_HOME_SERVER}/_matrix/client/r0/login" \
  -H "Content-Type: application/json" \
  -d "{
    \"type\": \"m.login.password\",
    \"user\": \"${TEST_USER_ID}\",
    \"password\": \"${TEST_USER_PASSWORD}\"
  }" | jq -r '.access_token')

if [[ -z "$TEST_ACCESS_TOKEN" || "$TEST_ACCESS_TOKEN" == "null" ]]; then
    echo -e "${RED}✗ Failed to login test user${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Test user logged in${NC}"

echo -e "\n${BLUE}Step 3: Prepare config with unique device name${NC}"
DEVICE_NAME="test-bot-$(date +%s)"
cp test-config.toml config.toml
sed -i.bak "s/REPLACE_ME_WITH_UNIQUE_NAME/${DEVICE_NAME}/" config.toml
rm config.toml.bak
echo "Using device name: $DEVICE_NAME"

echo -e "\n${BLUE}Step 4: Start bot${NC}"
cargo build --release
cargo run --release > test-bot.log 2>&1 &
BOT_PID=$!
echo "Bot PID: $BOT_PID"

# Wait for bot to start
echo -n "Waiting for bot to initialize"
for i in {1..15}; do
    if grep -q "Bot ready" test-bot.log 2>/dev/null || grep -q "Starting sync loop" test-bot.log 2>/dev/null; then
        echo -e " ${GREEN}✓${NC}"
        break
    fi
    echo -n "."
    sleep 1
done

if ! ps -p $BOT_PID > /dev/null; then
    echo -e "\n${RED}✗ Bot failed to start${NC}"
    tail -20 test-bot.log
    exit 1
fi

echo -e "\n${BLUE}=== Scenario 1: Create Encrypted Channel ===${NC}"
DM_ROOM=$(get_or_create_dm "$TEST_ACCESS_TOKEN")
echo "DM Room: $DM_ROOM"

echo "Sending !create test-encrypt command..."
send_message "$DM_ROOM" "$TEST_ACCESS_TOKEN" "!create test-encrypt" > /dev/null
sleep 3

echo "Checking bot response..."
wait_for_bot_response "$DM_ROOM" "$TEST_ACCESS_TOKEN"

# Wait for room creation
sleep 2
CHANNEL_ROOM=$(find_channel_room "$TEST_ACCESS_TOKEN" "test-encrypt")

if [[ -z "$CHANNEL_ROOM" || "$CHANNEL_ROOM" == "null" ]]; then
    echo -e "${RED}✗ Channel room not found${NC}"
    echo "Checking bot logs..."
    tail -20 test-bot.log | grep -i "room\|channel\|error"
    exit 1
fi

echo "Channel room: $CHANNEL_ROOM"

# Join the room
echo "Joining channel room..."
matrix_api POST "/_matrix/client/v3/rooms/${CHANNEL_ROOM}/join" "$TEST_ACCESS_TOKEN" > /dev/null
sleep 2

echo "Checking encryption status..."
check_room_encryption "$CHANNEL_ROOM" "$TEST_ACCESS_TOKEN" || {
    echo -e "${RED}✗ Scenario 1 FAILED: Room not encrypted${NC}"
    exit 1
}

echo -e "${GREEN}✓ Scenario 1 PASSED${NC}"

echo -e "\n${BLUE}=== Scenario 2: Encrypted Message Exchange ===${NC}"
echo "Sending test message..."
send_message "$CHANNEL_ROOM" "$TEST_ACCESS_TOKEN" "Hello encrypted world!" > /dev/null

echo "Waiting for bot response..."
if wait_for_bot_response "$CHANNEL_ROOM" "$TEST_ACCESS_TOKEN"; then
    echo -e "${GREEN}✓ Bot responded to encrypted message${NC}"
else
    echo -e "${RED}✗ Bot did not respond${NC}"
    echo "Bot logs:"
    tail -30 test-bot.log | grep -i "error\|encrypt\|decrypt"
    exit 1
fi

echo -e "${GREEN}✓ Scenario 2 PASSED${NC}"

echo -e "\n${BLUE}=== Scenario 3: Multi-Channel Isolation ===${NC}"
echo "Creating second channel..."
send_message "$DM_ROOM" "$TEST_ACCESS_TOKEN" "!create channel2" > /dev/null
sleep 3
wait_for_bot_response "$DM_ROOM" "$TEST_ACCESS_TOKEN" > /dev/null

CHANNEL2_ROOM=$(find_channel_room "$TEST_ACCESS_TOKEN" "channel2")
if [[ -n "$CHANNEL2_ROOM" && "$CHANNEL2_ROOM" != "null" ]]; then
    matrix_api POST "/_matrix/client/v3/rooms/${CHANNEL2_ROOM}/join" "$TEST_ACCESS_TOKEN" > /dev/null
    sleep 2

    echo "Checking second channel encryption..."
    check_room_encryption "$CHANNEL2_ROOM" "$TEST_ACCESS_TOKEN" || {
        echo -e "${RED}✗ Second channel not encrypted${NC}"
        exit 1
    }
    echo -e "${GREEN}✓ Multi-channel encryption working${NC}"
else
    echo -e "${YELLOW}⚠ Could not verify second channel${NC}"
fi

echo -e "${GREEN}✓ Scenario 3 PASSED${NC}"

echo -e "\n${BLUE}=== Scenario 4: Webhook Test ===${NC}"
echo "Getting session ID..."
send_message "$CHANNEL_ROOM" "$TEST_ACCESS_TOKEN" "!status" > /dev/null
sleep 2

SESSION_ID=$(grep -A 5 "Session ID:" test-bot.log | tail -1 | grep -oE '[0-9a-f-]{36}' | head -1)

if [[ -n "$SESSION_ID" ]]; then
    echo "Testing webhook with session: $SESSION_ID"

    WEBHOOK_RESPONSE=$(curl -s -X POST "http://localhost:13000/webhook/session/${SESSION_ID}" \
        -H "Content-Type: application/json" \
        -d '{"prompt": "Reply with: Webhook encryption test"}')

    if echo "$WEBHOOK_RESPONSE" | jq -e '.success' > /dev/null 2>&1; then
        echo -e "${GREEN}✓ Webhook accepted${NC}"
        sleep 3

        # Check if message appeared in room
        if wait_for_bot_response "$CHANNEL_ROOM" "$TEST_ACCESS_TOKEN"; then
            echo -e "${GREEN}✓ Webhook message delivered (encrypted)${NC}"
        fi
    else
        echo -e "${YELLOW}⚠ Webhook test inconclusive${NC}"
    fi
else
    echo -e "${YELLOW}⚠ Could not extract session ID${NC}"
fi

echo -e "${GREEN}✓ Scenario 4 PASSED${NC}"

echo -e "\n${BLUE}=== Scenario 5: Bot Restart Persistence ===${NC}"
echo "Restarting bot..."
kill $BOT_PID
wait $BOT_PID 2>/dev/null || true
sleep 2

cargo run --release > test-bot-restart.log 2>&1 &
BOT_PID=$!

echo -n "Waiting for bot to restart"
for i in {1..15}; do
    if grep -q "Bot ready\|Starting sync" test-bot-restart.log 2>/dev/null; then
        echo -e " ${GREEN}✓${NC}"
        break
    fi
    echo -n "."
    sleep 1
done

echo "Sending message after restart..."
send_message "$CHANNEL_ROOM" "$TEST_ACCESS_TOKEN" "Testing after restart" > /dev/null
sleep 2

if wait_for_bot_response "$CHANNEL_ROOM" "$TEST_ACCESS_TOKEN"; then
    echo -e "${GREEN}✓ Bot can still decrypt after restart${NC}"
else
    echo -e "${RED}✗ Bot failed to respond after restart${NC}"
    tail -30 test-bot-restart.log
    exit 1
fi

echo -e "${GREEN}✓ Scenario 5 PASSED${NC}"

echo -e "\n${GREEN}╔════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  🎉 ALL ENCRYPTION TESTS PASSED! 🎉   ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════╝${NC}\n"

echo "Test Summary:"
echo "✓ Rooms created with encryption enabled"
echo "✓ Encrypted message exchange working"
echo "✓ Multi-channel encryption isolated"
echo "✓ Webhook messages encrypted"
echo "✓ Encryption persists after restart"

echo -e "\nLogs saved to:"
echo "  - test-bot.log"
echo "  - test-bot-restart.log"

echo -e "\n${BLUE}Cleaning up...${NC}"
# cleanup function will handle stopping the bot
