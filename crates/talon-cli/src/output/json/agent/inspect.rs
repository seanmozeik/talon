use serde::Serialize;
use talon_core::{InspectCheck, InspectFinding, InspectResponse};

#[derive(Debug, Serialize)]
pub(super) struct AgentInspect<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    total: usize,
    checks: AgentInspectChecks<'a>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentInspectChecks<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    orphans: Vec<AgentInspectFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    broken_links: Vec<AgentInspectFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dangling_refs: Vec<AgentInspectFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unreferenced: Vec<AgentInspectFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    graph: Vec<AgentInspectFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_links: Vec<AgentInspectFinding<'a>>,
}

#[derive(Debug, Serialize)]
struct AgentInspectFinding<'a> {
    path: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

impl<'a> From<&'a InspectResponse> for AgentInspect<'a> {
    fn from(resp: &'a InspectResponse) -> Self {
        let mut checks = AgentInspectChecks::default();
        for finding in &resp.findings {
            checks.push(finding);
        }
        Self {
            vault: resp.vault.as_ref().map(talon_core::ContainerPath::as_str),
            total: resp.findings.len(),
            checks,
        }
    }
}

impl<'a> AgentInspectChecks<'a> {
    fn push(&mut self, finding: &'a InspectFinding) {
        let compact = AgentInspectFinding {
            path: finding.path.as_str(),
            message: &finding.message,
            line: finding.line,
        };
        match finding.check {
            InspectCheck::All => {}
            InspectCheck::Orphans => self.orphans.push(compact),
            InspectCheck::BrokenLinks => self.broken_links.push(compact),
            InspectCheck::DanglingRefs => self.dangling_refs.push(compact),
            InspectCheck::Unreferenced => self.unreferenced.push(compact),
            InspectCheck::Graph => self.graph.push(compact),
            InspectCheck::MissingLinks => self.missing_links.push(compact),
        }
    }
}
