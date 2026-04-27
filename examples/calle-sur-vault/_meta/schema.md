---
title: schema
type: meta
generated_by: agent
generated: 2026-04-27
tags: [meta]
---

# Vault Schema

This document describes the structure, frontmatter conventions, and tagging taxonomy for Marco Reyes' Calle Sur vault. Refer to this when creating or organizing notes.

## Directory Structure

| Directory | Purpose | Privacy | Notes |
|-----------|---------|---------|-------|
| `wiki/` | Foundational knowledge, techniques, references. Timeless. | Public | Growing library. Add new article only when it's genuinely reusable knowledge. |
| `projects/` | Active work: menus, dishes, initiatives, procurement. Living documents. | Public | Regenerated each quarter. Old projects move to `archive/`. |
| `artifacts/` | Generated outputs: costed menus, order sheets, prep lists, pitch notes. | Public | Snapshots in time. Link-rich; reference the project they came from. |
| `daily/` | Daily service notes, brief reflections, walk-throughs. | Public | Ephemeral; archive monthly. Not searchable long-term. |
| `raw/` | Clips, quotes, voice memos, external references. Unprocessed. | Public | Temporary holding. Process into wiki or project, then move to archive. |
| `private/` | Financial, legal, HR, personal. Sensitive material. | Private | Low priority for agent retrieval. Treated as off-limits for default search. |
| `archive/` | Completed projects, old menus, deprecated recipes. Historical. | Public | Reference only. Keeps vault lean. |
| `_meta/` | Vault infrastructure: index, lint reports, schema (this), garden-pass logs. | Public | Agent-maintained. Regenerated regularly. |

## Frontmatter Conventions

### Wiki Articles

```yaml
---
title: Article Name
type: wiki
compiled: true                 # Is this article stable/complete?
sources: [source1.md, source2] # What informed this? (optional)
status: active                 # active | stub | deprecated
tags: [topic, category]
last_updated: 2026-MM-DD
---
```

Use `status: stub` if the article is incomplete. Use `status: deprecated` if it's been superseded. Mark `compiled: false` if it's still draft/research.

### Project Notes

```yaml
---
title: Project Name
type: project
status: active                 # active | planning | paused | complete
priority: high                 # high | medium | low
started: 2026-MM-DD
target_end: 2026-MM-DD         # optional
tags: [project, category]
last_updated: 2026-MM-DD
---
```

### Artifacts (Generated Output)

```yaml
---
title: Artifact Name
type: artifact
source: Project or Note        # What generated this?
generated: 2026-MM-DD
valid_until: 2026-MM-DD        # optional (if time-sensitive)
tags: [artifact, category]
---
```

### Raw Clips/Clips

```yaml
---
title: Clip Name
type: raw
source_url: https://...        # If external
captured: 2026-MM-DD
tags: [raw, source_type]
---
```

### Private Notes

```yaml
---
title: Note Name
type: private
sensitivity: high              # All private notes are "high"
tags: [private, category]      # category: financial, legal, hr, personal
last_updated: 2026-MM-DD
---
```

### Meta Files

```yaml
---
title: File Name
type: meta
generated_by: agent
generated: 2026-MM-DD
tags: [meta]
---
```

## Wikilink Convention

**Use basename-only links.** All notes are linked by their title (without directory path).

✅ Good: `[[Spring 2026 Menu]]`, `[[Lacto-Fermentation]]`  
❌ Bad: `[[projects/Spring 2026 Menu]]`, `[[wiki/Lacto-Fermentation]]`

Obsidian automatically resolves to the correct file. If two files share the same name, add a disambiguator:
- `[[Fermentation Fundamentals]]` (wiki)
- `[[Fermentation (Project)]]` (if ambiguous)

## Tag Taxonomy

### Structural Tags (required per type)

- `wiki` — Wiki articles
- `project` — Active projects
- `artifact` — Generated outputs
- `raw` — Unprocessed clips
- `private` — Sensitive material
- `meta` — Vault infrastructure

### Category Tags (secondary; optional but recommended)

**Food/Technique:**
- `fermentation` — Fermentation, koji, salt
- `sauce` — Sauce mothers, emulsions
- `vegetables` — Vegetable prep, seasonality
- `bread` — Bread, doughs, fermentation
- `plating` — Plating, composition
- `technique` — Knife skills, mise, cooking methods

**Business:**
- `financial` — Revenue, costs, projections
- `legal` — Lease, contracts, liability
- `hr` — Staffing, payroll, scheduling
- `supplier` — Vendors, procurement, sourcing

**Projects/Initiatives:**
- `spring-2026` — Spring 2026 menu work
- `tasting-counter` — Tasting counter build-out
- `hot-sauce` — Hot sauce line
- `foraging` — Foraging program

**Metadata:**
- `status:stub` — Incomplete; needs work
- `status:active` — Currently in focus
- `status:complete` — Done; reference only

Nest when helpful: `financial/payroll`, `technique/knife`, `project/spring-2026`.

## Special Formatting

### Callouts

Use callouts for:
- `> [!warning]` — Risk, deadline, or caution
- `> [!note]` — Clarification or detail
- `> [!quote]` — Marco's voice / personal note (use in private notes)
- `> [!tip]` — Best practice or lesson learned

### Block References

Mark important paragraphs with block IDs for re-linking:

```markdown
This key insight applies elsewhere. ^key-insight

## Another section

See [[Note Name#^key-insight]] for context.
```

## Maintenance

This vault is regenerated weekly via garden pass. See [[last-garden-pass]] for the most recent review. See [[lint-report]] for structure violations or stale notes.

**Guidelines for long-term health:**
- Archive completed projects monthly (move to `archive/`, update `last_updated`).
- Mark `status: stub` or `status: deprecated` when an article goes out of date.
- Link newly created notes to existing wiki articles to avoid duplication.
- Keep `daily/` notes brief; archive weekly.
- Run lint monthly to catch orphans and broken links.

Refer to [[VAULT_INDEX]] for a hand-curated map of high-value notes.
