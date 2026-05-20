use std::collections::HashMap;

use crate::text::frontmatter::{normalize_keyword, normalize_vault_path};

use super::{NoteReference, basename, has_markdown_extension};

#[derive(Debug, Default)]
pub struct LinkResolver {
    exact: HashMap<String, String>,
    basenames: HashMap<String, Option<String>>,
    suffixes: HashMap<String, Option<String>>,
    titles_or_aliases: HashMap<String, String>,
}

impl LinkResolver {
    #[must_use]
    pub fn new(notes: &[NoteReference]) -> Self {
        let mut resolver = Self::default();
        for note in notes {
            resolver.insert(note);
        }
        resolver
    }

    fn insert(&mut self, note: &NoteReference) {
        let normalized_path = normalize_keyword(&normalize_vault_path(&note.vault_path));
        let normalized_path_stem = normalized_path
            .strip_suffix(".md")
            .unwrap_or(&normalized_path)
            .to_string();
        self.exact.insert(normalized_path, note.vault_path.clone());
        self.exact
            .insert(normalized_path_stem.clone(), note.vault_path.clone());
        insert_unique(
            &mut self.basenames,
            basename(&normalized_path_stem).to_string(),
            &note.vault_path,
        );
        for suffix in component_suffixes(&normalized_path_stem) {
            insert_unique(&mut self.suffixes, suffix, &note.vault_path);
        }
        if let Some(title) = &note.title {
            self.titles_or_aliases
                .entry(normalize_keyword(title))
                .or_insert_with(|| note.vault_path.clone());
        }
        for alias in &note.aliases {
            self.titles_or_aliases
                .entry(normalize_keyword(alias))
                .or_insert_with(|| note.vault_path.clone());
        }
    }

    #[must_use]
    pub fn resolve(&self, target: &str) -> Option<String> {
        let normalized_target = normalize_keyword(&normalize_vault_path(target));
        let normalized_stem = normalized_target
            .strip_suffix(".md")
            .unwrap_or(&normalized_target);
        let normalized_with_ext = if has_markdown_extension(&normalized_target) {
            normalized_target.clone()
        } else {
            format!("{normalized_target}.md")
        };

        if let Some(path) = self
            .exact
            .get(&normalized_target)
            .or_else(|| self.exact.get(&normalized_with_ext))
            .or_else(|| self.exact.get(normalized_stem))
        {
            return Some(path.clone());
        }
        if normalized_stem.contains('/') {
            if let Some(Some(path)) = self.suffixes.get(normalized_stem) {
                return Some(path.clone());
            }
        } else if let Some(Some(path)) = self.basenames.get(normalized_stem) {
            return Some(path.clone());
        }
        self.titles_or_aliases
            .get(&normalized_target)
            .or_else(|| self.titles_or_aliases.get(normalized_stem))
            .cloned()
    }
}

fn insert_unique(map: &mut HashMap<String, Option<String>>, key: String, path: &str) {
    map.entry(key)
        .and_modify(|existing| {
            if existing.as_deref() != Some(path) {
                *existing = None;
            }
        })
        .or_insert_with(|| Some(path.to_string()));
}

fn component_suffixes(path_stem: &str) -> Vec<String> {
    let parts: Vec<&str> = path_stem.split('/').collect();
    (1..parts.len()).map(|idx| parts[idx..].join("/")).collect()
}
