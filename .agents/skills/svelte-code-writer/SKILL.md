---
name: svelte-code-writer
description: CLI tools for Svelte 5 documentation lookup and code analysis. MUST be used whenever creating, editing or analyzing any Svelte component (`.svelte`) or Svelte module (`.svelte.ts`/`.svelte.js`). If possible, this skill should be executed within the `svelte-file-editor` subagent for optimal results.
---

## CLI tools

You have access to `@sveltejs/mcp` CLI for Svelte-specific assistance. Use these commands via `npx`:

### List documentation sections

```bash
npx @sveltejs/mcp@0.1.26 list-sections
```

Lists all available Svelte 5 and SvelteKit documentation sections with titles and paths.

### Get documentation

```bash
npx @sveltejs/mcp@0.1.26 get-documentation "<section1>,<section2>,..."
```

Retrieves full documentation for specified sections. Use after `list-sections` to fetch relevant docs.

**Example:**

```bash
npx @sveltejs/mcp@0.1.26 get-documentation '$state,$derived,$effect'
```

### Svelte autofixer

```bash
npx @sveltejs/mcp@0.1.26 svelte-autofixer "<code_or_path>" [options]
```

Analyzes Svelte code and suggests fixes for common issues.

**Options:**

- `--async` - Enable async Svelte mode (default: false)
- `--svelte-version` - Target major version: 4 or 5 (default: 5). The pinned CLI 0.1.26 accepts only the major version, not a full semantic version. Use project package versions when checking whether a suggested API is available.

**Examples:**

```bash
# Analyze inline code (single quotes preserve literal dollar signs)
npx @sveltejs/mcp@0.1.26 svelte-autofixer '<script>let count = $state(0);</script>'

# Analyze a file
npx @sveltejs/mcp@0.1.26 svelte-autofixer ./src/lib/Component.svelte

# Target Svelte 4
npx @sveltejs/mcp@0.1.26 svelte-autofixer ./Component.svelte --svelte-version 4
```

**Important:** Use single quotes around inline code or documentation topics containing `$state`, `$derived`, or `$effect`. Do not escape dollar signs inside single-quoted shell arguments.

## Workflow

1. **Uncertain about syntax?** Run `list-sections` then `get-documentation` for relevant topics
2. **Reviewing/debugging?** Run `svelte-autofixer` on the code to detect issues
3. **Always validate** - Run `svelte-autofixer` before finalizing any Svelte component


## Local repository integration

This installation is pinned and adapted for this repository. Follow user instructions and applicable AGENTS.md before this skill.

- Preserve existing changes and processes. Inspect baselines with git show, git diff, and git blame at explicit refs; never switch the user's checkout for an audit.
- Commit, stage, push, publish, deploy, send external messages, or alter infrastructure only when the user explicitly requests that action. An audit request authorizes findings, not automatic fixes.
- Preserve meaningful test coverage; never delete tests merely because an extraction or example workflow suggests it.
- Do not assume Claude plugin agents, hooks, commands, or allowed-tools metadata are registered or grant access in Codex. Use available host tools; optional plugin roles require explicit adaptation.
- Do not run unpinned downloaded CLIs. Check the project's versions, inspect dependencies, and use an explicitly pinned version when a tool is needed. Do not pass secrets or private source to external services without authorization.
- Follow the host's subagent policy. Invoke additional design/refactoring workflows only within the requested scope.
