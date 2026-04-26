# talon recall

`talon recall` is a composite query command that retrieves vault-native context
for agent lifecycle hooks.  It fans out to five existing query modules and
packs the results into a token-budgeted payload with a calibrated evidence score.

## Usage

```
talon recall <message...> [flags]
```

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--since <ts>` | 7 days ago | Time window for recent_edits |
| `--budget-tokens <N>` | 2000 | Token budget for the response |
| `--exclude <path>` | (none) | Vault paths to suppress (repeatable) |
| `--scope <name>` | (none) | Scope names to include |
| `--depth <1-3>` | 1 | Link graph traversal depth |
| `--recency-half-life-days <N>` | 7 | Half-life for recency scoring |
| `--min-confidence <0-1>` | 0.0 | Minimum evidence score threshold |
| `--fast` | false | Skip expansion and rerank |

## Section Semantics

Each section maps to an existing query module:

### active_notes
Results from the hybrid search pipeline (US-005) applied to `<message>`.
Ordered by hybrid score × scope-priority multiplier.

### linked_context
Notes reachable via the link graph (BFS, direction=Both per US-008) from the
top active_note.  Both outgoing links and backlinks are traversed, giving 360°
context.

### frontmatter
Structured key-value facts extracted from `note_frontmatter_fields` for each
active_note.  Useful for type/status/tag metadata.

### recent_edits
Notes modified since `--since`, ordered by a composite score:
```
score = 0.6 * topic_relevance + 0.4 * exp(-days_since_modified / half_life_days)
```
Where `topic_relevance = 1.0` if the note appears in active_notes, else 0.3.

### fuzzy_anchors
Title/alias matches that scored _below_ the main active_notes threshold.
These are candidates the hybrid pipeline didn't rank highly but that share
vocabulary with the query.

## Evidence Score

The `evidence_score` field summarizes retrieval confidence in [0, 1]:

```
evidence_score =
  0.45 * top_rerank_score
+ 0.20 * top_lexical_indicator
+ 0.15 * graph_density_bonus       // min(1, link_count / 5)
+ 0.10 * recency_bonus             // exp(-days / 14)
+ 0.10 * frontmatter_match_indicator
```

Attribution: weight design inspired by MemoryBank (Ebbinghaus decay),
Mem0/Zep dual-signal composites, and the Confidence Gate pattern from the RAG
literature.  Weights are v1 calibration — re-tuned via the US-022 eval suite.

### Confidence Gate

When `evidence_score < --min-confidence` or zero results are returned, the
response is:
```json
{ "vault_recall": null, "evidence_score": 0.18, "tokens_used": 0, "skipped": true }
```

In `--format prompt-xml` this renders as:
```xml
<vault_recall skipped="true" evidence_score="0.1800"/>
```

## Token Budget

Budget enforcement is greedy across sections in priority order:
`active_notes → linked_context → frontmatter → recent_edits → fuzzy_anchors`

Items dropped during trimming appear in `excluded_by_budget`.
Token counting uses `tokenx-rs` (same estimator as the chunker).

## Formats

### JSON (default)

Full `TalonEnvelope` with `data.vaultRecall` containing all five sections.
See `docs/recall-schema.md` for the complete field reference.

### Prompt-XML (`--format prompt-xml`)

A `<vault_recall>` block ready for direct agent context injection:
```xml
<vault_recall source="talon" vault="/path/to/vault" evidence_score="0.8734">
  <active_notes>…</active_notes>
  <linked_context>…</linked_context>
  <frontmatter>…</frontmatter>
  <recent_edits>…</recent_edits>
  <fuzzy_anchors>…</fuzzy_anchors>
</vault_recall>
```

See `docs/recall-schema.md` for element shapes and `examples/recall-output.xml`
for a full example.

## MCP Usage

The `recall` action is available via `tools/call` with action `"recall"`:
```json
{
  "action": "recall",
  "message": "what was I working on yesterday?",
  "budgetTokens": 2000,
  "fast": false
}
```

## Integration Patterns

### Hermes Memory Provider

The `integrations/hermes-talon-recall/` plugin wraps `talon recall --format prompt-xml`
and returns the XML block directly to the Hermes agent context.

### Hand-rolled hook

```python
import subprocess, os

def recall(message: str, budget: int = 2000) -> str:
    result = subprocess.run(
        ["talon", "recall", message, "--format", "prompt-xml",
         "--budget-tokens", str(budget)],
        capture_output=True, text=True,
        env={**os.environ, "TALON_VAULT": "/path/to/vault"}
    )
    if result.returncode != 0 or not result.stdout.strip():
        return ""
    return result.stdout
```
