---
name: music-caption-rewriter
description: Turn a brief music description and optional tagged lyrics into a professional MiniMax Music 3 structured caption with Global Metadata, Vocal Details, and a section-aware Arrangement. Use when users ask to enhance a music-generation prompt, preserve lyric-section directives, retrieve a similar style from bundled templates, fuse styles, or produce JSON or JSONL caption output.
---

# Music Caption Rewriter

Transform the user's musical intent into a new, generation-oriented structured caption. Find useful references through progressive disclosure: route to a small style family, compare compact cards, then read only the selected complete templates.

Use natural-language reasoning and local text files only. Do not execute scripts, build a database, calculate embeddings, call external APIs, or scan all 1,000 templates.

## Inputs

Accept:

- `Caption`: required natural-language music description.
- `Lyrics`: optional lyrics containing bracketed section or control tags.
- Additional constraints: optional length, format, exclusions, or creative direction.

Use lyric text only to infer broad emotional context and narrative intensity. Never quote, paraphrase, summarize, or reproduce it. Treat only bracketed tags as executable structural, musical, vocal, or production directives.

## Workflow

Follow these stages in order:

1. Build a private Music Brief from the inputs.
2. Resolve explicit constraints and section-local directives.
3. Read [references/genre-router.md](references/genre-router.md).
4. Read one primary family index and, only when useful, one secondary family index.
5. Select up to three references with distinct roles.
6. Read only the complete template files named by those cards.
7. Design a coherent section-by-section timeline.
8. Render and validate the new caption.

Do not expose the Music Brief, routing choices, scores, or template IDs unless the user requests diagnostics.

## Build the Music Brief

Extract only supported or reasonably inferred values:

- macro genre, subgenres, and cultural or market style
- mood and emotional arc
- approximate tempo, meter, and groove
- vocal presence, gender, register, timbre, and delivery
- core instruments and production texture
- section structure and section-specific changes
- spatial character and explicit exclusions

Classify each value internally as `explicit`, `tagged`, `inferred`, or `unspecified`.

Do not invent a precise key, BPM, vocal gender, melodic interval, or production technique when a broader description is sufficient.

Preserve an explicit instrumental request. Do not add vocals. If vocal presence is unspecified, choose a conservative treatment supported by the user's description and the closest style family.

## Resolve Constraints

Apply this precedence:

1. Explicit user requirements and exclusions.
2. Section-local directives from lyric tags, within that section.
3. Strong implications from the user's Caption.
4. Selected reference characteristics.
5. Conservative musical defaults.

A section tag may change its local arrangement without replacing the song's global genre. Preserve a hard user exclusion when a tag conflicts with it.

When two explicit instructions conflict, prefer the more specific and later instruction if the intent remains clear. Otherwise make the smallest musically coherent compromise.

Never silently reverse an explicit vocal gender, instrumental requirement, tempo limit, required instrument, or prohibited element.

## Route by Progressive Disclosure

Read the genre router first. Choose:

- one primary family for a clear genre request
- one primary and one secondary family for an explicit fusion
- at most two plausible families for an ambiguous genre
- the general pop and ballad family when only mood or imagery is available

Use genre, groove, instrumentation, and cultural context as stronger routing signals than generic adjectives such as `emotional`, `epic`, `dark`, or `modern`.

Read only the family indexes selected by the router. Do not inspect every family index, reconstruct a global catalog, or scan every template filename.

## Select References

Compare cards in the selected family indexes using this priority:

1. Genre and subgenre compatibility.
2. Explicit requirements and exclusions.
3. Groove and tempo compatibility, including plausible half-time or double-time relationships.
4. Vocal configuration.
5. Instrumentation.
6. Mood and emotional arc.
7. Production character.

Apply a strong penalty to direct conflicts. Prefer a close musical family over a card that merely shares mood vocabulary.

Select up to three references with different responsibilities:

- `Foundation`: closest overall identity, groove, and songwriting language.
- `Modifier`: best source for a requested secondary genre, vocal character, cultural color, or production texture.
- `Arrangement`: best source for section development, energy contour, transitions, and instrument lifecycle.

Use one or two references when the request is simple. Do not select a weak match merely to reach three.

## Use Templates Safely

Use the Foundation for broad musical identity, the Modifier only for its matched dimension, and the Arrangement reference only for timeline logic.

Do not inherit unsupported details such as a template's exact key, BPM, vocalist, instruments, emotional story, or section order.

Do not copy sentences, distinctive phrases, or a template's complete structure. Synthesize a new caption around the user's brief.

## Plan the Timeline

Build around the user's section tags when present. Otherwise choose only sections appropriate to the style, for example:

`Intro → Verse → Pre-Chorus → Chorus → Verse → Chorus → Bridge → Final Chorus → Outro`

For every included section, state what enters, exits, changes, or intensifies. Keep instrument behavior continuous and make transitions musically plausible.

Create a readable energy arc rather than a static equipment list or a stack of production terminology.

## Output Contract

Write the final caption in English unless the user explicitly requests another language.

Return exactly these three top-level headings in this order:

### Global Metadata

Include genre and subgenres, tempo, emotional progression, and overall sonic and production profile. Use an exact BPM only when explicit or strongly justified; otherwise use a range or qualitative tempo. Include key and scale only when explicit or musically useful.

### Vocal Details

For vocal music, describe the lead configuration, timbre, register, delivery, harmony or backing vocals, and restrained vocal effects.

For instrumental music, state that the piece is instrumental and identify the instrument or texture carrying the lead melodic role.

Do not invent lyrical subject matter or reproduce lyrics.

### Arrangement

Describe the song as a section-by-section timeline. Explain primary and secondary instrument lifecycles, groove development, transitions, embellishments, texture, and spatial effects only where relevant.

Prefer concrete musical changes over decorative prose. Default to approximately 250–450 English words unless the user requests another length.

Do not include a song title, track ID, selected template ID, reasoning trace, or copied lyric line.

## Machine-Readable Output

Return JSON or JSONL only when explicitly requested. Include original inputs and `rewritten_caption`. Include routing diagnostics or selected template IDs only when explicitly requested.

Never include complete template contents in machine-readable output unless the user specifically asks for them.

## Validate Before Returning

Verify that:

- every explicit user constraint is preserved
- every actionable section tag appears in the matching section
- no quoted, paraphrased, or summarized lyric content, title, or track ID appears
- an instrumental request remains instrumental
- vocal gender is not contradicted
- genre and local modifiers coexist coherently
- the three required headings are present
- the arrangement follows a readable timeline
- instruments have coherent entrances, changes, and exits
- exact BPM, key, and technical details are not fabricated
- no template sentence or complete template structure is copied
- the caption is specific enough to guide generation without becoming an essay

Revise once when any check fails, then return only the corrected result.

## Static Library Maintenance

Keep the library entirely text-based. When adding a template:

1. Add one complete Caption file under `templates/`.
2. Add one compact card to exactly one family index linked from the genre router.
3. Record compatible secondary families in that card instead of duplicating it.
4. Confirm that the card ID matches the template filename and that its path exists.
5. Update the family count in that index.

Do not add scripts, generated catalogs, embeddings, vector stores, databases, or external service configuration.
