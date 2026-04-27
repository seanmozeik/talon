use serde::Serialize;
use talon_core::{LintCheck, LintFinding, LintResponse};

#[derive(Debug, Serialize)]
pub(super) struct AgentLint<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<&'a str>,
    total: usize,
    checks: AgentLintChecks<'a>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentLintChecks<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    orphans: Vec<AgentLintFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    broken_links: Vec<AgentLintFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dangling_refs: Vec<AgentLintFinding<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unreferenced: Vec<AgentLintFinding<'a>>,
}

#[derive(Debug, Serialize)]
struct AgentLintFinding<'a> {
    path: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

impl<'a> From<&'a LintResponse> for AgentLint<'a> {
    fn from(lint: &'a LintResponse) -> Self {
        let mut checks = AgentLintChecks::default();
        for finding in &lint.findings {
            checks.push(finding);
        }
        Self {
            vault: lint.vault.as_ref().map(talon_core::ContainerPath::as_str),
            total: lint.findings.len(),
            checks,
        }
    }
}

impl<'a> AgentLintChecks<'a> {
    fn push(&mut self, finding: &'a LintFinding) {
        let compact = AgentLintFinding {
            path: finding.path.as_str(),
            message: &finding.message,
            line: finding.line,
        };
        match finding.check {
            LintCheck::All => {}
            LintCheck::Orphans => self.orphans.push(compact),
            LintCheck::BrokenLinks => self.broken_links.push(compact),
            LintCheck::DanglingRefs => self.dangling_refs.push(compact),
            LintCheck::Unreferenced => self.unreferenced.push(compact),
        }
    }
}
