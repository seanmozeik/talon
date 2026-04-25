//! CLI argument parsing via `bpaf`.

use bpaf::{Parser, long, positional};
use std::path::PathBuf;
use talon_core::{Direction, SearchMode};

/// Parsed command-line arguments.
#[derive(Debug, Clone)]
pub struct CliArgs {
    /// Run MCP-over-stdio mode.
    pub mcp: McpFlag,
    /// Print embedded `SKILL.md`.
    pub skill: SkillFlag,
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
    /// Positional command and command arguments.
    pub positionals: Vec<String>,
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
        .help("Maximum lines for read.")
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
    let positionals = positional::<String>("ARGS")
        .help("Command and command arguments.")
        .many();

    bpaf::construct!(CliArgs {
        mcp,
        skill,
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
        positionals
    })
    .to_options()
    .descr("Talon Obsidian vault search, indexing, and MCP server.")
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
