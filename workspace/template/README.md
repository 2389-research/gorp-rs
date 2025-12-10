# Channel Template Directory

This directory is used as a template for new Claude channels.

When you create a new channel with `!create <name>`, all files and directories in this template folder will be automatically copied to the new channel's workspace directory.

## What to Put Here

Common things to include in your template:

- **CLAUDE.md** - Project-specific instructions for Claude
- **.mcp-servers.json** - MCP server configurations
- **.gitignore** - Default gitignore for channel directories
- **README.md** - Template README for your channels
- Any other files/directories you want in every new channel

## Example Structure

```
workspace/template/
├── CLAUDE.md              # Claude instructions
├── .mcp-servers.json      # MCP server configs
├── README.md              # Project README template
└── .gitignore             # Gitignore template
```

## How It Works

1. You add files to `workspace/template/`
2. Run `!create my-channel` in a DM with the bot
3. Bot creates `workspace/my-channel/` and copies all template contents
4. Your new channel starts with all your template files ready to go

Delete this README if you don't need it!
