//! Link graph types and resolution logic.
//!
//! Implements wikilink resolution against the indexed note set, producing
//! directed edges for the link graph with backlink computation.

use crate::text::frontmatter::{WikiLink, normalize_keyword, normalize_vault_path};
use serde::{Deserialize, Serialize};

// ── Link graph types ────────────────────────────────────────────────────────

/// A resolved link between two notes in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedLink {
    /// Source note path (where the link appears).
    pub from_path: String,
    /// Target note path (where the link resolves to).
    pub to_path: String,
    /// Display alias (if `[[target|alias]]`).
    pub alias: Option<String>,
    /// Section heading anchor (if `[[target#heading]]`).
    pub heading: Option<String>,
    /// Raw target text (before resolution).
    pub raw_target: String,
}

/// A note reference used for wikilink resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteReference {
    /// Vault-relative path.
    pub vault_path: String,
    /// Note title (from frontmatter or filename).
    pub title: Option<String>,
    /// Normalized aliases from frontmatter.
    pub aliases: Vec<String>,
}

/// Link graph edge for database storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkEdge {
    /// Source note path.
    pub from_path: String,
    /// Target note path.
    pub to_path: String,
    /// Whether the target was resolved.
    pub resolved: bool,
    /// Raw target text.
    pub raw_target: String,
    /// Display alias.
    pub alias: Option<String>,
    /// Section heading anchor.
    pub heading: Option<String>,
}

/// Link graph statistics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkGraphStats {
    /// Total number of links (edges).
    pub total_links: u32,
    /// Number of resolved links.
    pub resolved_links: u32,
    /// Number of unresolved links.
    pub unresolved_links: u32,
    /// Number of unique target paths.
    pub unique_targets: u32,
    /// Number of nodes with no outgoing links.
    pub isolated_nodes: u32,
}

/// Resolves a single wikilink target against the note reference set.
///
/// Returns the resolved vault path, or `None` if the target doesn't match any note.
///
/// # Algorithm
/// 1. Normalize the target (NFD, lowercase, remove `.md` suffix).
/// 2. For each note, check:
///    - Path match (exact, with/without `.md`)
///    - Title match
///    - Alias match
/// 3. Return first match.
#[must_use]
pub fn resolve_wiki_link_target(target: &str, notes: &[NoteReference]) -> Option<String> {
    let normalized_target = normalize_keyword(&normalize_vault_path(target));
    let normalized_stem = normalized_target
        .strip_suffix(".md")
        .unwrap_or(&normalized_target)
        .to_string();
    let normalized_with_ext = if target.to_lowercase().ends_with(".md") {
        target.to_string()
    } else {
        format!("{target}.md")
    };

    for note in notes {
        let normalized_path = normalize_keyword(&normalize_vault_path(&note.vault_path));
        let normalized_path_stem = normalized_path
            .strip_suffix(".md")
            .unwrap_or(&normalized_path)
            .to_string();
        let normalized_title = note
            .title
            .as_ref()
            .map(|t| normalize_keyword(t))
            .unwrap_or_default();
        let normalized_aliases: std::collections::HashSet<String> =
            note.aliases.iter().map(|a| normalize_keyword(a)).collect();

        // Path match
        let matches_path = normalized_path == normalized_target
            || normalized_path == normalized_with_ext
            || normalized_path_stem == normalized_stem;

        // Title match
        let matches_title =
            normalized_title == normalized_target || normalized_title == normalized_stem;

        // Alias match
        let matches_alias = normalized_aliases.contains(&normalized_target)
            || normalized_aliases.contains(&normalized_stem);

        if matches_path || matches_title || matches_alias {
            return Some(note.vault_path.clone());
        }
    }

    None
}

/// Resolves all wikilinks from a source note against the note reference set.
///
/// Returns resolved links (those that match an indexed note).
#[must_use]
pub fn resolve_wiki_links(
    from_path: &str,
    links: &[WikiLink],
    notes: &[NoteReference],
) -> Vec<ResolvedLink> {
    let mut resolved = Vec::new();

    for link in links {
        let to_path = resolve_wiki_link_target(&link.target, notes);
        if let Some(to_path) = to_path {
            resolved.push(ResolvedLink {
                from_path: from_path.to_string(),
                to_path,
                alias: link.alias.clone(),
                heading: link.heading.clone(),
                raw_target: link.raw_target.clone(),
            });
        }
    }

    resolved
}

/// Builds a link graph edge list from resolved links.
#[must_use]
pub fn build_link_edges(from_path: &str, resolved: &[ResolvedLink]) -> Vec<LinkEdge> {
    resolved
        .iter()
        .map(|r| LinkEdge {
            from_path: from_path.to_string(),
            to_path: r.to_path.clone(),
            resolved: true,
            raw_target: r.raw_target.clone(),
            alias: r.alias.clone(),
            heading: r.heading.clone(),
        })
        .collect()
}

/// Computes backlinks: for each target, find all sources that link to it.
#[must_use]
pub fn compute_backlinks(edges: &[LinkEdge]) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut backlinks: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();

    for edge in edges {
        if edge.resolved {
            backlinks
                .entry(edge.to_path.clone())
                .or_default()
                .insert(edge.from_path.clone());
        }
    }

    backlinks
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect()
}

/// Finds unresolved links (links whose targets don't resolve to any indexed note).
#[must_use]
pub fn find_unresolved_links(
    from_path: &str,
    links: &[WikiLink],
    notes: &[NoteReference],
) -> Vec<ResolvedLink> {
    let mut unresolved = Vec::new();

    for link in links {
        if resolve_wiki_link_target(&link.target, notes).is_none() {
            unresolved.push(ResolvedLink {
                from_path: from_path.to_string(),
                to_path: String::new(),
                alias: link.alias.clone(),
                heading: link.heading.clone(),
                raw_target: link.raw_target.clone(),
            });
        }
    }

    unresolved
}

/// Computes link graph statistics.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn compute_link_stats(edges: &[LinkEdge], note_paths: &[String]) -> LinkGraphStats {
    let total_links = edges.len() as u32;
    let resolved_links = edges.iter().filter(|e| e.resolved).count() as u32;
    let unresolved_links = edges.iter().filter(|e| !e.resolved).count() as u32;

    let unique_targets: std::collections::BTreeSet<String> = edges
        .iter()
        .filter(|e| e.resolved)
        .map(|e| e.to_path.clone())
        .collect();

    // Nodes with no outgoing links
    let sources_with_outgoing: std::collections::BTreeSet<String> =
        edges.iter().map(|e| e.from_path.clone()).collect();
    let isolated_nodes = note_paths
        .iter()
        .filter(|p| !sources_with_outgoing.contains(p.as_str()))
        .count() as u32;

    LinkGraphStats {
        total_links,
        resolved_links,
        unresolved_links,
        unique_targets: unique_targets.len() as u32,
        isolated_nodes,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
