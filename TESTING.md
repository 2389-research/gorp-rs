# Encryption Testing Guide

This guide helps you verify that E2E encryption is working correctly in the Matrix-Claude bridge.

## Quick Start

### Prerequisites
1. Two Matrix test accounts (see `test-accounts.txt` - not committed)
2. Element client for manual verification
3. `jq` installed for JSON parsing: `brew install jq`

### Running the Full Test Suite

```bash
# 1. Set up test configuration
cp test-config.toml config.toml
# Edit config.toml with your test account credentials

# 2. Run the automated test suite
./run_encryption_tests.sh
```

The script will guide you through 6 comprehensive scenarios:
1. **Fresh Bot Setup** - Verify encryption on new rooms
2. **Message Round-Trip** - Test encrypted message exchange
3. **Device Verification** - Test emoji verification flow
4. **Multi-Channel** - Verify encryption across channels
5. **Webhook Testing** - Ensure webhook messages are encrypted
6. **Persistence** - Test encryption after bot restart

## Manual Testing

If you prefer manual testing, follow these steps:

### 1. Clean Start
```bash
rm -rf crypto_store/ workspace/
```

### 2. Configure Test Account
Edit `config.toml`:
```toml
[matrix]
home_server = "https://matrix.org"
user_id = "@testbot:matrix.org"
password = "your-test-password"
allowed_users = ["@testuser:matrix.org"]
```

### 3. Start Bot
```bash
cargo run --release 2>&1 | tee bot.log
```

### 4. Create Encrypted Channel
From Element with test user account:
1. DM the bot
2. Send: `!create test-encrypt`
3. Join the room when invited
4. **Verify**: Room shows encryption icon (padlock or shield)

### 5. Test Message Encryption
In the encrypted room:
1. Send: "Hello encrypted world"
2. **Verify**: Message has padlock icon
3. Wait for bot response
4. **Verify**: Bot's response has padlock icon
5. **Verify**: No "Unable to decrypt" errors

### 6. Verify Encryption API
Get your access token from Element (Settings → Help & About → Access Token)

```bash
./verify_room_encryption.sh '!roomid:matrix.org' 'syt_your_access_token'
```

Expected output:
```
✅ ENCRYPTION ENABLED
   Algorithm: m.megolm.v1.aes-sha2
   ✅ Using recommended Megolm algorithm
```

### 7. Test Device Verification (Optional but Recommended)
1. In Element, click bot's avatar → Verify
2. Choose emoji verification
3. Check bot logs for emoji display:
   ```bash
   tail -f bot.log | grep -A 10 "Emoji verification"
   ```
4. Verify emojis match
5. Bot auto-confirms after 5 seconds
6. **Verify**: Green checkmark appears in Element

## Common Issues

### "Unable to decrypt" on first message
**Cause**: Key distribution delay
**Fix**: Wait 5 seconds and try again

### Bot doesn't show emojis for verification
**Cause**: Verification handler not registered
**Fix**: Check bot logs for "verification handlers registered"

### Room created without encryption
**Cause**: Code not updated
**Fix**: Ensure you're running latest version with encryption enabled

### Webhook messages not encrypted
**Cause**: Bot not in encrypted room
**Fix**: Check room was created after encryption feature was added

## Debugging

### Check if room is encrypted via database
```bash
sqlite3 crypto_store/matrix-sdk-crypto.db \
  "SELECT * FROM devices WHERE device_id = 'test-bot-encryption';"
```

### Check encryption state logs
```bash
grep -i "encrypt\|decrypt\|verification" bot.log
```

### View Megolm session keys
```bash
sqlite3 crypto_store/matrix-sdk-crypto.db \
  "SELECT room_id, session_id FROM inbound_group_sessions;"
```

## Success Criteria

✅ **PASS** if:
- All new rooms show encryption indicator
- All messages have padlock icons
- No "Unable to decrypt" messages
- Device verification completes successfully
- Bot can decrypt after restart
- Webhook messages are encrypted

❌ **FAIL** if:
- Any messages show "Unable to decrypt"
- Room created without encryption
- Device verification fails
- Bot can't decrypt after restart

## Cleanup

After testing:
```bash
# Remove test data
rm -rf test-workspace/ crypto_store/ test-bot*.log

# Restore original config
mv config.toml.backup config.toml  # if you backed it up
```

## Security Notes

⚠️ **Test accounts**: Use dedicated test accounts, not production accounts
⚠️ **Credentials**: Never commit test credentials to git
⚠️ **Recovery keys**: Test accounts should have recovery keys saved
⚠️ **Clean up**: Delete crypto_store after testing to avoid key confusion
