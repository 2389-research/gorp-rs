# Getting Started with gorp

This guide will walk you through setting up gorp from scratch, even if you've never used Matrix before.

## What is gorp?

gorp is a bridge that lets you chat with Claude (an AI assistant) through Matrix, a secure messaging platform. Think of it like having Claude in a private chat room.

---

## Step 1: Create a Matrix Account

Matrix is a secure, open-source chat platform. You'll need two accounts:
- **Your personal account** (to chat with Claude)
- **A bot account** (for gorp to use)

### Create Your Personal Account

1. Go to [app.element.io](https://app.element.io)
2. Click **"Create Account"**
3. Choose **"matrix.org"** as your server (it's free)
4. Pick a username and password
5. Complete the signup

Your Matrix ID will look like: `@yourname:matrix.org`

### Create a Bot Account

Repeat the process above to create a second account for the bot:
1. Go to [app.element.io](https://app.element.io) in a private/incognito window
2. Click **"Create Account"**
3. Choose a bot username like `myname-claude-bot`
4. Use a strong password (save it somewhere safe!)

Your bot's Matrix ID will look like: `@myname-claude-bot:matrix.org`

---

## Step 2: Install Element (Matrix Client)

Element is the app you'll use to chat on Matrix.

### Desktop (Recommended)
- **Mac**: Download from [element.io/download](https://element.io/download)
- **Windows**: Download from [element.io/download](https://element.io/download)
- **Linux**: `sudo apt install element-desktop` or download from website

### Mobile
- **iPhone**: Search "Element" in the App Store
- **Android**: Search "Element" in Google Play

### Web
Just use [app.element.io](https://app.element.io) in your browser.

---

## Step 3: Set Up Encryption (Important!)

Matrix uses end-to-end encryption. You need to set up a **recovery key** so gorp can read encrypted messages.

### Set Up Security for Your Bot Account

1. **Log into Element** with your **bot account** (the one gorp will use)

2. **Go to Settings**
   - Click your profile picture (top left)
   - Click **"All settings"**

3. **Navigate to Security**
   - Click **"Security & Privacy"** in the left sidebar

4. **Set Up Secure Backup**
   - Look for **"Secure Backup"** section
   - Click **"Set up"** (or "Reset" if already set up)

5. **Choose "Generate a Security Key"**
   - Click **"Generate a Security Key"**
   - Click **"Continue"**

6. **Save Your Recovery Key**
   - You'll see a key that looks like: `EsTR mwqJ JoXZ 8dKN F8hP 9tPq ...`
   - **Copy this key and save it somewhere safe!**
   - Click **"Continue"**

7. **Verify It Worked**
   - Enter your account password when prompted
   - Click **"Done"**

⚠️ **Keep this recovery key safe!** You'll need it for gorp's config file.

---

## Step 4: Configure gorp

Now you have everything needed to set up gorp.

### Option A: Using a Config File

1. Create a folder for gorp's config:
   ```bash
   mkdir -p ~/.config/gorp
   ```

2. Create the config file:
   ```bash
   nano ~/.config/gorp/config.toml
   ```

3. Paste this template and fill in your details:
   ```toml
   [matrix]
   home_server = "https://matrix.org"
   user_id = "@myname-claude-bot:matrix.org"  # Your BOT account
   password = "your-bot-password"
   device_name = "gorp"

   # Your PERSONAL account (who can talk to Claude)
   allowed_users = ["@yourname:matrix.org"]

   # The recovery key from Step 3 (keeps the spaces)
   recovery_key = "EsTR mwqJ JoXZ 8dKN F8hP 9tPq ..."

   [claude]
   binary_path = "claude"

   [workspace]
   path = "~/gorp-workspace"

   [scheduler]
   timezone = "America/Chicago"  # Change to your timezone
   ```

4. Save and exit (in nano: `Ctrl+X`, then `Y`, then `Enter`)

### Option B: Using Environment Variables

Create a `.env` file:
```bash
MATRIX_HOME_SERVER=https://matrix.org
MATRIX_USER_ID=@myname-claude-bot:matrix.org
MATRIX_PASSWORD=your-bot-password
MATRIX_RECOVERY_KEY=EsTR mwqJ JoXZ 8dKN F8hP 9tPq ...
ALLOWED_USERS=@yourname:matrix.org
```

---

## Step 5: Run gorp

### If you installed the binary:
```bash
gorp start
```

### Using Docker:
```bash
docker-compose up -d
```

---

## Step 6: Start Chatting!

1. **Open Element** with your **personal account**

2. **Start a Direct Message** with your bot:
   - Click the **+** next to "People"
   - Search for your bot: `@myname-claude-bot:matrix.org`
   - Click **"Go"**

3. **Send a message** like "Hello!"
   - gorp will respond as Claude

4. **Create topic rooms** (optional):
   - Message the bot: `!join projectname`
   - This creates a room called "Claude: projectname"
   - Each room has its own conversation context

---

## Common Commands

Once chatting with gorp, you can use these commands:

| Command | What it does |
|---------|--------------|
| `!help` | Show available commands |
| `!join roomname` | Create/join a topic room |
| `!schedule every day at 9am: Good morning!` | Schedule a recurring prompt |
| `!schedule list` | Show scheduled prompts |
| `!clear` | Clear conversation history |

---

## Troubleshooting

### "Encryption error" or "Unable to decrypt"
- Make sure you entered the recovery key correctly in the config
- The key has spaces between groups - keep them!
- Try logging out and back in to Element

### Bot doesn't respond
- Check that gorp is running: `gorp start`
- Verify your Matrix ID is in `allowed_users`
- Look at the logs for errors

### "Invalid Matrix user ID"
- Matrix IDs look like `@username:server.com`
- Make sure you included the `@` at the start
- Make sure you included the `:matrix.org` part

### Can't find the Security settings
- In Element, click your avatar → All settings → Security & Privacy
- If "Secure Backup" isn't there, you may need to update Element

---

## Getting Help

If you're stuck:
1. Check the logs: gorp prints helpful error messages
2. Open an issue: [github.com/2389-research/gorp-rs/issues](https://github.com/2389-research/gorp-rs/issues)

---

## Quick Reference

| What | Value |
|------|-------|
| Your personal Matrix ID | `@yourname:matrix.org` |
| Your bot's Matrix ID | `@yourname-claude-bot:matrix.org` |
| Config file location | `~/.config/gorp/config.toml` |
| Data directory | `~/.local/share/gorp/` |
| Default webhook port | `13000` |
