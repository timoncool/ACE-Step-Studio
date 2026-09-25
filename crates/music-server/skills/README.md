# MiniMax Music 3 — Agent Skills

This directory contains agent skills that extend AI coding agents (Claude Code, Cursor, Codex, etc.) with MiniMax Music 3 capabilities. Skills are plain-text, self-contained instruction packs — no external APIs, no runtime dependencies, no model weights required.

## Available Skills

| Skill | What it does | Invocation |
|---|---|---|
| [`music-caption-rewriter`](music-caption-rewriter/SKILL.md) | Turns a brief music description and optional tagged lyrics into a detailed **Music 3.0 Structured Caption** (Global Metadata, Vocal Details, Arrangement) for precise generation control. | `$music-caption-rewriter` |

## Installation

### Option 1: Install with the `skills` CLI (recommended)

From the repository root:

```bash
npx skills add MiniMax-AI/MiniMax-Music3 --skill music-caption-rewriter
```

This downloads the skill into your agent's skills directory and makes it available as `$music-caption-rewriter`.

### Option 2: Manual install

Copy the skill folder into your agent's skills directory:

```bash
# Claude Code (user-level)
cp -r skills/music-caption-rewriter ~/.claude/skills/

# Cursor (project-level)
cp -r skills/music-caption-rewriter .cursor/skills/

# Codex (user-level)
cp -r skills/music-caption-rewriter ~/.codex/skills/
```

The directory name must stay `music-caption-rewriter` — agents locate skills by folder name.

## Basic Usage

Invoke `$music-caption-rewriter` in a supported coding agent with:

- **Caption** (required) — a natural-language music description, e.g. *"A warm acoustic pop song with intimate female vocals, fingerpicked guitar, and a gradual emotional build into a wide final chorus."*
- **Lyrics** (optional) — lyric text containing bracketed section tags such as `[Verse]`, `[Chorus]`, `[Bridge]`, `[Instrumental]`.
- **Constraints** (optional) — desired length, output format, exclusions, or creative direction.

Example prompt:

> Use `$music-caption-rewriter` to turn my music description and optional tagged lyrics into a concise, section-aware Music 3.0 caption.

### Output

The skill returns a structured caption with exactly three sections:

- **Global Metadata** — genre, subgenre, tempo, emotional progression, and production profile.
- **Vocal Details** — lead configuration, timbre, register, delivery, harmony/backing vocals, and restrained vocal effects.
- **Arrangement** — a section-by-section timeline with instrument lifecycles, groove development, and transitions.

JSON or JSONL output is available on request.

The rewritten caption is designed to be passed directly to MiniMax Music 3 — as the `instructions` field of the generation API — alongside the original lyrics in the `input` field. See the top-level [README](../README.md#prompt-enhancement) for the full workflow.

## How It Works

The skill is built on a fully static, text-based library with progressive disclosure — the agent routes to the right style family first and only opens the templates it actually needs:

```
SKILL.md                      Skill instructions and workflow
├── agents/openai.yaml        Agent metadata (display name, default prompt)
├── references/
│   ├── genre-router.md       Entry point: maps genres to style families
│   └── index-*.md            18 family indexes with compact style cards
└── templates/                1,000 full caption templates
```

The genre router maps your genre/mood cues to one of 18 style families (pop & ballad, hip-hop & rap, metal & heavy rock, cinematic orchestral, East Asian modern, jazz & swing, and more). The agent selects up to three references with distinct roles — *Foundation*, *Modifier*, and *Arrangement* — then synthesizes a new, original caption around your brief. Lyric text is never reproduced or rewritten; only bracketed section tags act as musical directives.

## Repository Layout

```
skills/
├── README.md                                  This file
└── music-caption-rewriter/
    ├── SKILL.md                               Skill instructions
    ├── agents/openai.yaml                     Agent metadata
    ├── references/                            Genre router + family indexes
    └── templates/                             Caption template library
```

## Maintenance

- The library is intentionally 100% text-based — no scripts, databases, embeddings, or external services.
- To add a template: add one complete caption file under `templates/`, add one compact card to exactly one family index linked from the genre router, and update that index's family count.
- Keep template IDs matching their filenames and their referenced paths existing.
