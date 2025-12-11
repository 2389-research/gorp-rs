---
name: email-management
description: Use when handling email tasks (checking inbox, drafting replies, managing threads, adding events to calendar). Prevents common mistakes like broken threading and ensures proper MCP tool usage.
---

# Email Management Assistant

Use this skill when the user asks you to:
- Check inbox or find specific emails
- Draft email replies
- Add events from emails to calendar
- Manage email workflows

## Core Principles

1. **Always draft, never send** - Save emails as drafts so the user can review before sending
2. **Threading is critical** - Replies must appear in the correct conversation thread
3. **Match the user's voice** - Learn their style from examples and memory
4. **Extract structured data** - Pull event details, action items, contact info from emails

## Email Drafting Workflow

### Step 1: Find the Thread

When drafting a reply, ALWAYS search for the original email first:

```
Search for: [subject] from [sender name]
Get: Full email content, thread ID, message ID
```

**Why:** You need context AND threading information. Skipping this = standalone draft instead of threaded reply.

### Step 2: Get Threading Details

Extract from search results:
- `thread_id` - Links all messages in the conversation
- `message_id` - The specific message you're replying to (usually the most recent)

**Critical:** Both are required for proper threading. Missing either = broken thread.

### Step 3: Draft Using the user's Voice

Reference the Email Drafting Style section in CLAUDE.md:

**the user's patterns:**
- Ultra-concise (1-2 lines)
- No signatures or sign-offs
- Lowercase casual tone acceptable
- Action-focused
- No fluff or pleasantries

**Common templates:**
- Scheduling: "are you free early next week? have time monday and tuesday"
- Confirming: "that works perfect"
- Quick response: "sure."
- Declining: "unsubscribe"

### Step 4: Create Threaded Draft

When creating the draft, specify:
- **To:** Recipient's email address (CRITICAL - tool doesn't auto-extract from thread!)
- Thread ID (to keep it in conversation)
- In-Reply-To message ID (the message you're replying to)
- Subject (maintain thread subject, usually "Re: [original]")
- Body (ultra-concise, the user's voice)
- NO signature

**CRITICAL:** Always explicitly provide the `To:` email address. The MCP tool does NOT automatically extract the recipient from the thread - if you omit it, it will create a broken draft with `@example.com` addresses.

**Example tool call structure:**
```
To: phil@philcifone.com
Thread ID: 19a5fc252ad4dd3a
In-Reply-To message ID: 19a711957d96874d
Subject: Re: [EXTERNAL]Re: Cyber Touchpoint
Body: "are you free early next week? traveling wednesday but have time monday and tuesday"
```

### Step 5: Verify Threading

After creating draft, confirm:
- Draft appears in the correct conversation thread
- Subject line maintains thread format
- Draft is saved (not sent)

If threading failed:
1. Get correct thread ID and message ID again
2. Recreate draft with proper IDs
3. Don't just create a new standalone email

### Step 6: Iterate on Feedback

the user will refine wording:
- Update the draft with changes
- Maintain proper threading
- Keep the user's voice consistent

## Calendar Integration

**IMPORTANT:** When emails contain event information, PROACTIVELY add them to the calendar and check for conflicts. Don't wait for the user to ask.

### Step 1: Extract Event Details

Look for:
- Date and time
- Location
- Event title/description
- Any special notes
- Attendees/who's invited

### Step 2: Check Calendar for Conflicts

BEFORE creating the event:
1. Check the user's calendar for the event date
2. Look for conflicts around the event time
3. Note any existing commitments

**Why:** the user needs to know if there's a conflict before committing to the event.

### Step 3: Report Conflicts or Availability

Tell the user:
- "You're free at [time] on [date]" ✅
- "That conflicts with [existing event] at [time]" ⚠️
- Show the relevant portion of the day's schedule

### Step 4: Add Event to Calendar (if appropriate)

If the user confirms or if there's no conflict, create the event with:
- Clear title (include location if helpful)
- Correct date/time with timezone awareness (America/Chicago)
- Location field
- Description noting who invited/context
- Attendees (if it's a meeting with others)

**Example:**
```
Title: "Lunch with Mike Evans, Jonathan Treble at Soho House"
Date: December 3, 2025
Time: 12:00 PM to 1:30 PM (America/Chicago)
Location: Soho House
Description: "Lunch with Mike Evans, Chris Gladwin, Kristopher Kubicki, and Jonathan Treble (former Grubhub employee running for Congress in AZ)"
Attendees: mevans314159@gmail.com, cgladwin@ocient.com, kristopher.kubicki@gmail.com
```

### Step 5: Confirm Addition

Let the user know the event was added and provide:
- Calendar link for verification
- Note any conflicts that were identified
- Confirm if focus time or other blocks need adjusting

### Step 6: Handle Tentative Events with Calendar Holds

For events pending confirmation:
- Create calendar event with "HOLD:" prefix in title
- Example: "HOLD: Call with Jean Labuschagne (pending confirmation)"
- Add "(pending confirmation)" in description
- Update event to remove "HOLD" once confirmed
- Delete if falls through

### Step 7: Timezone Handling for International Contacts

When scheduling with people in other timezones:
- Specify BOTH timezones in the email
- Example: "9am chicago time (4pm zurich)"
- Common contacts and their timezones:
  - Jean Labuschagne: Switzerland (CET/CEST, +7 hours from Chicago)
  - Kohei: Tokyo (JST, +15 hours from Chicago)
- Always use America/Chicago as the user's primary timezone
- Double-check timezone conversion before sending

## Inbox Triage

When checking inbox:

### Step 1: Search for Unread

Get recent unread emails with:
- Sender
- Subject
- Date
- Preview snippet

### Step 2: Categorize

Group mentally by type:
- **Urgent/Today**: Meetings, time-sensitive requests
- **Action needed**: Need response or calendar add
- **FYI**: Updates, newsletters
- **Archive**: No action needed, informational only

**NEVER categorize as "DELETE"** - the user never deletes emails, only archives them.

### Step 3: Summarize Clearly

Present in order of priority:
- Recent/urgent first
- Group related emails (threads)
- Highlight action items
- Note any follow-ups needed

### Step 4: Use Subagents for Bulk Processing

For large triage operations (10+ emails):
- Use Task tool with general-purpose subagent
- Have subagent categorize all emails
- Subagent can draft multiple replies in one go
- More efficient than processing one-by-one

## Bulk Email Processing with Subagents

When the user asks to triage many emails or handle multiple replies:

1. **Use subagent for triage**:
   - Pass clear categorization criteria
   - Have subagent check calendars for scheduling
   - Get comprehensive report back

2. **Use subagent for bulk drafting**:
   - Provide list of emails needing replies
   - Give subagent the user's voice guidelines
   - Subagent creates all drafts with proper threading
   - Review and send

**Example prompt structure for subagent:**
```
Triage all READ emails in inbox. Categorize as:
- ACTION NEEDED (with specific next steps)
- CALENDAR (extract event details, check conflicts)
- ARCHIVE (no action needed)

For ACTION NEEDED, draft replies in the user's voice.
Return comprehensive report.
```

## Common Mistakes to Avoid

### ❌ Creating Standalone Drafts Instead of Threaded Replies
**Problem:** Draft appears as new email, not in conversation thread
**Solution:** Always get thread ID and message ID before creating draft

### ❌ Adding Signatures or Formal Sign-offs
**Problem:** Doesn't match the user's voice
**Solution:** End email with just the message, no "Best," "Thanks," etc.

### ❌ Over-explaining or Adding Fluff
**Problem:** Makes emails longer than necessary
**Solution:** Get straight to the point, trust context is clear

### ❌ Sending Instead of Drafting
**Problem:** the user can't review before it goes out
**Solution:** ALWAYS save as draft, let the user send

### ❌ Forgetting to Extract Calendar Events
**Problem:** the user has to manually add events later
**Solution:** Proactively offer to add events when you see them in emails

### ❌ Being Too Terse for Personal Emails
**Problem:** Some personal/warm emails need more than 1-2 lines to feel appropriate
**Solution:** Match the tone of the incoming email. If someone writes warmly with multiple paragraphs, respond with warmth (3-5 sentences) while keeping the user's casual voice. Balance conciseness with context.

### ❌ DELETING EMAILS
**Problem:** NEVER DELETE EMAIL. EVER.
**Solution:** ALWAYS archive instead of delete. the user never deletes emails.

## Learning the user's Voice

If you need to refresh understanding of the user's email style:

1. Search sent emails for recent examples
2. Look at 15-30 emails to see patterns
3. Note recurring phrases and structures
4. Update understanding of tone and style

**Key indicators you've captured the voice:**
- Email feels casual but effective
- Could be sent from mobile device
- Gets to point in first sentence
- No wasted words

## Integration Checklist

Before completing an email task, verify:

- [ ] Found original email/thread for context
- [ ] Got thread ID and message ID if replying
- [ ] Drafted in the user's voice (ultra-concise, no signature)
- [ ] Created as DRAFT (not sent)
- [ ] Verified proper threading if reply
- [ ] Extracted and added calendar events if present
- [ ] Summarized what was done for the user

## Success Criteria

You've successfully handled email tasks when:
- Drafts appear in correct conversation threads
- the user says "looks good" without needing changes
- Calendar events are added proactively
- Inbox summaries surface what matters
- Process feels efficient and natural

## Remember

Email is personal communication using the user's voice. The goal is to save time while maintaining authentic, effective communication that sounds like the user wrote it.

When in doubt: shorter, more casual, and always draft first.
