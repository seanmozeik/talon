# Talon

Talon is a standalone Rust binary for Obsidian vault search, read, sync, related-note traversal, status, and MCP-over-stdio integration.

This repository is currently scaffolded from `2026-04-25-talon-extraction-design.md`. The major architecture questions in that design are resolved; implementation is intentionally skeletal while the Rust port is built out.

## Layout

- `crates/talon-core`: typed contracts, config, constants, and future core search/indexing modules.
- `crates/talon-cli`: `bpaf` CLI, `--agent`, `--skill`, banner/spinner scaffolding, `--mcp` entry point, config loading, and process wiring.
- `skill/SKILL.md`: agent-facing contract copied from the reference implementation.
- `ts/`: thin npm wrapper skeleton for binary resolution and MCP child specs.

## CLI UX Direction

Human output is a product surface. The scaffold uses `anstyle`/`anstream` for terminal styling, `rattles` for indeterminate spinners, and reserves `indicatif` for future sync/embed progress bars. Tables/result cards are intentionally deferred until real search output lands.

## Development

```bash
just verify
cargo run -p talon-cli -- --help
cargo run -p talon-cli -- init
cargo run -p talon-cli -- --skill
```

## Standalone Embedder

Set `inference.base_url` in `~/.config/talon/config.toml` to any TEI-compatible endpoint. Good defaults are Hugging Face `text-embeddings-inference`, Infinity, or ultraclaw's local sidecar if you are running one. Talon only expects `/embed`, `/embed-chunked`, and `/rerank` endpoints with the shapes described in the design doc.

## Chunking

Markdown chunking is configured under `[indexer]` in `talon.toml`:

```toml
[indexer]
chunk_tokens = 512
chunk_overlap = 64
chunk_min_tokens = 16
```

The text-splitter/tokenx chunker strips frontmatter from BM25 and embedding text. Existing indexes will be re-chunked on the next `talon sync`, which also queues the new chunks for embedding. On a large vault, the first post-upgrade sync can spend 30+ minutes re-embedding against a real sidecar.
