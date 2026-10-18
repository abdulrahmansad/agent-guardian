# Agent Skill packaging and validation

Research checked 2026-10-18 against the [Agent Skills specification](https://agentskills.io/specification), the official [skills-ref validator](https://github.com/agentskills/agentskills/tree/main/skills-ref), and the [Vercel Skills CLI](https://github.com/vercel-labs/skills).

## Package requirements

- Each skill lives in a directory containing SKILL.md.
- YAML frontmatter requires name and description; name must match the skill directory and use lowercase letters, digits, and hyphens.
- Description explains the capability and activation conditions. Compatibility and license are supported optional fields.
- Additional detail belongs in nearby references. The CLI discovers a skills/ container and supports selecting a skill by name.

This repository provides skills/agent-guardian/SKILL.md, a concise instruction file, and focused references. It states that skill installation and runtime installation are separate.

## Commands

When this repository has a published default branch, install the skill with:

    npx skills add abdulrahmansad/agent-guardian

To select Codex explicitly and suppress interactive prompts:

    npx skills add abdulrahmansad/agent-guardian --skill agent-guardian --agent codex --yes

For local discovery without installation:

    npx --yes skills add ./ --list

Validate the skill package with the reference validator:

    skills-ref validate ./skills/agent-guardian

The reference validator checks package metadata and naming. It does not validate runtime behavior, Codex interoperability, installation safety, or product claims. The Skills CLI local-list check is separate from the official spec validator.

## Current validation limits

The official skills-ref validator reports the local package as valid. The Vercel Skills CLI local-list command discovers exactly one skill named agent-guardian. Public GitHub installation cannot be verified while the configured remote has no refs. A published repository and a clean Codex install/uninstall environment are required for end-to-end validation.
