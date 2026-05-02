use eyre::{Result, WrapErr as _};
use talon_core::{AskSource, SearchResponse, SearchResult, TalonConfig};

#[derive(Debug)]
struct ChunkSource {
    text: String,
    heading_path: Option<String>,
    chunk_index: u32,
    term_matches: u32,
}

#[derive(Debug)]
struct RankedAskSource {
    source: AskSource,
    chunk_rank: u32,
    result_rank: usize,
    chunk_index: u32,
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

    for (result_rank, result) in search_response.results.iter().enumerate() {
        let chunks = matching_chunks(&conn, result, &terms)?;
        if chunks.is_empty() {
            sources.push(RankedAskSource {
                source: result_to_source(result),
                chunk_rank: 0,
                result_rank,
                chunk_index: 0,
            });
            continue;
        }
        for chunk in chunks {
            sources.push(RankedAskSource {
                chunk_rank: chunk.term_matches,
                chunk_index: chunk.chunk_index,
                source: chunk_to_source(result, chunk),
                result_rank,
            });
        }
    }

    sort_ranked_sources(&mut sources);
    Ok(sources.into_iter().map(|ranked| ranked.source).collect())
}

fn sort_ranked_sources(sources: &mut [RankedAskSource]) {
    sources.sort_by(|left, right| {
        right
            .source
            .score
            .total_cmp(&left.source.score)
            .then_with(|| right.chunk_rank.cmp(&left.chunk_rank))
            .then_with(|| left.result_rank.cmp(&right.result_rank))
            .then_with(|| left.chunk_index.cmp(&right.chunk_index))
    });
}

fn matching_chunks(
    conn: &talon_core::Connection,
    result: &SearchResult,
    terms: &[String],
) -> Result<Vec<ChunkSource>> {
    let mut stmt = conn
        .prepare(
            "SELECT c.text, c.heading_path, c.chunk_index
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
                chunk_index: row.get(2)?,
                term_matches: 0,
            })
        })
        .wrap_err("querying ask chunks")?;
    let mut chunks = Vec::new();
    for row in rows {
        let mut chunk = row.wrap_err("reading ask chunk")?;
        chunk.term_matches = chunk_score(&chunk, terms);
        if chunk.term_matches > 0 {
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
            chunk_index: 0,
            term_matches: 0,
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
                chunk_index: 0,
                term_matches: 0,
            },
        );
        assert_eq!(source.snippet, "Cook\nBraise gently.");
    }

    #[test]
    fn ranked_sources_keep_multiple_strong_chunks_ahead_of_weaker_notes() {
        let mut sources = vec![
            ranked_source("weak.md", 0.4, 3, 1, 0),
            ranked_source("strong.md", 0.9, 1, 0, 0),
            ranked_source("strong.md", 0.9, 2, 0, 1),
            ranked_source("middle.md", 0.7, 4, 2, 0),
        ];

        sort_ranked_sources(&mut sources);

        let paths: Vec<&str> = sources
            .iter()
            .map(|ranked| ranked.source.vault_path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["strong.md", "strong.md", "middle.md", "weak.md"]
        );
    }

    fn ranked_source(
        path: &str,
        score: f64,
        chunk_rank: u32,
        result_rank: usize,
        chunk_index: u32,
    ) -> RankedAskSource {
        RankedAskSource {
            source: AskSource {
                vault_path: VaultPath::parse(path)
                    .unwrap_or_else(|err| panic!("valid vault path: {err}")),
                title: path.to_string(),
                snippet: path.to_string(),
                score,
            },
            chunk_rank,
            result_rank,
            chunk_index,
        }
    }
}
