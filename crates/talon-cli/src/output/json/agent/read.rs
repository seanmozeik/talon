use serde::Serialize;
use talon_core::{ReadResponse, ReadResult, ReadSection};

#[derive(Debug, Serialize)]
pub(super) struct AgentReadResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    results: Vec<AgentReadResult<'a>>,
}

impl<'a> From<&'a ReadResponse> for AgentReadResponse<'a> {
    fn from(read: &'a ReadResponse) -> Self {
        Self {
            vault: read.vault.as_ref().map(talon_core::ContainerPath::as_str),
            results: read.results.iter().map(AgentReadResult::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AgentReadResult<'a> {
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    found: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<AgentReadSection<'a>>,
}

impl<'a> From<&'a ReadResult> for AgentReadResult<'a> {
    fn from(result: &'a ReadResult) -> Self {
        Self {
            path: result.vault_path.as_str(),
            found: (!result.found).then_some(false),
            title: result.title.as_deref(),
            content: result.content.as_deref(),
            section: result.section.as_ref().map(AgentReadSection::from),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentReadSection<'a> {
    heading: &'a str,
    from_line: u32,
    to_line: u32,
    obsidian_ref: &'a str,
}

impl<'a> From<&'a ReadSection> for AgentReadSection<'a> {
    fn from(section: &'a ReadSection) -> Self {
        Self {
            heading: &section.heading,
            from_line: section.from_line,
            to_line: section.to_line,
            obsidian_ref: &section.obsidian_ref,
        }
    }
}
