# recall Response Schema

Full field reference for the `talon recall` JSON and prompt-XML outputs.

## JSON (TalonEnvelope)

```json
{
  "action": "recall",
  "version": "0.1.0",
  "ok": true,
  "data": {
    "action": "recall",
    "vaultRecall": {
      "activeNotes": [ NoteExcerpt ],
      "linkedContext": [ LinkedNote ],
      "frontmatter": [ FrontmatterFact ],
      "recentEdits": [ EditedNote ],
      "fuzzyAnchors": [ FuzzyAnchor ]
    },
    "evidenceScore": 0.85,
    "tokensUsed": 1234,
    "excluded": [],
    "excludedByBudget": [],
    "skipped": false
  },
  "meta": {
    "durationMs": 120,
    "resultCount": 5,
    "warnings": [],
    "scopeSet": ["default"],
    "since": "2026-04-19T00:00:00Z"
  }
}
```

### NoteExcerpt

| Field | Type | Description |
|-------|------|-------------|
| `vaultPath` | string | Vault-relative path |
| `title` | string | Display title |
| `snippet` | string | Result snippet (with heading breadcrumb) |
| `score` | f64 | Hybrid score × scope multiplier |
| `rank` | u32 | 1-based rank within active_notes |

### LinkedNote

| Field | Type | Description |
|-------|------|-------------|
| `vaultPath` | string | Vault-relative path |
| `title` | string | Display title |
| `linkText` | string | Raw link text that created this edge |
| `relation` | "outgoing"\|"backlink" | Direction relative to source note |
| `hops` | u8 | Graph hops from top active_note |

### FrontmatterFact

| Field | Type | Description |
|-------|------|-------------|
| `vaultPath` | string | Vault-relative path of containing note |
| `key` | string | Frontmatter key |
| `value` | any | Frontmatter value (string, number, bool, array, etc.) |

### EditedNote

| Field | Type | Description |
|-------|------|-------------|
| `vaultPath` | string | Vault-relative path |
| `title` | string | Display title |
| `indexedAt` | u64 | Last indexed timestamp (ms since epoch) |
| `daysSinceModified` | f64 | Fractional days since modification |
| `score` | f64 | Composite recency+relevance score |

### FuzzyAnchor

| Field | Type | Description |
|-------|------|-------------|
| `vaultPath` | string | Vault-relative path |
| `title` | string | Display title |
| `snippet` | string | Matching text snippet |
| `matchScore` | f64 | Title/alias match score |

## Prompt-XML

### Root element `<vault_recall>`

Attributes:
- `source`: always `"talon"`
- `vault`: vault root path from config
- `evidence_score`: formatted to 4 decimal places

When skipped:
```xml
<vault_recall skipped="true" evidence_score="0.0500"/>
```

### `<active_notes>` children

```xml
<note path="Atlas/Note.md" title="Note Title" score="0.9234">
  Snippet text here
</note>
```

### `<linked_context>` children

```xml
<note path="Graph/Child.md" title="Child" relation="Outgoing" hops="1"/>
```

### `<frontmatter>` children

```xml
<fact path="Filters/Frontmatter.md" key="status">"archived"</fact>
```

### `<recent_edits>` children

```xml
<note path="Atlas/Note.md" title="Note Title" days_ago="1.3" score="0.7200"/>
```

### `<fuzzy_anchors>` children

```xml
<anchor path="Search/Cafe.md" title="Cafe del Sol" score="0.5400"/>
```
