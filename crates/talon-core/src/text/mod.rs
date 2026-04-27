//! Text processing, frontmatter parsing, and markdown chunking helpers.

pub mod chunker;
pub mod frontmatter;
pub mod nfd;
pub mod processing;

pub use chunker::{
    NoteChunk, build_embedding_text, build_heading_path, chunk_markdown, make_chunk_hash,
};
pub use frontmatter::{
    FrontmatterEntry, FrontmatterExtract, FrontmatterReverseIndex, FrontmatterValue,
    FrontmatterValueType, ReverseSourceIndex, WikiLink, extract_wikilinks,
};
pub use nfd::normalize as normalize_text_nfd;
pub use processing::{
    LineSpan, ParsedWikiLink, TOKEN_CHAR_RATIO, estimate_tokens, is_fence_line, is_heading_line,
    normalize_keyword, normalize_vault_path, parse_wikilink, split_lines, strip_heading_text,
    strip_outer_quotes,
};
