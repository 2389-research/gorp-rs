---
name: crm-management
description: Use when working with contacts, companies, deals, or relationships in the Pagen CRM. This skill ensures proper contact management, duplicate prevention, and interaction logging.
---

# Pagen CRM Management Skill

## When to Use This Skill

Use this skill ANY time you:
- Add contacts from emails or conversations
- Create or update deals
- Log interactions with people
- Search for existing contacts or companies
- Work with relationships between contacts
- Backfill CRM data from email history
- Need to check if a contact already exists

## Core Principles

### 1. Always Check Before Adding
**NEVER add a contact or company without checking if they already exist first.**

```bash
# WRONG - adds without checking
mcp__pagen__add_contact(name="John Doe", email="john@example.com")

# RIGHT - check first
mcp__pagen__find_contacts(query="john@example.com")
# Only add if not found
```

### 2. Associate Contacts with Companies
When you know someone's company, ALWAYS link them:

```bash
# Add company first (if needed)
mcp__pagen__add_company(
  name="Acme Corp",
  domain="acme.com",
  industry="Technology"
)

# Then add contact with company association
mcp__pagen__add_contact(
  name="John Doe",
  email="john@example.com",
  company_name="Acme Corp",
  phone="+1-555-0100"
)
```

### 3. Log Meaningful Interactions
After ANY significant email exchange, meeting, or conversation, log it:

```bash
mcp__pagen__log_contact_interaction(
  contact_id="abc-123",
  note="Discussed Q4 partnership. They're interested in our AI tool. Follow up after Thanksgiving.",
  interaction_date="2025-11-22"
)
```

## Available CRM Tools

### Core Tools
- `mcp__pagen__add_company(name, domain, industry, notes)` - Add new company
- `mcp__pagen__add_contact(name, email, company_name, phone, notes)` - Add new contact
- `mcp__pagen__update_contact(id, name, email, phone, notes)` - Update existing contact
- `mcp__pagen__create_deal(title, company_name, amount, stage, contact_name, initial_note, expected_close_date, currency)` - Create business opportunity
- `mcp__pagen__update_deal(id, title, stage, amount, expected_close_date)` - Update deal status

### Search & Query Tools
- `mcp__pagen__query_crm(entity_type, query, filters, limit)` - Universal search (contact/company/deal/relationship)
- `mcp__pagen__find_contacts(query, company_id, limit)` - Search contacts by name/email
- `mcp__pagen__find_companies(query, limit)` - Search companies by name/domain

### Relationship & Interaction Tools
- `mcp__pagen__log_contact_interaction(contact_id, note, interaction_date)` - Log interactions
- `mcp__pagen__link_contacts(contact_id_1, contact_id_2, relationship_type, context)` - Connect contacts
- `mcp__pagen__find_contact_relationships(contact_id, relationship_type)` - View relationships
- `mcp__pagen__add_deal_note(deal_id, content)` - Add notes to deals

### Advanced Tools
- `mcp__pagen__get_record_or_create(...)` - Get contact or create if missing

## Deal Stages

Use these standard stages for deals:
- **prospecting** - Initial conversations, exploring possibilities
- **qualification** - Determining if it's a real opportunity with potential
- **proposal** - Active proposal or pitch in progress
- **negotiation** - Terms being discussed, getting close
- **closed_won** - Deal completed successfully
- **closed_lost** - Deal didn't happen (still valuable to track why)

## Workflow: Adding Contact from Email

When processing an email with a new person:

```bash
# 1. Check if contact exists
mcp__pagen__find_contacts(query="person@example.com")

# 2. If not found, extract info from email:
#    - Full name
#    - Email address
#    - Company (if mentioned)
#    - Phone (if in signature)
#    - Context about them

# 3. Check if company exists (if applicable)
mcp__pagen__find_companies(query="Example Corp")

# 4. Add company if needed
mcp__pagen__add_company(
  name="Example Corp",
  domain="example.com",
  industry="Technology",
  notes="Context from email conversation"
)

# 5. Add contact with company association
mcp__pagen__add_contact(
  name="Jane Smith",
  email="jane@example.com",
  company_name="Example Corp",
  phone="+1-555-0200",
  notes="Met via introduction from Bob. Interested in AI consulting."
)

# 6. Log the interaction
mcp__pagen__log_contact_interaction(
  contact_id="<returned_id>",
  note="Initial email exchange about AI consulting project. Scheduled call for next week.",
  interaction_date="2025-11-22"
)
```

## Workflow: Creating a Deal

When you identify a business opportunity:

```bash
# 1. Ensure contact and company exist (see above)

# 2. Create the deal
mcp__pagen__create_deal(
  title="AI Consulting Project - Example Corp",
  company_name="Example Corp",
  contact_name="Jane Smith",
  stage="prospecting",
  amount=50000,  # in cents
  currency="USD",
  expected_close_date="2026-03-01",
  initial_note="Jane reached out about implementing AI workflows. Potential 3-month engagement starting Q1 2026."
)

# 3. Log updates as deal progresses
mcp__pagen__add_deal_note(
  deal_id="<deal_id>",
  content="Had discovery call. They need help with LLM integration into existing platform. Sending proposal next week."
)

# 4. Update deal stage when it changes
mcp__pagen__update_deal(
  id="<deal_id>",
  stage="proposal",
  amount=75000  # updated after scope discussion
)
```

## What Counts as a "Deal"?

Track these as deals:
- ✅ Consulting or advisory opportunities
- ✅ Speaking engagements with compensation
- ✅ Book publishing opportunities
- ✅ Partnership or collaboration opportunities
- ✅ Investment opportunities
- ✅ Real estate transactions
- ✅ Major purchases or sales
- ✅ Sponsorships (being a sponsor or receiving sponsorship)

Don't track as deals:
- ❌ Social lunches with no business angle
- ❌ Informational coffee chats
- ❌ Personal favors or introductions
- ❌ Newsletter subscriptions
- ❌ Generic networking

## Email Backfill Best Practices

When backfilling CRM from email history:

1. **Work in time-bounded batches** - Process 1-2 months at a time
2. **Start with SENT emails** - You're usually the initiator, so sent emails have better signal
3. **Then do INBOX** - Catch incoming opportunities you might have missed
4. **Skip noise** - Ignore newsletters, receipts, automated emails
5. **Focus on humans** - Only add real people you interact with
6. **Log context** - Include what was discussed, not just "sent email"
7. **Check for dupes** - Always search before adding

## Common Mistakes to Avoid

### ❌ Adding without checking
```bash
# This creates duplicates!
mcp__pagen__add_contact(name="Bob Jones", email="bob@test.com")
mcp__pagen__add_contact(name="Bob Jones", email="bob@test.com")
```

### ❌ Not associating with company
```bash
# Missing valuable context
mcp__pagen__add_contact(
  name="Jane Smith",
  email="jane@bigcorp.com"
  # Should include company_name="BigCorp"!
)
```

### ❌ Vague interaction logs
```bash
# Not helpful
mcp__pagen__log_contact_interaction(
  contact_id="abc",
  note="Sent email"
)

# Much better
mcp__pagen__log_contact_interaction(
  contact_id="abc",
  note="Sent proposal for Q1 AI workshop series. They're reviewing with team, follow up Dec 5."
)
```

### ❌ Not tracking deals
```bash
# Someone asks about consulting work - this is a deal!
# Don't just log it as an interaction
# Create a deal to track the opportunity
```

## Data Quality Guidelines

### Good Contact Notes
Include:
- How you met them
- What they're interested in
- Any ongoing projects or conversations
- Relevant personal details (timezone, availability preferences)

### Good Company Notes
Include:
- Industry or sector
- Company size if relevant
- What they do (in your own words)
- Any context about your relationship with the company

### Good Interaction Notes
Include:
- What was discussed (specific topics)
- Outcomes or decisions
- Next steps or follow-ups needed
- Date of interaction (use interaction_date parameter)

## Summary Checklist

Before adding to CRM, ask yourself:

- ☐ Did I search to see if this contact already exists?
- ☐ Did I search to see if their company already exists?
- ☐ Did I include company association if I know it?
- ☐ Did I add meaningful notes with context?
- ☐ Is this a deal I should be tracking?
- ☐ Should I log this interaction?
- ☐ Did I include phone number if available?

**Remember**: The CRM is only valuable if the data is clean and contextual. Quality over quantity.
