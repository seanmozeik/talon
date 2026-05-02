# Graph Intelligence Implementation Plan

**Date:** 2026-05-02
**Status:** implementation plan
**Goal:** make Talon's existing graph-facing commands smarter without adding a noisy `graph`
command family or write-capable link automation.

## 1. Direction

Do not add:

- `talon graph stats`
- `talon graph gaps`
- `talon graph bridges`
- `talon graph communities`
- `talon related --ranked`
- `talon related --explain`
- write-capable `suggest-links`

Instead, build one internal graph intelligence layer and route existing
mechanisms through it:

- `talon related NOTE` becomes ranked and evidence-backed by default.
- `talon recall ...` uses the same ranked graph expansion for linked context.
- `talon search ...` uses graph refinement in the hybrid pipeline without a new
  search mode.
- `talon lint` includes graph health and read-only missing-link findings by
  default.
- MCP keeps `talon_related`; no new graph MCP tools in this plan.
- Agent-facing docs and MCP tool descriptions are updated so agents know that
  search, recall, related, and lint are graph-aware.

The user-facing design principle is: better defaults, not more switches.

## 2. Reference Material

Primary Talon note:

- `docs/2026-04-29-llm-wiki-inspiration.md:7-14` records the LLM Wiki source
  snapshot 
- `docs/2026-04-29-llm-wiki-inspiration.md:166-226` identifies ranked related
  notes as the first useful bridge.
- `docs/2026-04-29-llm-wiki-inspiration.md:502-638` sketches graph relevance,
  Adamic-Adar, source overlap, community diversity, graph expansion, and budget
  diagnostics.
- `docs/2026-04-29-llm-wiki-inspiration.md:789-813` gives the design rules:
  deterministic before LLM, JSON before prose for agents, score explanations
  when scores affect ranking, source provenance as a first-class signal, and no
  structural-page domination.
- `docs/2026-04-29-llm-wiki-inspiration.md:863-880` lists fixture/evaluation
  cases for graph relevance and link suggestions.
- `docs/2026-04-29-llm-wiki-inspiration.md:984-994` recommends ranked related
  notes first, then graph gaps/bridges.

LLM Wiki local source snapshot:

- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/graph-relevance.ts:30-43`
  defines LLM Wiki's relevance constants and type affinity table.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/graph-relevance.ts:247-287`
  combines direct links, source overlap, Adamic-Adar common neighbors, and type
  affinity.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/graph-relevance.ts:289-308`
  scores all other nodes and sorts by relevance.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/wiki-graph.ts:31-113`
  runs Louvain community detection and computes per-community cohesion.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/wiki-graph.ts:159-286`
  builds graph nodes/edges from wiki pages and assigns communities.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/graph-insights.ts:31-102`
  finds surprising cross-community/type/hub edges.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/graph-insights.ts:114-193`
  detects isolated nodes, sparse communities, and bridge nodes.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/enrich-wikilinks.ts:7-24`
  explains why constrained term-to-target suggestions are safer than LLM page
  rewrites.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/enrich-wikilinks.ts:108-157`
  parses constrained JSON link suggestions.
- `/home/yolo/.opensrc/repos/github.com/nashsu/llm_wiki/main/src/lib/enrich-wikilinks.ts:166-210`
  applies only first-occurrence replacements outside frontmatter/existing links.

## 3. Current Talon Context

Useful existing data:

- `notes`: active path, title, tags JSON, aliases JSON, content, mtime, scope.
- `links`: `from_path`, `to_path`, `raw_target`, `heading`, `alias`.
- `note_aliases`, `note_tags`, `note_frontmatter_fields`.
- `note_frontmatter_fields` already stores flattened `sources`, `type`, and any
  other frontmatter field.
- Existing index schema is in `crates/talon-core/src/indexing/migrations.rs:27-128`.

Existing graph behavior:

- `crates/talon-core/src/query/related.rs:93-176` performs BFS over `links`.
- `crates/talon-core/src/query/related.rs:194-241` queries outgoing/backlink
  rows directly from `links`, without joining active notes.
- `crates/talon-core/src/indexer/wiring.rs:121-131` intentionally stores
  unresolved links with `to_path = raw_target` so broken-link lint can report
  them. Any graph ranker must filter candidates to active notes.
- `crates/talon-core/src/query/recall/sections.rs:72-156` builds linked context
  by calling `find_related` for each active note and aggregating source scores.
- `crates/talon-core/src/query/lint.rs:11-52` runs structural lint and already
  re-resolves stale unresolved links before reporting.
- `crates/talon-core/src/sync/mod.rs:172-211` is the right sync boundary for
  graph construction: after full scan, deletion/ignore reconciliation, and
  unresolved-link relinking; before the optional embed pass.
- `crates/talon-core/src/indexing/input.rs:8-19` has lint checks for `all`,
  `orphans`, `broken-links`, `dangling-refs`, and `unreferenced`; graph findings
  should be included in `all` and classified as graph findings.
- `crates/talon-cli/src/cli/related_args.rs:31-45` has only path, depth,
  direction, and scope flags. Keep it that way.
- `crates/talon-cli/src/cli/lint_args.rs:37-44` has a positional check.
  `graph` should be added as a normal lint category, selectable the same way as
  `orphans`, `broken-links`, `dangling-refs`, and `unreferenced`.
- `crates/talon-cli/src/mcp/tool/public.rs:134-145` already advertises a
  `limit` field for `talon_related`, although `RelatedInput` does not currently
  have one. Treat this as a compatibility cleanup, not a new CLI flag. The MCP
  field caps the final ranked result list after scoring; it is not traversal
  depth and should not change graph construction.
- `crates/talon-cli/src/agent_contract.rs` owns the public MCP tool descriptions
  for `talon_search`, `talon_read`, and `talon_related`; update these contracts
  when graph ranking/refinement lands.
- `crates/talon-cli/src/mcp/tool/public.rs` owns MCP input schemas; update
  schema descriptions/defaults so agents understand graph-ranked related results
  and graph-refined search.
- `skill/SKILL.md` is the agent-facing usage contract; update graph navigation,
  search, recall, and lint guidance as part of the implementation.
- Search normally opens the database through a refresh path, so non-fast search
  will update the sync-built graph before querying. `search --fast` opens the
  existing index read-only and should still use the last built graph artifact
  for traversal/refinement when it is present.

Existing parsing helpers useful for read-only suggestions:

- `crates/talon-core/src/text/frontmatter/links.rs:54-105` extracts wikilinks
  outside fenced code and inline code spans, with line numbers.
- `crates/talon-core/src/text/processing.rs:58-91` splits content into line
  spans.
- `crates/talon-core/src/text/processing.rs:125-127` detects fenced code lines.
- `crates/talon-core/src/text/frontmatter/parse.rs:20-53` separates frontmatter
  from body.

## 4. Algorithm Coverage

The implementation goal is not "copy every graph feature LLM Wiki has." The goal
is to implement the graph algorithms that make Talon's existing retrieval,
recall, related-note traversal, and lint more accurate.

Implement in the core plan:

- Graph construction from Talon's SQLite index:
  - active note nodes
  - active-active directed wikilink edges
  - undirected neighbor views for common-neighbor math
  - source/provenance reverse indexes
  - degree and structural-page metrics
  - Louvain community assignments
  - per-community cohesion and top-node summaries
- Relatedness ranking:
  - direct outgoing/backlink strength
  - source-overlap ranking with IDF-style dampening
  - Adamic-Adar common-neighbor scoring
  - weak type/scope affinity
  - structural-page penalties
  - community diversity and bridge-aware boosts/penalties
  - deterministic score breakdowns
- Graph health:
  - isolated/low-degree notes
  - sparse Louvain communities
  - overcentral structural hubs
  - bridge-like notes using community neighbor counts
  - surprising cross-community/type/hub connections
- Read-only missing-link detection:
  - title/alias mention matching
  - constrained LLM term-to-target suggestions when ask-mode inference is
    configured
  - frontmatter/fence/inline-code/existing-link exclusions
  - stable line-numbered lint findings

Out of scope:

- ForceAtlas2 or any visual graph layout. It exists in LLM Wiki for the GUI and
  does not help Talon's CLI/MCP behavior.
- A `talon graph` command family.
- Write-capable link application.

Louvain/community detection is in scope and required. Implement it in Rust as a
Talon-owned algorithm, or use a dependency. The implementation must be
deterministic for a fixed graph: stable node ordering, stable edge ordering,
stable tie-breaking, and stable community renumbering.

## 5. Core Design

Add a new internal module:

```text
crates/talon-core/src/graph/
  mod.rs
  build.rs
  snapshot.rs
  storage.rs
  community.rs
  scoring.rs
  health.rs
  suggest.rs
  tests.rs
```

Initial public core APIs:

```rust
pub struct GraphSnapshot { ... }
pub struct GraphRankInput { ... }
pub struct GraphRankedNode { ... }
pub struct GraphSignalBreakdown { ... }
pub struct CommunityAssignment { ... }
pub struct CommunityInfo { ... }
pub struct GraphBuildInput { ... }
pub struct GraphBuildStats { ... }

pub fn rebuild_graph(conn: &mut Connection, input: &GraphBuildInput) -> Result<GraphBuildStats>;
pub fn load_graph_snapshot(conn: &Connection) -> Result<GraphSnapshot>;
pub fn detect_communities(snapshot: &mut GraphSnapshot) -> Vec<CommunityInfo>;
pub fn rank_related(snapshot: &GraphSnapshot, input: &GraphRankInput) -> Vec<GraphRankedNode>;
pub fn graph_health(snapshot: &GraphSnapshot, input: &GraphHealthInput) -> Vec<GraphHealthFinding>;
pub fn suggest_missing_links(snapshot: &GraphSnapshot, input: &SuggestLinksInput) -> Vec<LinkSuggestion>;
```

Keep this module independent of CLI output. Sync constructs and persists the
graph artifact. Query modules load the latest persisted snapshot and convert
graph outputs into existing response structs; they must not build communities,
source indexes, or graph topology on demand.

## 6. Graph Snapshot

`GraphSnapshot` is a sync-built SQLite artifact. `talon sync`, `refresh_index`,
and any query surface that performs a background refresh rebuild or update the
graph after note/link reconciliation. Search, recall, related, and lint load the
latest persisted graph snapshot; they do not construct or update the graph at
query time.

Fields:

- nodes keyed by `vault_path`
- title
- aliases
- tags
- scope
- optional explicit `type` frontmatter
- normalized `sources`
- active-only outgoing links
- active-only backlinks
- degree in/out/total
- structural-page flag
- Louvain community id
- community cohesion
- bridge/community-neighbor count

Build and loading rules:

- Rebuild graph tables from `notes.active = 1` during sync.
- Include `links` only where `to_path` and `from_path` both map to active notes.
- Preserve `raw_target` and alias/link text for display, but never let unresolved
  placeholder rows become graph candidates.
- Normalize sources with the same path/reference cleanup used by search
  affordances where possible; consider extracting source normalization from
  `query/search_affordances.rs`.
- Build and persist the `source -> citing paths` map needed for source-overlap
  IDF.
- Persist Louvain community assignments, cohesion, top-node summaries, bridge
  counts, and structural-page metrics during sync.
- Persist deterministic read-only missing-link candidates during graph sync.
  When ask-mode inference is configured, also persist validated LLM
  term-to-target candidates from that same sync pass.
- Store the index content version used to build the graph. Normal query paths
  should refresh before reading; `search --fast` can use the last built graph
  read-only and should warn or gracefully skip graph refinement only when the
  graph artifact is missing.
- Use `BTreeMap`/`BTreeSet` where deterministic iteration matters.

Schema changes are required for the persisted graph artifact. Use compact tables
or JSON columns only where that is simpler than over-normalizing. Initial table
shape should cover:

- `graph_meta`: graph build version, timestamp, node count, edge count.
- `graph_nodes`: active note metadata, normalized `sources`, explicit
  frontmatter `type`, degree metrics, structural flag, community id, cohesion,
  and bridge metrics.
- `graph_edges`: active-active directed edges and link strength/count.
- `graph_sources`: normalized source to citing path rows for IDF.
- `graph_communities`: community summaries and deterministic top nodes.
- `graph_missing_links`: read-only deterministic and validated LLM link
  suggestions with path, target, term, line, and provenance.

Consider indexes if profiling shows load time is high:

- `idx_links_from` on `links(from_path)`
- `idx_notes_active_scope` on `notes(active, scope)`
- graph table indexes on source path, target path, community id, and suggestion
  source path.

## 7. Community Detection

Implement Louvain community detection for the active undirected graph.

Inputs:

- node ids from `GraphSnapshot`
- undirected weighted edges derived from active wikilinks
- edge weights based on distinct link rows, capped so repeated links do not
  dominate community assignment
- no source-overlap-only edges. Mirror LLM Wiki's graph construction: community
  topology comes from resolved wikilinks; retrieval/provenance relevance can
  weight existing wikilink edges, but sources do not create community edges by
  themselves.

Required outputs:

- `community_id` per node
- `CommunityInfo` with node count, internal edge density/cohesion, top nodes by
  degree or weighted internal degree
- stable community ids renumbered from `0..N` by deterministic sort key
- node community-neighbor counts for bridge detection

Algorithm requirements:

- modularity-based Louvain optimization
- deterministic iteration order
- deterministic tie-breaking on equal modularity gain
- configurable resolution constant with a Talon-owned default
- max-iteration cap
- convergence threshold
- small-graph fallbacks for 0, 1, or 2 nodes

License and dependency posture:

- Prefer an in-house implementation from the standard Louvain algorithm rather
  than copying graphology or LLM Wiki code.
- A crate may be used only if the API gives stable
  deterministic results. `single-clustering` advertises Louvain/Leiden support
- `leiden-rs` exists, but Leiden is a
  different algorithm. It can be considered only if the project deliberately
  chooses Leiden over Louvain; the plan target remains Louvain because that is
  what LLM Wiki uses (though feel free to push back if leiden is more appropriate)

## 8. Scoring Model

Implement Talon-owned scoring constants. Can copy LLM Wiki's constants.

Candidate sources:

- direct outgoing links from the source
- direct backlinks to the source
- two-hop graph neighbors
- notes sharing frontmatter `sources`
- title/alias mention candidates from read-only suggestions

Signals:

- `direct_out`: count/strength of source -> candidate links
- `direct_backlink`: count/strength of candidate -> source links
- `shared_sources`: IDF-weighted shared sources
- `common_neighbors`: Adamic-Adar over undirected neighbor sets
- `type_affinity`: weak optional signal from explicit `type` frontmatter only
- `community_affinity`: same-community support, cross-community diversity, and
  bridge-node treatment
- `scope_multiplier`: use configured `ScopePriority` scoring behavior; do not
  add a graph-specific scope multiplier
- `structural_penalty`: downweight index/readme/overview/log/schema-style pages
- `hops`: prefer closer candidates when scores are close

Suggested independent starting shape:

```text
score =
  direct_out_weight * direct_out
  + direct_backlink_weight * direct_backlink
  + source_overlap_weight * sum(source_idf)
  + common_neighbor_weight * capped_adamic_adar
  + type_affinity_bonus
  + community_bonus
  - structural_penalty

final_score = score * scope_multiplier
```

Use known graph concepts, and LLM Wiki code:

- Adamic-Adar: sum `1 / ln(max(degree(neighbor), 2))` over common neighbors.
- Source IDF: `1 / ln(2 + citing_note_count)` or another independently chosen
  dampening curve.
- Cap common-neighbor and source-overlap contributions so a single hub or broad
  book cannot dominate.
- Use Louvain communities to diversify recall/search expansions and to explain
  cross-community bridge results.

Sorting:

1. final score descending
2. direct relation before indirect when scores are effectively tied
3. lower hop count
4. deterministic vault path ascending

Diagnostics:

- JSON output should always include `score` and `signals` when graph ranking
  affects the response. This does not require a verbose mode.
- `--agent` output should remain token efficient. Include only fields that
  materially help agent navigation: path, title, relation, compact rounded
  score, link text when present, and `reasons` with at most two labels such as
  `direct_link`, `backlink`, `shared_source`, `common_neighbor`,
  `same_community`, or `bridge`. Do not include full signal breakdowns,
  community tables, cohesion metrics, or other debug diagnostics in agent
  output.
- Human output should remain quiet: path/title/relation, optionally no numeric
  score unless the existing output mode is JSON.

## 9. Related Integration

Change `find_related` from BFS-first to graph-rank-first.

Response changes:

- Add `score: f64` to `RelatedResult`.
- Add `signals: GraphSignalBreakdown` to `RelatedResult` and always serialize it
  in normal JSON output. Agent JSON can use compact reason labels instead of the
  full signal map.
- Because `f64` breaks `Eq`, change `RelatedResponse` and `RelatedResult`
  derives from `PartialEq, Eq` to `PartialEq`, matching `SearchResult`.
- Preserve `count`, `scope`, `mtime`, `link_text`, and `relation`.

Behavior:

- Keep existing CLI flags: path, depth, direction, scope.
- Do not add `--ranked`, `--explain`, or `--limit` to the CLI.
- Honor the MCP schema's existing `limit` field by adding `limit` to
  `RelatedInput` with a default. `RelatedInput.limit` means "maximum number of
  ranked related results returned by the MCP tool." It caps after scoring and is
  separate from `depth`, which controls link-derived candidate expansion radius.
  CLI can omit it.
- Default direction remains `both`.
- If direction is `outgoing` or `backlinks`, direct candidate generation honors
  that direction. Advanced source-overlap candidates should be included only for
  `both` until directional semantics are designed.
- Depth still controls graph expansion radius for link-derived candidates.

Acceptance:

- Direct results still appear for existing fixture tests.
- Results are sorted by graph relevance, not BFS discovery order.
- Unresolved placeholder links never appear as related notes.
- Multiple link rows still affect strength through `count`.

## 10. Recall Integration

After related ranking is implemented, update
`crates/talon-core/src/query/recall/sections.rs:72-156`.

Current behavior calls `find_related` once per active search hit and aggregates
source search scores. Keep that structure but replace the candidate quality with
graph-ranked outputs.

New aggregation:

- For each active note above `LINKED_CTX_MIN_SCORE`, rank graph candidates.
- For each candidate, add `source.score * graph_score * scope_affinity`.
- Keep `scope_affinity` as a recall-only aggregation multiplier. The shared
  graph ranker itself uses configured `ScopePriority`, not recall's hardcoded
  affinity table and not a new graph-specific scope multiplier.
- Keep `source_notes` for MCP suppression compatibility.
- Preserve `LinkedNote` output shape initially; optionally add graph score only
  after consumers can use it.
- Apply a per-source and per-community cap internally to avoid five near-duplicate
  linked notes from the same cluster.

No new recall flags.

Acceptance:

- Existing recall tests still pass.
- Linked context remains budget-trimmable by `aggregated_score`.
- Strong direct graph links continue to surface.
- Shared-source candidates can surface even without explicit wikilinks when they
  are stronger than weak direct neighbors.

## 11. Search Integration

Implement graph-aware search as part of the full build, without adding a new
search mode or command.

Required behavior:

- Load the sync-built graph snapshot when graph refinement is active. Do not
  construct or update graph topology, communities, source maps, or missing-link
  suggestions inside search.
- Normal non-fast search refreshes the index before querying, so that refresh
  updates the persisted graph. `search --fast` reads the existing graph artifact
  and can still run the same graph traversal/refinement as long as the artifact
  exists.
- Use the top lexical/semantic/hybrid hits as graph seeds.
- Rank graph neighbors from strong seeds using the same graph score components
  as `related`.
- Fuse search score and graph score conservatively.
- Use Louvain communities to diversify near-duplicate clusters in the final
  candidate set.
- Keep graph-only expansion bounded so search remains anchored in textual or
  semantic relevance.
- If `SearchInput.related` remains the explicit API field for graph expansion,
  wire it to this behavior rather than leaving it unused.
- Do not add a new CLI flag. Initial default: graph refinement is part of the
  default hybrid pipeline only. Fulltext and semantic modes can opt in through
  an internal/API field such as `SearchInput.related` after behavior is proven.

Guardrails:

- Never let graph-only candidates swamp lexical/semantic relevance.
- Do not expand from weak top hits.
- JSON diagnostics should show graph expansion count and score contribution when
  it affects ranking. `--agent` should stay compact.

## 12. Lint Graph

Add graph as a normal lint category to:

- `crates/talon-core/src/indexing/input.rs`
- `crates/talon-cli/src/cli/lint_args.rs`
- JSON lint output and compact agent finding output as needed
- human lint formatter labels

`graph` is selectable/deselectable like the other lint categories. It should not
be framed as a separate command family.

Graph health is included in default lint behavior:

- `talon lint` runs graph health as part of default/all lint.
- `LintCheck::All` includes graph findings.
- `talon lint graph` can select only graph findings, exactly like
  `talon lint broken-links` selects broken-link findings.
- Agent output should classify graph findings with `check: "graph"` but stay
  terse: path, line when available, and a short actionable message. Normal JSON
  can carry richer metadata later if needed.

Finding categories:

- `graph-isolated`: active non-structural note with very low degree.
- `graph-sparse-area`: deterministic component/local cluster with weak internal
  links.
- `graph-sparse-community`: Louvain community with low internal cohesion.
- `graph-overcentral`: structural page with high degree that is likely
  distorting graph relevance.
- `graph-bridge-thin`: high-bridge note with thin content or low degree inside
  its own cluster.
- `graph-surprising-connection`: cross-community/type/hub edge worth review.
- `graph-missing-link`: read-only suggestion where body text mentions an
  existing title/alias without an existing wikilink.

Output:

- Use existing `LintFinding` initially: `check`, `path`, `message`, `line`.
- Keep messages actionable and stable.
- Example: `possible wikilink: "retrieval augmented generation" -> wiki/rag.md`.
- If richer metadata is added, expose it in normal JSON first. Add it to
  `--agent` only when it is compact and clearly useful.

Graph health should avoid moralizing valid structures. A bridge note is often a
good thing; it should become a finding only when it is brittle, thin, or
overcentral enough to warrant review.

## 13. Read-Only Link Suggestions

Build suggestions during graph sync and expose them through default graph-aware
lint, not as write-capable automation.

Deterministic suggestion path:

- Build target dictionary from active note title, aliases, and basename.
- Skip structural targets by default.
- Scan note body only, not frontmatter.
- Skip fenced code blocks, inline code spans, existing wikilinks, and markdown
  links.
- Require whole-word or conservative boundary match.
- Require target page to exist.
- Limit one suggestion per target per source page.
- Emit line number using `split_lines`.

LLM-assisted read-only suggestion path:

- Use ask-mode inference when it is configured. The `[ask]` model rides the
  existing OpenAI-compatible expansion transport, but uses ask-specific model,
  token, and reasoning settings for constrained term-to-target suggestions.
- Do not defer this path to a later phase: if ask-mode inference is available,
  graph sync should produce validated LLM suggestion candidates.
- Return constrained JSON suggestions only, following the LLM Wiki lesson in
  `enrich-wikilinks.ts:7-24`.
- Validate every term and target deterministically after the LLM returns.
- Emit lint findings only; never edit files.
- Talon still remains read-only unless a separate explicit write workflow is
  approved.

No `--apply`.

## 14. Community And Bridge Handling

Implement community and bridge metrics as first-class internal graph data.

Community metrics:

- Louvain community assignment per active note.
- Cohesion as actual intra-community edges divided by possible intra-community
  edges, matching the LLM Wiki concept.
- Top nodes per community by weighted internal degree.
- Community size and structural-node ratio.

Bridge metrics:

- number of distinct neighboring communities
- weighted cross-community degree
- whether a node is structural
- whether a node is thin relative to its bridge role
- surprising edge score for cross-community/type/hub edges

Use communities primarily for:

- recall diversity
- suppressing hub domination
- lint graph health
- bridge-aware ranking
- search result diversification

Not for:

- a new graph command family
- decorative output

## 15. Implementation Sequence

This is sequenced for implementation and review, not scoped as optional work.
The whole section is the target build.

### Step 0: Attribution And Baselines

- Add this plan.
- Use `examples/config.toml` and the bundled `examples/calle-sur-vault` as the
  canonical end-to-end graph fixture. If graph-quality notes need to be added to
  a vault, add them there.
- Extend the Calle Sur vault with graph ranking coverage for:
  - direct links
  - backlinks
  - a hub page
  - shared sources without direct links
  - a structural index page
  - a sparse cluster
- Add a community fixture with at least two clear Louvain communities and one
  bridge note in the Calle Sur vault.
- Run `just check` before any code phase lands.

### Step 1: Sync-Built Graph Snapshot

- Add graph persistence tables through `crates/talon-core/src/indexing/migrations.rs`.
- Add `crates/talon-core/src/graph/snapshot.rs`.
- Add `crates/talon-core/src/graph/build.rs` and `graph/storage.rs`.
- Load active notes, links, aliases, tags, scopes, `sources`, and explicit
  frontmatter `type`.
- Filter links to active-active edges.
- Hook graph rebuild/update into `run_sync_with_chunker_locked` after
  reconciliation and unresolved-link relinking, before the optional embed pass.
- Ensure `refresh_index` and query-triggered background refreshes rebuild the
  graph, while `search --fast` reads the last built graph artifact without
  writing.
- Add unit tests for unresolved links, active filtering, source maps, and
  structural-page detection.

Commit: `feat(graph): persist sync-built graph snapshot`

### Step 2: Louvain Communities

- Add `graph/community.rs`.
- Implement deterministic Louvain community detection.
- Compute cohesion, top nodes, cross-community edge counts, and bridge metrics.
- Add tests for stable assignments, cohesion, bridge counts, and deterministic
  community id renumbering.

Commit: `feat(graph): detect graph communities`

### Step 3: Graph Scoring

- Add `graph/scoring.rs`.
- Implement candidate generation and signal breakdowns.
- Add tests for:
  - direct link beats two-hop
  - shared source beats unrelated direct hub noise only when IDF supports it
  - hub common neighbors are downweighted
  - community diversity affects broad expansions
  - bridge-aware scoring surfaces useful cross-community notes
  - structural page penalty works
  - configured scope priority affects graph ranking without a new graph-specific
    multiplier
  - deterministic sort order

Commit: `feat(graph): score related notes from graph signals`

### Step 4: Related Uses Graph Ranker

- Update `RelatedResult` and `find_related`.
- Preserve existing human output quietness.
- Update normal JSON and agent JSON.
- Honor the existing MCP `limit` schema as a result cap after graph scoring.
- Update snapshots/tests.

Commit: `feat(related): rank related notes with graph signals`

### Step 5: Recall Uses Graph Ranker

- Replace raw BFS-linked context with graph-ranked candidate aggregation.
- Keep recall response shape unless a score field is necessary.
- Use Louvain communities for diversity and hub suppression.
- Add tests for shared-source linked context, community diversity, and hub
  suppression.

Commit: `feat(recall): use graph-ranked linked context`

### Step 6: Search Uses Graph Refinement

- Wire graph refinement into hybrid search without a new command or mode.
- Load the sync-built graph snapshot rather than constructing graph state inside
  search.
- Use strong top hits as seeds.
- Fuse graph and retrieval scores conservatively.
- Use Louvain communities for result diversification.
- Verify `search --fast` can use the last built graph artifact when available.
- Add ranking regression and integration coverage.

Commit: `feat(search): refine search results with graph signals`

### Step 7: Lint Graph Health

- Add `LintCheck::Graph` as a normal lint category and include it in
  `LintCheck::All`.
- Add `graph` to `crates/talon-cli/src/cli/lint_args.rs` so it can be selected
  exactly like existing categories.
- Add `graph/health.rs`.
- Implement isolated, sparse-community, overcentral, brittle-bridge, and
  surprising-connection findings.
- Include graph findings in `LintCheck::All` and default `talon lint`.
- Add human and agent output tests.

Commit: `feat(lint): add graph health findings`

### Step 8: Read-Only Missing-Link Suggestions

- Add `graph/suggest.rs`.
- Build deterministic suggestions during graph sync and expose them through
  default graph-aware lint.
- Reuse frontmatter/body parsing and code/link exclusion helpers.
- Add constrained LLM read-only suggestions during graph sync when ask-mode
  inference is configured.
- Add tests from the inspiration doc's suggestion evaluation list.

Commit: `feat(lint): suggest missing wikilinks read-only`

### Step 9: Agent And MCP Contract Updates

- Update `skill/SKILL.md`:
  - search guidance should mention graph-refined ranking when relevant.
  - graph navigation should describe `related` as ranked graph/provenance
    exploration, not raw traversal.
  - recall guidance should mention community-diverse graph context.
  - lint guidance should say default lint includes graph health and read-only
    missing-link opportunities, and that `lint graph` is just the category
    selector for graph findings.
- Update `crates/talon-cli/src/agent_contract.rs` descriptions:
  - `talon_search` should mention graph-aware refinement.
  - `talon_related` should mention ranked related notes from links, backlinks,
    sources, common neighbors, and communities.
  - avoid promising write behavior.
- Update `--agent` output carefully:
  - related output adds a rounded score and `reasons` with one or two compact
    labels.
  - lint graph output should stay on the existing compact finding shape where
    possible.
  - do not add full signal maps, community/cohesion tables, or debug metrics to
    `--agent`.
- Keep normal `--json` output complete: graph-ranked responses include signals
  without needing a verbose mode.
- Update MCP schemas/descriptions in `crates/talon-cli/src/mcp/tool/public.rs`
  as needed:
  - describe `related.limit` as the MCP-only result cap for ranked related
    output.
  - keep no new `talon_graph_*` tools.
- Update MCP/tool tests that assert descriptions or schema fields.

Commit: `docs(agent): document graph-aware Talon tools`

## 16. Test Plan

Unit tests:

- `graph::snapshot`/`graph::storage`: active filtering, unresolved filtering,
  source map, structural detection, graph version metadata, and missing-artifact
  fallback.
- `graph::community`: Louvain modularity improvement, deterministic assignments,
  stable community renumbering, cohesion, top nodes, bridge counts, and source
  overlap not creating community edges.
- `graph::scoring`: direct, backlink, source overlap, Adamic-Adar, caps,
  community diversity, bridge-aware scoring, configured scope priority,
  deterministic ties.
- `graph::health`: isolated, sparse community, overcentral, brittle bridge,
  surprising connection.
- `graph::suggest`: no frontmatter, no fences, no inline code, no existing
  wikilinks, line numbers, one suggestion per target.
- agent output tests assert graph additions stay compact and omit detailed
  signal/community diagnostics.

Integration tests:

- `talon sync --config examples/config.toml` builds graph tables for
  `examples/calle-sur-vault`.
- `talon related` against the Calle Sur graph fixture hub still returns expected
  direct graph results.
- Related ranking orders fixture candidates by graph quality.
- Default `talon lint` emits stable graph findings.
- Recall linked context improves source-overlap case without adding unrelated
  hub context.
- Hybrid search refinement improves graph-relevant candidates without displacing
  strong lexical/semantic matches.
- `search --fast` can use the last sync-built graph artifact for the same graph
  traversal/refinement path when the artifact exists.
- Louvain communities are stable on fixture vaults.
- MCP `talon_related` output remains compact, valid, and accurately described.
- `skill/SKILL.md` and MCP tool descriptions match the new graph-aware behavior.

Regression/eval:

- Extend the existing graph entries in `crates/talon-core/tests/fixtures/golden-set.json`.
- Use `examples/config.toml` and `examples/calle-sur-vault` for graph-quality
  integration/eval coverage. Any added vault notes belong in
  `examples/calle-sur-vault`.
- Run `just check` for each step.

## 17. Risks And Mitigations

Risk: source overlap over-ranks broad source pages.

- Mitigation: IDF weighting, cap contribution, structural penalties, fixture
  with broad source.

Risk: structural pages dominate common-neighbor scoring.

- Mitigation: structural-page flag, degree caps, Adamic-Adar, explicit tests.

Risk: related output becomes noisy for humans.

- Mitigation: keep human output quiet; put detailed score diagnostics in normal
  JSON. For `--agent`, expose only compact, high-signal fields that help
  navigation.

Risk: graph health becomes vague lint spam.

- Mitigation: include only actionable graph findings in default lint, cap noisy
  categories, and prefer missing-link/isolated/sparse findings over abstract
  stats.

Risk: recall latency increases.

- Mitigation: compute topology, communities, source maps, and suggestion
  candidates during sync; query handlers load the persisted `GraphSnapshot` once
  per request, cap candidates per source, and benchmark related/recall/search.

Risk: sync latency increases from graph construction.

- Mitigation: keep deterministic graph construction cheap and report graph build
  stats in sync diagnostics.

Risk: Louvain output becomes unstable across equivalent graphs.

- Mitigation: deterministic node/edge ordering, deterministic tie-breaking,
  stable community renumbering, fixture tests for repeatability, and no random
  initialization.

## 18. Resolved Decisions

- Resolved: `RelatedInput.limit` is added for MCP compatibility only. It caps
  the final ranked related result list and is not exposed as a CLI flag.
- Resolved: graph `signals` are always included in normal `--json` output when
  graph ranking affects the response. No verbose mode is needed for this.
- Resolved: type affinity uses only explicit `type:` frontmatter.
- Resolved for the initial build: graph-refined search is default for hybrid
  search only. Fulltext and semantic modes can opt in later through the API or
  config after evaluation.
- Resolved: `--agent related` adds a rounded score plus `reasons` containing at
  most two compact labels. It does not include full signal maps, community
  tables, or debug metrics.
- Resolved: the shared graph ranker uses configured `ScopePriority`; recall
  keeps its existing `scope_affinity` as a recall-only aggregation multiplier.
  Do not add a graph-specific scope multiplier.
- Resolved: Louvain communities use wikilink topology only, matching LLM Wiki.
  Existing wikilink edges may be relevance-weighted, but source overlap does not
  create community edges.

## 19. Whole Build Definition Of Done

The plan is complete when all of these are true:

1. `GraphSnapshot` loads the sync-built active graph and provenance data from
   SQLite.
2. Sync constructs and persists graph topology, source maps, missing-link
   candidates, Louvain communities, cohesion, top nodes, and bridge metrics
   deterministically.
3. `related` is backed by graph relevance, not BFS ordering.
4. `recall` uses graph-ranked and community-diverse linked context.
5. Hybrid `search` uses graph refinement without a new command or mode, and
   `search --fast` can use the last sync-built graph artifact when present.
6. Default lint reports graph health, sparse communities, bridges, surprising
   connections, and missing-link opportunities.
7. Read-only missing-link suggestions are deterministic.
8. `skill/SKILL.md`, MCP tool contracts, and MCP schemas describe the new
   graph-aware behavior accurately.
9. Human output stays quiet; normal JSON exposes scores and signals where graph
   ranking affects output; `--agent` stays compact and only includes high-signal
   additions.
10. `just check` passes.
