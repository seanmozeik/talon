---
title: lint-report
type: meta
generated_by: agent
generated: 2026-04-26
tags: [meta]
---

# Lint Report

Automated lint scan. Run: Sunday, April 26, 2026, 2:00 PM.

> [!note]
> Lint checks: frontmatter completeness, malformed dates, orphan notes (no incoming wikilinks), stale content (>6 months without `last_updated` bump), schema violations.

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Missing frontmatter | 0 | ✓ Pass |
| Malformed dates | 0 | ✓ Pass |
| Orphan notes | 2 | ⚠ Review |
| Stale (>6 months) | 3 | ⚠ Flag |
| Schema violations | 0 | ✓ Pass |

**Vault health:** 47 notes, 94% compliant. No blockers.

## Issues by Category

### Orphan Notes (2)

Notes with no incoming wikilinks. These are isolated; consider linking or deleting.

| Note | Type | Last Updated | Action |
|------|------|--------------|--------|
| [[Voice Memo - Foraging walk 2026-04-12]] | raw | 2026-04-12 | Link to [[Foraging Program]] |
| [[Clip - Atlantic Mag Salt Industry]] | raw | 2026-04-18 | Link to [[Salt Marsh Spring 2026]] or [[Seasonality Calendar]] |

Both are recent and have value. Recommend linking to a project or wiki article.

### Stale Articles (3)

Notes older than 6 months without update. Assume deprecated unless refreshed.

| Note | Type | Last Updated | Age | Action |
|------|------|--------------|-----|--------|
| [[Bread and Doughs]] | wiki | 2025-09-12 | 7 months | Mark `status: reference-only` |
| [[Costing Fundamentals]] | wiki | 2025-10-15 | 6 months | Refresh with Q2 2026 rates |
| [[Service Flow Theory]] | wiki | 2025-11-02 | 5 months | Revisit with [[Spring 2026 Menu]] context |

Mark as stubs or deprecate if no longer active.

### Schema Violations (0)

All frontmatter present and valid. All tags conform to [[schema]]. No CSS class mismatches.

## Missing Frontmatter Details

No notes are missing critical fields. All have:
- `title`
- `type` (wiki, project, artifact, raw, private, meta)
- `tags`
- `last_updated` (or `generated` for meta notes)

## Wikilink Quality

- **Total wikilinks:** 127
- **Broken links:** 0 (all resolved in [[last-garden-pass]])
- **Markdown links (external URLs):** 8 (all valid)

All wikilinks follow basename-only convention per [[schema]].

## Private Notes (Not Listed Here)

Private notes (`sensitivity: high`) are excluded from this report for security. All 4 private notes pass frontmatter and schema compliance. See [[schema]] for private note conventions.

## Recommendations

1. **Immediate:** Link the 2 orphan notes. Takes 5 minutes.
2. **Q2 priority:** Refresh [[Costing Fundamentals]] and [[Service Flow Theory]] with 2026 data.
3. **Mark stale:** Tag [[Bread and Doughs]] as `status: reference-only` since the program is paused.
4. **Archive review:** [[Winter 2025 Menu]] and [[Dish - Cassoulet]] are dormant; consider archiving by end of Q2.

---

**Next lint run:** Sunday, May 3, 2026.

Refer to [[schema]] for full conventions. See [[last-garden-pass]] for content-level maintenance.
