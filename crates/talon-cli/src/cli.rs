//! CLI argument parsing via `bpaf`.

use bpaf::{Parser, long, positional};
use std::path::PathBuf;
use talon_core::{Direction, SearchMode, WhereClause, WhereOperator};

/// Parsed command-line arguments.
#[derive(Debug, Clone)]
pub struct CliArgs {
    /// Run MCP-over-stdio mode.
    pub mcp: McpFlag,
    /// Print embedded `SKILL.md`.
    pub skill: SkillFlag,
    /// Print the Talon CLI version.
    pub version: VersionFlag,
    /// Token-efficient JSON for agents. Disables human banner and spinner.
    pub agent: AgentFlag,
    /// Emit JSON output.
    pub json: JsonFlag,
    /// Raw read output.
    pub raw: RawFlag,
    /// Fast mode. For search this means lexical-only; for sync this skips embeddings.
    pub fast: FastFlag,
    /// Force vector rebuild during sync.
    pub force: ForceFlag,
    /// Optional config file path.
    pub config_file: Option<PathBuf>,
    /// Search mode.
    pub mode: Option<SearchMode>,
    /// Result limit.
    pub limit: Option<u16>,
    /// First line for read.
    pub from_line: Option<u16>,
    /// Maximum lines for read.
    pub max_lines: Option<u16>,
    /// Related traversal depth.
    pub depth: Option<u8>,
    /// Related traversal direction.
    pub direction: Option<Direction>,
    /// Frontmatter `--where` filters for search.
    pub where_clauses: Vec<String>,
    /// Filter results indexed since this timestamp.
    pub since: Option<String>,
    /// Meta-command specific flags.
    pub meta: MetaArgs,
    /// Positional command and command arguments.
    pub positionals: Vec<String>,
}

/// Meta-command specific options.
#[derive(Debug, Clone, Default)]
pub struct MetaArgs {
    /// Frontmatter fields to project (repeatable).
    pub select: Vec<String>,
    /// Emit tag counts.
    pub tag_counts: bool,
    /// Resolve reverse-source references for this path.
    pub sources: Option<String>,
}

macro_rules! flag_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            /// Flag is absent.
            Disabled,
            /// Flag is present.
            Enabled,
        }

        impl $name {
            /// Returns true when the flag was present.
            #[must_use]
            pub const fn enabled(self) -> bool {
                matches!(self, Self::Enabled)
            }
        }

        impl From<bool> for $name {
            fn from(value: bool) -> Self {
                if value { Self::Enabled } else { Self::Disabled }
            }
        }
    };
}

flag_type!(McpFlag);
flag_type!(SkillFlag);
flag_type!(VersionFlag);
flag_type!(AgentFlag);
flag_type!(JsonFlag);
flag_type!(RawFlag);
flag_type!(FastFlag);
flag_type!(ForceFlag);

/// Parses CLI args or exits through `bpaf`.
#[must_use]
pub fn parse_or_exit() -> CliArgs {
    cli_parser().run()
}

fn cli_parser() -> bpaf::OptionParser<CliArgs> {
    let mcp = long("mcp")
        .help("Run MCP-over-stdio mode.")
        .switch()
        .map(McpFlag::from);
    let skill = long("skill")
        .help("Print embedded SKILL.md.")
        .switch()
        .map(SkillFlag::from);
    let version = long("version")
        .help("Print the Talon CLI version.")
        .switch()
        .map(VersionFlag::from);
    let agent = long("agent")
        .help("Emit compact JSON for agents and disable human CLI art.")
        .switch()
        .map(AgentFlag::from);
    let json = long("json")
        .help("Emit JSON output.")
        .switch()
        .map(JsonFlag::from);
    let raw = long("raw")
        .help("Read raw note content.")
        .switch()
        .map(RawFlag::from);
    let fast = long("fast")
        .help("Use fast mode for search or sync.")
        .switch()
        .map(FastFlag::from);
    let force = long("force")
        .help("Force vector rebuild during sync.")
        .switch()
        .map(ForceFlag::from);
    let config_file = long("config")
        .help("Read config from PATH.")
        .argument::<PathBuf>("PATH")
        .optional();
    let mode = long("mode")
        .help("Search mode: hybrid, semantic, fulltext, or title.")
        .argument::<String>("MODE")
        .parse(|value| parse_search_mode(&value))
        .optional();
    let limit = long("limit")
        .help("Search result limit.")
        .argument::<u16>("N")
        .optional();
    let from_line = long("from-line")
        .help("First line for read.")
        .argument::<u16>("N")
        .optional();
    let max_lines = long("max-lines")
        .help("Max lines for read.")
        .argument::<u16>("N")
        .optional();
    let depth = long("depth")
        .help("Related traversal depth.")
        .argument::<u8>("N")
        .optional();
    let direction = long("direction")
        .help("Related direction: outgoing, backlinks, or both.")
        .argument::<String>("DIRECTION")
        .parse(|value| parse_direction(&value))
        .optional();
    let where_clauses = long("where")
        .help("Frontmatter filter: KEY OP VALUE (repeatable). Ops: =, !=, <, <=, >, >=, contains, exists.")
        .argument::<String>("WHERE")
        .many();
    let since = long("since")
        .help("Filter results indexed since this timestamp (ISO 8601 or epoch ms).")
        .argument::<String>("SINCE")
        .optional();
    let meta = meta_parser();
    let positionals = positional::<String>("ARGS")
        .help("Command and command arguments.")
        .many();

    bpaf::construct!(CliArgs {
        mcp,
        skill,
        version,
        agent,
        json,
        raw,
        fast,
        force,
        config_file,
        mode,
        limit,
        from_line,
        max_lines,
        depth,
        direction,
        where_clauses,
        since,
        meta,
        positionals
    })
    .to_options()
    .descr("Talon Obsidian vault search, indexing, and MCP server.")
}

fn meta_parser() -> impl bpaf::Parser<MetaArgs> {
    let select = long("select")
        .help("Frontmatter field to project (repeatable).")
        .argument::<String>("FIELD")
        .many();
    let tag_counts = long("tag-counts").help("Emit tag counts.").switch();
    let sources = long("sources")
        .help("Resolve notes referencing this path via their sources: field.")
        .argument::<String>("PATH")
        .optional();
    bpaf::construct!(MetaArgs {
        select,
        tag_counts,
        sources
    })
}

fn parse_search_mode(value: &str) -> Result<SearchMode, String> {
    match value {
        "hybrid" => Ok(SearchMode::Hybrid),
        "semantic" => Ok(SearchMode::Semantic),
        "fulltext" => Ok(SearchMode::Fulltext),
        "title" => Ok(SearchMode::Title),
        _ => Err("mode must be hybrid, semantic, fulltext, or title".to_string()),
    }
}

fn parse_direction(value: &str) -> Result<Direction, String> {
    match value {
        "outgoing" => Ok(Direction::Outgoing),
        "backlinks" => Ok(Direction::Backlinks),
        "both" => Ok(Direction::Both),
        _ => Err("direction must be outgoing, backlinks, or both".to_string()),
    }
}

/// Parses a `--where` string into a [`WhereClause`].
///
/// Format: `KEY OP VALUE` (three space-separated tokens).
/// `exists` takes only one token: `KEY exists`.
///
/// # Errors
///
/// Returns an error string if the format is invalid or the operator is unknown.
pub fn parse_where_clause(value: &str) -> Result<WhereClause, String> {
    let parts: Vec<&str> = value.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err(format!(
            "invalid where clause '{value}'; expected 'KEY OP VALUE' or 'KEY exists'"
        ));
    }
    let key = parts[0].to_string();
    let op = match parts[1] {
        "=" => WhereOperator::Equals,
        "!=" => WhereOperator::NotEquals,
        "<" => WhereOperator::LessThan,
        "<=" => WhereOperator::LessThanOrEqual,
        ">" => WhereOperator::GreaterThan,
        ">=" => WhereOperator::GreaterThanOrEqual,
        "contains" => WhereOperator::Contains,
        "exists" => WhereOperator::Exists,
        other => {
            return Err(format!(
                "unknown operator '{other}'; try =, !=, <, <=, >, >=, contains, exists"
            ));
        }
    };
    let value = if op == WhereOperator::Exists {
        None
    } else {
        Some(parts.get(2).unwrap_or(&"").to_string())
    };
    Ok(WhereClause { key, op, value })
}
