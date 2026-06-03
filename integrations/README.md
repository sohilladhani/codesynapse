# Codesynapse Integrations

Skill/rule files for AI coding assistants. Use these when MCP is unavailable (e.g., blocked by org policy). The AI will run codesynapse CLI commands via bash instead.

## Claude Code

Copy `claude-code/skills/codesynapse-cli/` into your project's `.claude/skills/`:

```bash
cp -r claude-code/skills/codesynapse-cli/ /path/to/your/project/.claude/skills/
```

The skill auto-triggers when you ask architecture questions ("how does X work", "blast radius of Y", etc.).

## Cursor

Copy `cursor/rules/codesynapse-cli.mdc` into your project's `.cursor/rules/`:

```bash
cp cursor/rules/codesynapse-cli.mdc /path/to/your/project/.cursor/rules/
```

Restart Cursor after copying.

## Prerequisites

Before the skill works, index your repo:

```bash
codesynapse module add myrepo /path/to/your/repo
```

Or build a local graph:

```bash
codesynapse build /path/to/your/repo -o codesynapse-out/graph.json
```

See the main SETUP_GUIDE.md for full setup instructions.
