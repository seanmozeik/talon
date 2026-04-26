# Talon Phase 1: Chunker Implementation

Port the chunker from TypeScript (`/tmp/talon-scaffold-imports/ultra/edge/src/services/talon/shared/chunker.ts` + `text.ts`) to Rust. Pure functions, no DB dependency.

## Reference TS code

- `shared/text.ts` (100 lines) — line splitting, fence/heading detection, token estimation, wikilink parsing
- `shared/chunker.ts` (297 lines) — block-based segmentation, chunk creation, overlap logic
- `shared/chunker-types.ts` — types

## Tasks

### 1.1 Implement `text.rs` in `talon-core`
- [x] `split_lines()` — split content into LineSpan structs (start, end, lineNumber, text, breakLength)
- [x] `is_fence_line()` — regex `^(\`{3,}|~{3,})\s*.*$`
- [x] `is_heading_line()` — regex `^#{1,6}\s+(.*)$`
- [x] `strip_heading_text()` — remove leading `#` markers and trailing `#` markers
- [x] `estimate_tokens()` — `max(1, ceil(text.len() / 4))`
- [x] `normalize_keyword()` — NFD normalize + lowercase + trim
- [x] `normalize_vault_path()` — backslashes to forward slashes + NFD
- [x] `parse_wikilink()` — `[[target|alias]]` or `[[target#heading]]`

### 1.2 Implement `chunker.rs` in `talon-core`
- [x] `collect_blocks()` — walk content, produce MarkdownBlock list (fence/heading/paragraph)
- [x] `chunk_blocks()` — chunk blocks into NoteChunk with overlap
- [x] `chunk_markdown()` — heading-aware chunking with section flush
- [x] `build_heading_path()` — headings joined by ` > `
- [x] `build_embedding_text()` — `Title: {title}\nPath: {path}\nHeadings: {path}\n\n{text}`
- [x] `make_chunk_hash()` — SHA-256 of raw text

### 1.3 Tests
- [x] Write tests first (failing), then implement
- Test on fixture vault files from `/tmp/talon-scaffold-imports/ultra/edge/src/tests/fixtures/talon/`
- `just test` must pass

### 1.4 Quality gate
- `just check` passes
- `cargo clippy --workspace -- -D warnings` clean
- `just test` passes