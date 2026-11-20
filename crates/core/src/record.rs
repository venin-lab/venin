//! Record types shared by the runner, replay and verify. Field names and order are the contract in docs/RECORD.md.

use crate::gene::Genome;
use crate::targets::Targets;
use crate::verdict::Outcome;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    Attack,
    Benign,
    Decoy,
    Holder,
    /// A holder's own agent: their rules on a bare model, in the hour's attack frame.
    Defend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoidReason {
    Timeout,
    Transport,
    ProviderMismatch,
    Safeguard,
    RunnerDown,
    Paused,
    Budget,
    ClockSkew,
    RpcUnavailable,
    RpcMismatch,
    HostBusy,
}

/// One attempt at one model call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub step: u32,
    pub attempt: u32,
    pub status: u16,
    pub latency_ms: u64,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRecord {
    pub step: u32,
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub stopped_by: Option<String>,
    pub executed: bool,
    pub filtered_by: Vec<String>,
}

/// A sidecar exchange of a Priors body, kept verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct External {
    pub request: String,
    pub response: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunLine {
    pub body: String,
    pub kind: RunKind,
    pub task: String,
    pub strain: Option<String>,
    pub salt: String,
    pub targets: Targets,
    pub genome: Genome,
    pub model: String,
    pub responses: Vec<Response>,
    pub calls: Vec<CallRecord>,
    pub external: Vec<External>,
    pub usage: Usage,
    pub latency_ms: u64,
    pub cost_micro: u64,
    pub outcome: Outcome,
}

impl RunLine {
    /// The line as written, without its newline, and its hash.
    pub fn encode(&self) -> Result<(String, String), serde_json::Error> {
        let line = crate::json::to_string(self)?;
        let sha = crate::sha256_hex(line.as_bytes());
        Ok((line, sha))
    }

    /// The bodies replay feeds to the session: attempts with status 200, in order.
    pub fn replay_bodies(&self) -> impl Iterator<Item = &str> {
        self.responses
            .iter()
            .filter(|r| r.status == 200)
            .map(|r| r.body.as_str())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn line() -> RunLine {
        RunLine {
            body: "m1-venin1".into(),
            kind: RunKind::Attack,
            task: "A-001".into(),
            strain: Some("EAR-7.1".into()),
            salt: "00".repeat(16),
            targets: Targets::from_bytes(&[3; 32]),
            genome: Genome::from(vec!["S01".into(), "I01".into()]),
            model: "qwen".into(),
            responses: vec![
                Response {
                    step: 1,
                    attempt: 1,
                    status: 429,
                    latency_ms: 12,
                    body: "{\"error\":\"busy\"}".into(),
                },
                Response {
                    step: 1,
                    attempt: 2,
                    status: 200,
                    latency_ms: 900,
                    body: "{\"choices\":[]}".into(),
                },
            ],
            calls: vec![CallRecord {
                step: 1,
                id: "c1".into(),
                name: "read_inbox".into(),
                arguments: "{}".into(),
                result: Some("[]".into()),
                stopped_by: None,
                executed: true,
                filtered_by: vec![],
            }],
            external: vec![],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
            },
            latency_ms: 950,
            cost_micro: 0,
            outcome: Outcome::void(VoidReason::Timeout),
        }
    }

    #[test]
    fn run_line_keys_follow_the_record_contract() {
        let (text, sha) = line().encode().expect("writes");
        let keys = [
            "body",
            "kind",
            "task",
            "strain",
            "salt",
            "targets",
            "genome",
            "model",
            "responses",
            "calls",
            "external",
            "usage",
            "latency_ms",
            "cost_micro",
            "outcome",
        ];
        let mut at = 0;
        for key in keys {
            let found = text[at..]
                .find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("{key} in order"));
            at += found;
        }
        assert!(text.contains(r#""outcome":{"class":"void","reason":"timeout","exposed":false"#));
        assert!(
            text.contains(r#""targets":{"wallet":"0x"#)
                && text.contains(r#""genome":["I01","S01"]"#)
        );
        assert!(!text.ends_with('\n'));
        assert_eq!(sha, crate::sha256_hex(text.as_bytes()));
        let back: RunLine = serde_json::from_str(&text).expect("parses");
        assert_eq!(back, line());
        assert_eq!(back.encode().expect("writes").0, text);
    }

    #[test]
    fn replay_takes_successful_attempts_in_order() {
        assert_eq!(
            line().replay_bodies().collect::<Vec<_>>(),
            ["{\"choices\":[]}"]
        );
    }

    #[test]
    fn void_reasons_use_snake_case() {
        let text = crate::json::to_string(&[VoidReason::RpcMismatch, VoidReason::RunnerDown])
            .expect("writes");
        assert_eq!(text, r#"["rpc_mismatch","runner_down"]"#);
    }
}
