/// Compile-time contract for a named agent tool.
#[derive(Debug)]
pub struct AgentToolContract {
    pub name: &'static str,
    pub description: &'static str,
    pub when_to_use: &'static str,
    pub when_not_to_use: &'static str,
}

pub const SEARCH: AgentToolContract = AgentToolContract {
    name: "talon_search",
    description: "Search the Obsidian vault for notes relevant to a query, with hybrid retrieval and graph-aware refinement. Use for explicit lookup beyond automatic recall. Default search excludes scopes configured with `default = false` such as `raw/`, `archive/`, or `private/`; pass `scopeAll: true` or an explicit `scope` when looking for recall-injected paths from those scopes. Returns compact agent JSON with ranked plain-path results; synthesize answers yourself or call the CLI `talon ask` only when you specifically want Talon's smaller built-in answer model.",
    when_to_use: "When you need to find notes by topic, keyword, or semantic meaning that auto-recall did not cover, or when you need source snippets before synthesizing an answer.",
    when_not_to_use: "When you already have the exact path — use talon_read instead.",
};

pub const READ: AgentToolContract = AgentToolContract {
    name: "talon_read",
    description: "Read a vault note by path or Obsidian reference. Use after search when you need source text, exact wording, or a section body.",
    when_to_use: "When you have a specific vault path or [[Obsidian Ref]] and need its content.",
    when_not_to_use: "When you are looking for notes by topic — use talon_search instead.",
};

pub const RELATED: AgentToolContract = AgentToolContract {
    name: "talon_related",
    description: "Find ranked related notes from links, backlinks, shared sources, common neighbors, and graph communities. Use for deliberate graph/provenance exploration from a known note.",
    when_to_use: "When you want to explore graph-ranked context around a specific note.",
    when_not_to_use: "When you want a broad topic search — use talon_search instead.",
};

pub const RECALL_HOOK: AgentToolContract = AgentToolContract {
    name: "talon_hook_recall",
    description: "Hook-only tool that injects vault recall context before each agent turn. Managed automatically by the session lifecycle — do not call manually.",
    when_to_use: "Never — this is managed by Claude Code hooks automatically.",
    when_not_to_use: "Always. Do not call this tool directly.",
};
