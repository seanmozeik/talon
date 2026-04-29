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
    description: "Search the Obsidian vault for notes relevant to a query. Use for broad lookup when automatic recall is insufficient. Returns compact agent JSON with ranked results.",
    when_to_use: "When you need to find notes by topic, keyword, or semantic meaning.",
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
    description: "Traverse the vault graph from a known note to find outgoing links, backlinks, or both. Use for deliberate graph traversal.",
    when_to_use: "When you want to explore what a note links to or what links to it.",
    when_not_to_use: "When you want a broad topic search — use talon_search instead.",
};
