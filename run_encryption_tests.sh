#!/bin/bash
# Encryption Testing Script for Matrix-Claude Bridge
# This script helps automate encryption testing scenarios

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Matrix-Claude Bridge Encryption Test Suite ===${NC}\n"

# Check if we're in test mode
if [[ ! -f "test-config.toml" ]]; then
    echo -e "${RED}ERROR: test-config.toml not found${NC}"
    echo "Please create test-config.toml with your test account credentials"
    exit 1
fi

# Function to prompt user
prompt_continue() {
    echo -e "\n${YELLOW}$1${NC}"
    read -p "Press Enter to continue or Ctrl+C to abort..."
}

# Function to check test result
check_result() {
    echo -e "\n${YELLOW}Did this test pass? (y/n)${NC}"
    read -r response
    if [[ "$response" != "y" ]]; then
        echo -e "${RED}Test failed! Please investigate.${NC}"
        exit 1
    fi
    echo -e "${GREEN}✓ Test passed${NC}\n"
}

echo -e "${BLUE}Step 1: Clean slate${NC}"
echo "This will delete crypto_store and workspace to start fresh."
prompt_continue "Ready to clean?"

rm -rf crypto_store/ workspace/ test-workspace/
echo -e "${GREEN}✓ Cleaned up${NC}"

echo -e "\n${BLUE}Step 2: Use test configuration${NC}"
if [[ -f "config.toml" ]]; then
    echo "Backing up existing config.toml to config.toml.backup"
    cp config.toml config.toml.backup
fi
cp test-config.toml config.toml
echo -e "${GREEN}✓ Test config activated${NC}"

echo -e "\n${BLUE}Step 3: Build the bot${NC}"
cargo build --release
echo -e "${GREEN}✓ Built successfully${NC}"

echo -e "\n${BLUE}Step 4: Start the bot${NC}"
echo "Starting bot in background..."
cargo run --release > test-bot.log 2>&1 &
BOT_PID=$!
echo "Bot PID: $BOT_PID"

# Wait for bot to start
sleep 5

if ! ps -p $BOT_PID > /dev/null; then
    echo -e "${RED}ERROR: Bot failed to start!${NC}"
    echo "Check test-bot.log for details:"
    tail -20 test-bot.log
    exit 1
fi

echo -e "${GREEN}✓ Bot started (PID: $BOT_PID)${NC}"

echo -e "\n${BLUE}=== SCENARIO 1: Fresh Bot Setup with Encryption ===${NC}"
echo "Manual steps required:"
echo "1. Open Element with test user account"
echo "2. Find DM with bot (@harpertest1:matrix.org)"
echo "3. Send: !create test-encrypt"
echo "4. Check that room has encryption icon/padlock"
echo "5. Join the new room when invited"
prompt_continue "Complete these steps, then continue"

echo -e "\n${BLUE}Checking bot logs for room creation...${NC}"
tail -20 test-bot.log | grep -i "room created" || echo "Check full log if needed"
check_result

echo -e "\n${BLUE}=== SCENARIO 2: Message Round-Trip ===${NC}"
echo "Manual steps required:"
echo "1. In the test-encrypt room, send: 'Hello encrypted world'"
echo "2. Wait for bot to respond"
echo "3. Verify message has padlock icon (encrypted)"
echo "4. Verify bot's response has padlock icon"
prompt_continue "Complete these steps, then continue"
check_result

echo -e "\n${BLUE}=== SCENARIO 3: Device Verification ===${NC}"
echo "Manual steps required:"
echo "1. In Element, click on bot's avatar"
echo "2. Click 'Verify' button"
echo "3. Choose emoji verification"
echo "4. Watch bot logs for emoji display:"
tail -f test-bot.log | grep -A 10 "Emoji verification" &
TAIL_PID=$!
echo "   (Watching logs... Press Ctrl+C when verification completes)"
sleep 2
echo "5. Verify emojis match between Element and bot logs"
echo "6. Bot will auto-confirm after 5 seconds"
prompt_continue "Complete verification, then continue"
kill $TAIL_PID 2>/dev/null || true
check_result

echo -e "\n${BLUE}=== SCENARIO 4: Multi-Channel Encryption ===${NC}"
echo "Manual steps required:"
echo "1. In DM with bot, send: !create channel1"
echo "2. Join channel1 and send a test message"
echo "3. In DM, send: !create channel2"
echo "4. Join channel2 and send a test message"
echo "5. Verify both channels show encryption"
echo "6. Verify no 'Unable to decrypt' errors"
prompt_continue "Complete these steps, then continue"
check_result

echo -e "\n${BLUE}=== SCENARIO 5: Webhook Testing ===${NC}"
echo "Manual steps required:"
echo "1. In test-encrypt room, send: !status"
echo "2. Copy the session ID from the response"
read -p "Enter the session ID: " SESSION_ID
echo "3. Testing webhook..."

curl -X POST "http://localhost:3000/webhook/session/$SESSION_ID" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Reply with: Webhook test successful"}' \
  && echo -e "\n${GREEN}✓ Webhook request sent${NC}" \
  || echo -e "\n${RED}✗ Webhook request failed${NC}"

echo "4. Check the room for bot's webhook response"
echo "5. Verify webhook message is encrypted (has padlock)"
prompt_continue "Verify webhook message appeared encrypted, then continue"
check_result

echo -e "\n${BLUE}=== SCENARIO 6: Persistence Test ===${NC}"
echo "Restarting bot to test encryption persistence..."
kill $BOT_PID
wait $BOT_PID 2>/dev/null || true
sleep 2

echo "Starting bot again..."
cargo run --release > test-bot-restart.log 2>&1 &
BOT_PID=$!
sleep 5

if ! ps -p $BOT_PID > /dev/null; then
    echo -e "${RED}ERROR: Bot failed to restart!${NC}"
    echo "Check test-bot-restart.log for details"
    exit 1
fi

echo -e "${GREEN}✓ Bot restarted (PID: $BOT_PID)${NC}"

echo "Manual steps required:"
echo "1. Go back to test-encrypt room"
echo "2. Send another message: 'Testing after restart'"
echo "3. Verify bot can still decrypt and respond"
echo "4. Verify message is encrypted"
prompt_continue "Complete these steps, then continue"
check_result

echo -e "\n${BLUE}=== Cleanup ===${NC}"
echo "Stopping bot..."
kill $BOT_PID 2>/dev/null || true
wait $BOT_PID 2>/dev/null || true

if [[ -f "config.toml.backup" ]]; then
    echo "Restoring original config..."
    mv config.toml.backup config.toml
fi

echo -e "\n${GREEN}=== ALL TESTS PASSED! ===${NC}"
echo -e "${GREEN}Encryption is working correctly.${NC}\n"

echo "Test logs saved to:"
echo "  - test-bot.log (initial run)"
echo "  - test-bot-restart.log (after restart)"
echo ""
echo "You can clean up test data with:"
echo "  rm -rf test-workspace/ crypto_store/ test-bot*.log"
