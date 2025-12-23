# Gorp User Story: Complete User Journey

> **The Core Value:** I have Claude assistants that **live** in Matrix, **remember** everything, and **wake up** on schedule.

---

## Part 1: First Contact

I open my Matrix client and see a new bot in my contacts: `@gorp:myserver.org`. I send it a DM.

**Me:** `!help`

**Gorp responds:**
```
Welcome to Gorp!

I create dedicated Claude channels with persistent workspaces.

Commands:
  !create <name>  - Create a new channel (e.g., !create research)
  !list           - Show your existing channels
  !help           - This message

Each channel gets:
  - Its own Claude session that persists across conversations
  - A dedicated workspace directory for files
  - Webhook URL for automation
```

**Me:** `!list`

**Gorp:** `You don't have any channels yet. Try !create <name> to get started.`

**Me:** `!create pa`

**Gorp:**
```
Created channel: pa

Room: #gorp-pa:myserver.org
Workspace: /workspace/pa/
Session ID: a1b2c3d4

You've been invited. Head over and start chatting!
```

I see an invite notification. I click it and join `#gorp-pa`. This is now my dedicated personal assistant channel.

---

## Part 2: In-Channel Experience

I'm in `#gorp-pa`. I type my first message:

**Me:** `Hey, I'd like you to help me manage my tasks and schedule. I use Todoist and Google Calendar.`

**Claude (via Gorp):**
```
I'd be happy to help you manage tasks and scheduling! I can:

- Help you organize and prioritize tasks
- Draft calendar events
- Remind you of upcoming deadlines
- Help you plan your week

A few questions to get started:
1. What's your typical work schedule?
2. Do you have any recurring meetings I should know about?
3. How do you prefer task prioritization (by deadline, energy level, project)?
```

The response appears as a normal Matrix message from the bot. I reply naturally:

**Me:** `I work 9-5 CST, have standups at 9:30am Mon-Fri, and prefer prioritizing by deadline then energy level.`

**Claude:** `Got it! I'll remember...`

This continues like a normal conversation. But here's the magic: **I close my laptop, go to sleep, come back the next day**, and the context is still there.

**Me (next morning):** `What were we talking about yesterday?`

**Claude:** `Yesterday we set up your task management preferences. You work 9-5 CST with standups at 9:30am Mon-Fri...`

The session persists. Claude remembers everything from this channel.

---

## Part 3: Multi-Channel Workflow

Back in my DM with gorp, I create more channels for different purposes:

**Me:** `!create research`

**Gorp:** `Created channel: research`

**Me:** `!create weather`

**Gorp:** `Created channel: weather`

**Me:** `!create dev-api`

**Gorp:** `Created channel: dev-api`

**Me:** `!list`

**Gorp:**
```
Your channels:
  - pa        - #gorp-pa:myserver.org
  - research  - #gorp-research:myserver.org
  - weather   - #gorp-weather:myserver.org
  - dev-api   - #gorp-dev-api:myserver.org
```

Now I have four separate Claude instances, each with its own:
- **Conversation history** - research doesn't leak into weather
- **Workspace directory** - files are isolated per channel
- **Session context** - each Claude knows only what's relevant

I hop between them naturally:

**In #gorp-research:**
`I'm researching distributed consensus algorithms. Let's start with Raft.`

**In #gorp-weather:**
`I live in Austin, TX. Give me today's forecast.`

**In #gorp-dev-api:**
`I'm building a REST API in Rust with Axum. Help me design the authentication layer.`

Each channel maintains its own identity. The research Claude becomes an expert in my research. The dev Claude knows my codebase. The weather Claude knows my location.

---

## Part 4: Room Commands & Status

Inside any channel, I can check on things:

**Me:** `!status`

**Gorp:**
```
Channel: pa
Room: #gorp-pa:myserver.org
Session ID: a1b2c3d4
Workspace: /workspace/pa/
Created: 2025-01-15 09:23:00

Webhook URL: http://gorp.myserver.org:13000/webhook/session/a1b2c3d4

Scheduled prompts: 0 active
```

The webhook URL is interesting. That's how I can automate this channel from external systems.

**Me:** `!help`

**Gorp:**
```
Room commands:
  !status    - Show channel info and webhook URL
  !help      - This message

Just type normally to chat with Claude.
For scheduling, ask Claude to schedule something for you.
```

---

## Part 5: Webhook Automation

I want my CI/CD pipeline to notify my `dev-api` channel when deployments happen. I grab the webhook URL from `!status` and add it to my GitHub Actions:

```yaml
# .github/workflows/deploy.yml
- name: Notify Gorp
  run: |
    curl -X POST "${{ secrets.GORP_WEBHOOK_URL }}" \
      -H "Content-Type: application/json" \
      -d '{"prompt": "Deployment completed for commit ${{ github.sha }}. Please review the changes and note any potential issues."}'
```

Now after every deployment, a message appears in `#gorp-dev-api`:

**[Webhook]:** `Deployment completed for commit abc123f. Please review the changes and note any potential issues.`

**Claude:** `I see the deployment completed. Based on our recent work on the authentication layer, here are things to verify...`

Claude has context! It knows what we've been working on and can give relevant post-deployment advice.

**Other webhook use cases:**
- Monitoring alerts → Claude analyzes and suggests fixes
- Daily metrics dumps → Claude summarizes trends
- Customer support tickets → Claude drafts responses
- Log aggregation → Claude spots anomalies

---

## Part 6: Scheduled Prompts

I want my `weather` channel to give me a forecast every morning. I just ask Claude:

**Me:** `Can you give me the weather forecast every morning at 7am?`

**Claude:** `I'll schedule a daily weather prompt for 7am CST. Setting that up now...`

**Gorp:** `Scheduled: Daily at 7:00 AM CST`

The next morning at 7am, the channel comes alive:

**[Scheduled]:** `Good morning! Please provide today's weather forecast for Austin, TX.`

**Claude:** `Good morning! Here's today's forecast for Austin...`

I wake up to a fresh weather report waiting in my Matrix client.

**More scheduling examples:**

In `#gorp-pa`:
**Me:** `Remind me in 2 hours to check on the build.`
**Gorp:** `Scheduled: In 2 hours`

In `#gorp-research`:
**Me:** `Every Friday at 4pm, summarize what we learned this week.`
**Gorp:** `Scheduled: Fridays at 4:00 PM CST`

**Me:** `Schedule "check HN for AI news" at 9am on weekdays`
**Gorp:** `Scheduled: Weekdays at 9:00 AM CST`

I can see all my schedules from the CLI:

```bash
gorp schedule list
```

```
Channel    | Prompt                              | Schedule
-----------+-------------------------------------+------------------
weather    | weather forecast                    | Daily 7:00 AM
pa         | check on the build                  | In 1h 43m
research   | summarize what we learned           | Fridays 4:00 PM
research   | check HN for AI news                | Weekdays 9:00 AM
```

---

## Part 7: File Attachments

In `#gorp-research`, I want Claude to analyze a paper. I drag-and-drop a PDF into the Matrix room.

**Me:** `[attached: attention-is-all-you-need.pdf]`
`Can you summarize this paper and explain the key innovations?`

The file lands in the channel's workspace directory (`/workspace/research/`). Claude can access it:

**Claude:** `I've read the paper. "Attention Is All You Need" introduces the Transformer architecture...`

Later:

**Me:** `Compare this to the BERT paper I shared last week.`

**Claude:** `Comparing the two papers... The original Transformer paper focused on machine translation, while BERT...`

Both files are in the workspace. Claude remembers them across sessions.

**The workspace accumulates:**
```
/workspace/research/
├── attention-is-all-you-need.pdf
├── bert-paper.pdf
├── notes-on-transformers.md  (Claude can write here too)
└── comparison-matrix.csv
```

---

## Part 8: Power User Features

### Admin Panel

I browse to `http://gorp.myserver.org:13000/admin` and see a web dashboard:

- All my channels listed with activity timestamps
- Session health status (active/idle/expired)
- Scheduled prompt management (edit, delete, pause)
- Webhook logs showing recent triggers

### MCP Tools

Claude in my channels has access to special tools. In `#gorp-pa`:

**Me:** `Schedule a reminder for tomorrow at 2pm to review PRs.`

Claude doesn't just ask gorp to schedule—it uses the `gorp_schedule_prompt` MCP tool directly:

**Claude:** `I've scheduled a reminder for tomorrow at 2pm CST to review PRs.`

### CLI Management

```bash
gorp rooms sync  # Ensure room names match prefix convention
gorp schedule list  # View all scheduled prompts
gorp schedule clear  # Clear all schedules
```

---

## Summary: The Complete User Journey

```
┌─────────────────────────────────────────────────────────────────┐
│                         DISCOVERY                                │
│  "I need persistent Claude sessions organized by project"        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       FIRST CONTACT                              │
│  DM the bot → !help → !create pa → Join #gorp-pa                │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      DAILY USAGE                                 │
│  Chat naturally → Context persists → Pick up where you left off │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    MULTI-CHANNEL                                 │
│  !create research, !create weather, !create dev-api             │
│  Each channel = isolated Claude with its own workspace          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     AUTOMATION                                   │
│  Webhooks: CI/CD, monitoring, external triggers                 │
│  Schedules: "every morning at 7am", "in 2 hours", cron          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    POWER FEATURES                                │
│  File attachments → Admin panel → MCP tools → CLI management    │
└─────────────────────────────────────────────────────────────────┘
```

---

## Quick Reference

| Action | How |
|--------|-----|
| Create a channel | DM gorp: `!create <name>` |
| List channels | DM gorp: `!list` |
| Check status | In channel: `!status` |
| Chat with Claude | Just type in the channel |
| Attach files | Drag & drop into channel |
| Schedule prompt | Ask Claude: "remind me in 2 hours..." |
| Webhook trigger | POST to webhook URL from `!status` |
| View schedules | CLI: `gorp schedule list` |
| Admin panel | Browse to `:13000/admin` |
