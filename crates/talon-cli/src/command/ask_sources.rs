use eyre::{Result, WrapErr as _};
use talon_core::{AskSource, SearchResponse, SearchResult, TalonConfig};

#[derive(Debug)]
struct ChunkSource {
    text: String,
    heading_path: Option<String>,
}

pub(super) fn build_ask_sources(
    search_response: &SearchResponse,
    config: &TalonConfig,
    queries: &[String],
) -> Result<Vec<AskSource>> {
    let terms = query_terms(queries);
    let conn = talon_core::open_database_read_only(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let mut sources = Vec::new();

    for result in &search_response.results {
        let chunks = matching_chunks(&conn, result, &terms)?;
        if chunks.is_empty() {
            sources.push(result_to_source(result));
            continue;
        }
        for chunk in chunks {
            sources.push(chunk_to_source(result, chunk));
        }
    }

    Ok(sources)
}

fn matching_chunks(
    conn: &talon_core::Connection,
    result: &SearchResult,
    terms: &[String],
) -> Result<Vec<ChunkSource>> {
    let mut stmt = conn
        .prepare(
            "SELECT c.text, c.heading_path
             FROM chunks c
             JOIN notes n ON n.id = c.note_id
             WHERE n.vault_path = ? AND n.active = 1
             ORDER BY c.chunk_index",
        )
        .wrap_err("preparing ask chunk query")?;
    let rows = stmt
        .query_map([result.vault_path.as_str()], |row| {
            Ok(ChunkSource {
                text: row.get(0)?,
                heading_path: row.get(1)?,
            })
        })
        .wrap_err("querying ask chunks")?;
    let mut chunks = Vec::new();
    for row in rows {
        let chunk = row.wrap_err("reading ask chunk")?;
        if chunk_score(&chunk, terms) > 0 {
            chunks.push(chunk);
        }
    }
    Ok(chunks)
}

fn chunk_to_source(result: &SearchResult, chunk: ChunkSource) -> AskSource {
    let snippet = chunk.heading_path.map_or_else(
        || chunk.text.clone(),
        |heading| format!("{heading}\n{}", chunk.text),
    );
    AskSource {
        vault_path: result.vault_path.clone(),
        title: result.title.clone(),
        snippet,
        score: result.score,
    }
}

fn result_to_source(result: &SearchResult) -> AskSource {
    AskSource {
        vault_path: result.vault_path.clone(),
        title: result.title.clone(),
        snippet: result.snippet.clone(),
        score: result.score,
    }
}

fn chunk_score(chunk: &ChunkSource, terms: &[String]) -> u32 {
    let haystack = format!(
        "{}\n{}",
        chunk.heading_path.as_deref().unwrap_or(""),
        chunk.text
    )
    .to_lowercase();
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

fn query_terms(queries: &[String]) -> Vec<String> {
    let mut terms: Vec<String> = queries
        .iter()
        .flat_map(|query| {
            query
                .split(|c: char| !c.is_alphanumeric())
                .map(str::to_lowercase)
                .filter(|term| term.len() >= 3 && !STOP_WORDS.contains(&term.as_str()))
                .collect::<Vec<_>>()
        })
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

const STOP_WORDS: &[&str] = &[
    "about", "and", "are", "for", "from", "how", "into", "like", "notes", "say", "says", "tell",
    "that", "the", "their", "them", "this", "what", "when", "where", "with", "your",
];

#[cfg(test)]
mod tests {
    use super::*;
    use talon_core::VaultPath;

    #[test]
    fn query_terms_drops_common_words() {
        let terms = query_terms(&["what do my notes say about cooking lamb".to_string()]);
        assert_eq!(terms, vec!["cooking".to_string(), "lamb".to_string()]);
    }

    #[test]
    fn chunk_score_matches_heading_and_text() {
        let chunk = ChunkSource {
            text: "Slow braise until tender.".to_string(),
            heading_path: Some("Lamb Neck".to_string()),
        };
        assert_eq!(
            chunk_score(&chunk, &["lamb".to_string(), "braise".to_string()]),
            2
        );
    }

    #[test]
    fn chunk_to_source_prepends_heading() {
        let result = SearchResult {
            vault_path: VaultPath::parse("wiki/Lamb Neck.md")
                .unwrap_or_else(|err| panic!("valid vault path: {err}")),
            title: "Lamb Neck".to_string(),
            snippet: "fallback".to_string(),
            score: 0.8,
            raw_score: None,
            match_kind: talon_core::MatchKind::Semantic,
            scope: None,
            mtime: None,
            is_index: false,
            citations: vec![],
            links: vec![],
            backlinks: vec![],
            tags: vec![],
            aliases: vec![],
            preview_anchors: None,
        };
        let source = chunk_to_source(
            &result,
            ChunkSource {
                text: "Braise gently.".to_string(),
                heading_path: Some("Cook".to_string()),
            },
        );
        assert_eq!(source.snippet, "Cook\nBraise gently.");
    }
}
