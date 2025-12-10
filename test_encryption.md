# Encryption Testing Plan

## Test Accounts
- Bot: `@harpertest1:matrix.org` / `ndg@VQA0ajr@drt5pqz`
- User: `@harpertest1:matrix.org` / `kre2ktx5pnv9jkq-MUC` (Note: typo in file, both are harpertest1)

## Test Scenarios

### Scenario 1: Fresh Bot Setup with Encryption
**Goal**: Verify new rooms are created with encryption enabled

**Steps**:
1. Configure bot with test account credentials
2. Start bot fresh (delete crypto_store, workspace/sessions.db)
3. DM bot from user account: `!create test-encrypt`
4. Verify bot creates room with encryption indicator
5. User joins the room
6. User sends test message: "Hello encrypted world"
7. Bot receives and responds to encrypted message

**Expected Results**:
- Room shows encryption icon/indicator in Element
- Messages have padlock icon
- Bot can decrypt and respond to messages
- No "Unable to decrypt" errors

### Scenario 2: Message Round-Trip Verification
**Goal**: Ensure messages are actually encrypted end-to-end

**Steps**:
1. In encrypted channel, send: "Test message 1"
2. Bot responds with Claude output
3. Send: "!status" to get channel info
4. Verify webhook works: POST to webhook with test prompt
5. Check bot sends encrypted response via webhook

**Expected Results**:
- All messages encrypted (padlock icons)
- Bot successfully decrypts user messages
- Bot successfully encrypts responses
- Webhook messages are encrypted

### Scenario 3: Device Verification Flow
**Goal**: Test emoji verification between user and bot

**Steps**:
1. User initiates verification request from Element
2. Bot auto-accepts verification (check logs)
3. Bot displays emoji list in logs
4. User verifies emojis match
5. Bot auto-confirms after 5 seconds
6. Check verification completes successfully

**Expected Results**:
- Bot logs show emoji verification started
- Bot logs display 7 emojis with descriptions
- Bot auto-confirms after timeout
- Verification succeeds (green checkmark in Element)

### Scenario 4: Multi-Channel Encryption
**Goal**: Verify encryption works across multiple channels

**Steps**:
1. Create 3 channels: `!create channel1`, `!create channel2`, `!create channel3`
2. Send messages in each channel
3. Verify each channel is independently encrypted
4. Verify no key-sharing issues between channels

**Expected Results**:
- All 3 channels show encryption enabled
- No "Unable to decrypt" errors in any channel
- Each channel has separate Megolm session

### Scenario 5: Persistence After Restart
**Goal**: Ensure encryption persists after bot restart

**Steps**:
1. Create encrypted channel
2. Send test message
3. Stop bot
4. Restart bot
5. Send another message in same channel
6. Verify bot can still decrypt and respond

**Expected Results**:
- Bot restores crypto_store successfully
- Bot can decrypt new messages after restart
- No device verification needed after restart
- Session continuity maintained

## Manual Testing Checklist

### Before Starting
- [ ] Delete `crypto_store/` directory
- [ ] Delete `workspace/sessions.db`
- [ ] Update `config.toml` with test bot credentials
- [ ] Update `allowed_users` to include test user account

### Test Execution
- [ ] Scenario 1: Fresh Bot Setup
- [ ] Scenario 2: Message Round-Trip
- [ ] Scenario 3: Device Verification
- [ ] Scenario 4: Multi-Channel
- [ ] Scenario 5: Persistence

### Verification Points
- [ ] Room encryption indicator visible in Element
- [ ] All messages show padlock icon
- [ ] No "Unable to decrypt" warnings
- [ ] Bot logs show successful encryption/decryption
- [ ] Device verification completes successfully
- [ ] Webhook messages are encrypted
- [ ] Encryption persists after restart

## Success Criteria

**PASS**: All 5 scenarios complete without encryption errors
**FAIL**: Any "Unable to decrypt" messages or verification failures

## Known Issues to Watch For

1. **First message issues**: Sometimes first message in new E2E room fails - retry
2. **Key rotation**: Long-running sessions may trigger key rotation
3. **Multiple devices**: User having multiple devices can complicate verification
4. **Sync delays**: Initial sync may take time to propagate keys

## Debugging Commands

```bash
# Check crypto store contents
sqlite3 crypto_store/matrix-sdk-crypto.db "SELECT * FROM devices;"

# Check for encryption errors in logs
tail -f logs | grep -i "encrypt\|decrypt\|verification"

# Verify room is encrypted via curl
curl -X GET "https://matrix.org/_matrix/client/r0/rooms/!ROOM_ID:matrix.org/state/m.room.encryption" \
  -H "Authorization: Bearer ACCESS_TOKEN"
```
