//! CLI argument parsing via `bpaf`.

use bpaf::{Parser, long, positional, short};
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
    /// Include per-result match anchors (BM25 + semantic). Opt-in.
    pub anchors: AnchorsFlag,
    /// Meta-command specific flags.
    pub meta: MetaArgs,
    /// Recall-command specific flags.
    pub recall: RecallArgs,
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

/// Recall-command specific options.
#[derive(Debug, Clone, Default)]
pub struct RecallArgs {
    /// Output format: json (default) or prompt-xml.
    pub format: Option<String>,
    /// Token budget for the recall context block.
    pub budget_tokens: Option<u32>,
    /// Minimum evidence score; below this, returns empty context.
    pub min_confidence: Option<f64>,
    /// Half-life in days for recency decay weighting.
    pub recency_half_life_days: Option<u8>,
    /// Prior turn messages to widen the query (repeatable).
    pub prior_messages: Vec<String>,
    /// Vault paths to exclude from recall candidates (repeatable).
    pub exclude: Vec<String>,
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
flag_type!(AnchorsFlag);

/// Parses CLI args or exits through `bpaf`.
#[must_use]
pub fn parse_or_exit() -> CliArgs {
    cli_parser().run()
}

fn switch_flag(name: &'static str, help: &'static str) -> impl bpaf::Parser<bool> {
    long(name).help(help).switch()
}

fn short_switch_flag(
    short_name: char,
    long_name: &'static str,
    help: &'static str,
) -> impl bpaf::Parser<bool> {
    short(short_name).long(long_name).help(help).switch()
}

fn cli_parser() -> bpaf::OptionParser<CliArgs> {
    let mcp = switch_flag("mcp", "Run MCP-over-stdio mode.").map(McpFlag::from);
    let skill = switch_flag("skill", "Print embedded SKILL.md.").map(SkillFlag::from);
    let version =
        short_switch_flag('V', "version", "Print the Talon CLI version.").map(VersionFlag::from);
    let agent = switch_flag(
        "agent",
        "Emit compact JSON for agents and disable human CLI art.",
    )
    .map(AgentFlag::from);
    let json = switch_flag("json", "Emit JSON output.").map(JsonFlag::from);
    let raw = switch_flag("raw", "Read raw note content.").map(RawFlag::from);
    let fast = switch_flag("fast", "Use fast mode for search or sync.").map(FastFlag::from);
    let force = switch_flag("force", "Force vector rebuild during sync.").map(ForceFlag::from);
    let config_file = short('c')
        .long("config")
        .help("Read config from PATH.")
        .argument::<PathBuf>("PATH")
        .optional();
    let mode = long("mode")
        .help("Search mode: hybrid, semantic, fulltext, or title.")
        .argument::<String>("MODE")
        .parse(|value| parse_search_mode(&value))
        .optional();
    let limit = short('l')
        .long("limit")
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
        .help("Filter results indexed since this timestamp (ISO 8601, epoch ms, or relative like 7d/3h).")
        .argument::<String>("SINCE")
        .optional();
    let anchors = anchors_parser();
    let meta = meta_parser();
    let recall = recall_parser();
    let positionals = positionals_parser();

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
        anchors,
        meta,
        recall,
        positionals
    })
    .to_options()
    .descr("Talon Obsidian vault search, indexing, and MCP server.")
    .header("Commands: init, sync, status, search, read, related, meta, changes, lint, recall.")
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

fn anchors_parser() -> impl bpaf::Parser<AnchorsFlag> {
    long("anchors")
        .help("Include per-result match anchors (BM25 + semantic) in the response.")
        .switch()
        .map(AnchorsFlag::from)
}

fn positionals_parser() -> impl bpaf::Parser<Vec<String>> {
    positional::<String>("ARGS")
        .help("Command and command arguments.")
        .many()
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

fn recall_parser() -> impl bpaf::Parser<RecallArgs> {
    let format = long("format")
        .help("Output format: json (default) or prompt-xml.")
        .argument::<String>("FORMAT")
        .optional();
    let budget_tokens = long("budget-tokens")
        .help("Token budget for the recall context block (default 2000).")
        .argument::<u32>("N")
        .optional();
    let min_confidence = long("min-confidence")
        .help("Minimum evidence score threshold 0.0-1.0 (default 0.0).")
        .argument::<f64>("F")
        .optional();
    let recency_half_life_days = long("recency-half-life-days")
        .help("Half-life in days for recency decay (default 7).")
        .argument::<u8>("N")
        .optional();
    let prior_messages = long("prior-message")
        .help("Prior turn message to widen the query (repeatable).")
        .argument::<String>("TEXT")
        .many();
    let exclude = long("exclude")
        .help("Vault path to exclude from recall (repeatable).")
        .argument::<String>("PATH")
        .many();
    bpaf::construct!(RecallArgs {
        format,
        budget_tokens,
        min_confidence,
        recency_half_life_days,
        prior_messages,
        exclude,
    })
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
