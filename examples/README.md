# Talon examples

Material for hands-on dogfooding, demos, and reproducing the investigations in `docs/2026-04-27-*.md`. Not test fixtures (the unit-test fixture lives at `crates/talon-core/tests/fixtures/vault/`).

| Path                  | What it is |
|-----------------------|------------|
| `calle-sur-vault/`    | A 78-note synthetic Obsidian vault built around a fictional chef-restaurateur. Karpathy LLM-Wiki layout (`wiki/`, `projects/`, `artifacts/`, `daily/`, `raw/`, `private/`, `archive/`, `_meta/`). |
| `config.toml`         | Talon config with the Karpathy scopes preset wired up, pointing at `calle-sur-vault/` out of the box. Copy to `~/.config/talon/config.toml` (or use directly with `talon -c examples/config.toml ...`). |
| `recall-output.xml`   | Sample `talon recall --format prompt-xml` output for reference. |

## The vault — Calle Sur

### Persona

Marco Reyes — chef-restaurateur, 38, head chef and co-owner of Calle Sur, a 40-seat seasonal restaurant in the Cala neighbourhood of a coastal city. Spanish/Latin influence, vegetable-forward, fermentation-heavy. Open ~14 months. Staff: sous chef Lúcia, sommelier David, line cook Jonas, pastry Ana, GM Renata. Suppliers: Salt Marsh Farm (Maria), Costera Fish (Renato), Vega Dairy (Andrés). Co-owner is silent investor Andrés Vega.

The vault is meant to feel tended for ~14 months — extensive cross-linking, consistent frontmatter, varied note ages, real depth.

## Structure

Karpathy's LLM Wiki layout, mapped to Talon's `ScopePriority` tiers:

| Directory     | Files | Role                                  | Talon scope priority | `default` |
|---------------|------:|----------------------------------------|----------------------|-----------|
| `wiki/`       |    18 | Compiled, agent-curated knowledge      | `boosted` (3.0×)     | `true`    |
| `projects/`   |    14 | Active workspaces                      | `elevated` (1.5×)    | `true`    |
| `artifacts/`  |     4 | Agent outputs for the user             | `normal` (1.0×)      | `true`    |
| `daily/`      |    20 | Ephemeral daily notes                  | `muted` (0.3×)       | `false`   |
| `raw/`        |    10 | Untreated source material              | `muted` (0.3×)       | `false`   |
| `archive/`    |     3 | Completed/closed projects              | `buried` (0.05×)     | `false`   |
| `private/`    |     4 | Sensitive (lease, payroll, financial)  | `buried` (0.05×)     | `false`   |
| `_meta/`      |     5 | Vault infrastructure                   | `buried` (0.05×)     | `false`   |

`_meta/schema.md` documents the vault's own conventions.

## How it was generated

Five Haiku subagents in parallel, each owning a directory or pair of directories, briefed with:

- The persona above (verbatim).
- A canonical filename manifest so cross-vault wikilinks resolve.
- The `obsidian-markdown` skill (read-before-write).
- Per-type frontmatter conventions (wiki articles get `compiled`/`sources`; projects get `status`/`priority`; daily notes get `covers`; etc.).
- A loose narrative arc (Spring 2026 menu R&D → tasting → menu launch) so notes hang together.

## Quick start

`examples/config.toml` is wired up to use this vault out of the box. From the repo root:

```bash
# Use the example config directly via -c.
talon -c examples/config.toml sync
talon -c examples/config.toml --agent search "fermented hot sauce"
talon -c examples/config.toml --agent search "hot sauce launch readiness"
talon -c examples/config.toml --agent recall "what's the lamb dish for spring"
talon -c examples/config.toml --agent lint
talon -c examples/config.toml --agent related "wiki/Lacto-Fermentation.md"
```

Or copy `config.toml` to `~/.config/talon/config.toml` and drop the `-c` flag.

Indexing takes ~37 seconds end-to-end on the configured TEI sidecar (78 files, 1024-dim embeddings).

The reports in `docs/2026-04-27-*.md` were generated against a copy of this vault at `/tmp/talon-dogfood-vault/`. The findings reproduce against `examples/calle-sur-vault/` since it's the same vault.

## Caveats

- Frontmatter dates (`compiled`, `last_updated`, `archived`) are plausible but not real — they don't reflect filesystem mtime. A few haiku-introduced typos are present and surface as `lint broken-links` findings (this is realistic — real vaults have these).
- Tag taxonomy is shallow and has overlap (`fermentation` vs `ferment`, `costing` vs `financial`) — that's intentional; real vault gardening flags these via `_meta/last-garden-pass.md`.
- 100% fictional. No real chefs, restaurants, suppliers, or recipes.

## Related reading

- `docs/2026-04-27-dogfood-findings.md` — what surfaced from running the agent-mode CLI against this vault.
- `docs/2026-04-27-memory-landscape-research.md` — 2026 agent-memory landscape + injection-mechanics deep dive.
- `docs/2026-04-27-plan-vs-implementation.md` — original Talon spec vs current code.
