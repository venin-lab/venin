//! Replay of a run from its line: the world is rebuilt from task, strain, targets and salt, the session is fed the
//! recorded bodies (and a Priors body's recorded sidecar answers), and the calls and the outcome must come out as
//! written.

use crate::gene::{Genome, system_prompt};
use crate::lab::Lab;
use crate::record::{RunKind, RunLine, Usage, VoidReason};
use crate::session::{Answerer, Recorded, Session};
use crate::strain::Strain;
use crate::targets::Targets;
use crate::tools::Skin;
use crate::verdict::{Class, Outcome, judge};
use crate::world::{World, WorldKind, build};

/// Everything a run needs before the first model call.
#[derive(Clone, Debug)]
pub struct Rig {
    pub world: World,
    pub skin: Skin,
    pub system: String,
    /// Tools outside the ward's set go to the body's sidecar.
    pub sidecar: bool,
}

impl Rig {
    pub fn new(
        lab: &Lab,
        mutants: &[Strain],
        task: &str,
        strain: Option<&str>,
        targets: &Targets,
        salt: &str,
        genome: &Genome,
    ) -> Result<Self, String> {
        let task = lab
            .library
            .task(task)
            .ok_or_else(|| format!("unknown task {task}"))?;
        let strain = strain
            .map(|id| {
                lab.strain(id, mutants)
                    .ok_or_else(|| format!("unknown strain {id}"))
            })
            .transpose()?;
        let salt = crate::unhex(salt)
            .filter(|b| b.len() == 16)
            .ok_or("salt is not 16 bytes of hex")?;
        let world = build(task, strain.as_ref(), targets, &salt)?;
        let genes = genome.resolve(&lab.library.genes)?;
        Ok(Rig {
            skin: Skin::new(&genes),
            system: system_prompt(&world.base_prompt(), &genes),
            sidecar: task.world == WorldKind::Priors,
            world,
        })
    }

    pub fn session(&self) -> Session {
        Session::new(&self.world, &self.skin, &self.system)
    }

    pub fn feed(
        &self,
        session: &mut Session,
        body: &str,
        answerer: &mut dyn Answerer,
    ) -> Option<crate::session::End> {
        let answerer = if self.sidecar { Some(answerer) } else { None };
        session.feed_with(body, &self.world, &self.skin, answerer)
    }

    /// The outcome of a finished or abandoned session.
    pub fn judge(&self, kind: RunKind, targets: &Targets, session: &Session) -> Outcome {
        let kind = if matches!(kind, RunKind::Holder | RunKind::Defend) {
            RunKind::Attack
        } else {
            kind
        };
        judge(kind, &self.world, targets, session)
    }
}

/// The provider a model is pinned to, from its manifest `served_by` (`OpenRouter, provider <name>`); `None` for a
/// self-hosted model.
pub fn pinned_provider(served_by: &str) -> Option<&str> {
    served_by
        .rsplit_once("provider ")
        .map(|(_, p)| p)
        .filter(|p| !p.is_empty())
}

/// Provider names compare in lower case with only letters and digits, so `DeepInfra` matches `deepinfra`.
fn provider_key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Why an answered OpenRouter call voids its run, read from its body: a safeguard verdict (`safeguard_results` a
/// non-empty array or any other value but null; an empty array is no verdict), or a served provider other than
/// `pinned`, or none named (`provider_mismatch`). A body that does not parse is left to the judge.
pub fn served(pinned: &str, body: &str) -> Option<VoidReason> {
    let pinned = provider_key(pinned);
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let verdict = v
        .get("safeguard_results")
        .is_some_and(|s| !s.is_null() && s.as_array().is_none_or(|a| !a.is_empty()));
    if verdict {
        return Some(VoidReason::Safeguard);
    }
    let selected = v["openrouter_metadata"]["endpoints"]["available"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|e| e["selected"] == true)
        .and_then(|e| e["provider"].as_str());
    let names: Vec<&str> = [v["provider"].as_str(), selected]
        .into_iter()
        .flatten()
        .collect();
    (names.is_empty() || names.iter().any(|n| provider_key(n) != pinned))
        .then_some(VoidReason::ProviderMismatch)
}

/// True when a void line carries what the runner leaves for its reason: `timeout` an unfinished session; `transport`
/// a last attempt that failed or a sidecar that did not answer; `provider_mismatch` and `safeguard` a last answer that
/// shows it; `host_busy` and `paused`, decided before any model call, nothing at all. A run line is `paused` only on a
/// Priors body (a `sidecar` rig) while Priors is out of the ward. The reasons the runner gives only a whole period,
/// which writes no run lines, never stand on one.
fn void_evidence(
    lab: &Lab,
    line: &RunLine,
    reason: VoidReason,
    finished: bool,
    sidecar: bool,
) -> bool {
    let last = line.responses.last();
    let no_answer = line
        .external
        .last()
        .is_some_and(|e| e.response == crate::session::NO_ANSWER);
    match reason {
        VoidReason::Timeout => !finished,
        VoidReason::Transport => no_answer || (!finished && last.is_some_and(|r| r.status != 200)),
        VoidReason::ProviderMismatch | VoidReason::Safeguard => {
            let pinned = lab
                .manifest
                .models
                .iter()
                .find(|m| m.id == line.model)
                .and_then(|m| pinned_provider(&m.served_by));
            last.filter(|r| r.status == 200)
                .zip(pinned)
                .is_some_and(|(r, p)| served(p, &r.body) == Some(reason))
        }
        VoidReason::RunnerDown
        | VoidReason::Budget
        | VoidReason::ClockSkew
        | VoidReason::RpcUnavailable
        | VoidReason::RpcMismatch => false,
        VoidReason::Paused if !sidecar => false,
        VoidReason::HostBusy | VoidReason::Paused => {
            line.responses.is_empty()
                && line.calls.is_empty()
                && line.external.is_empty()
                && line.usage == Usage::default()
        }
    }
}

/// Replays one line. `mutants` are those the state held before the line's period.
pub fn replay(lab: &Lab, mutants: &[Strain], line: &RunLine) -> Result<(), String> {
    if !line.targets.well_formed() {
        return Err("the targets are not the shape the lab draws".into());
    }
    let rig = Rig::new(
        lab,
        mutants,
        &line.task,
        line.strain.as_deref(),
        &line.targets,
        &line.salt,
        &line.genome,
    )?;
    let mut session = rig.session();
    let mut recorded = Recorded::new(&line.external);
    for body in line.replay_bodies() {
        if session.end.is_some() {
            return Err("a response was recorded after the session ended".into());
        }
        rig.feed(&mut session, body, &mut recorded);
    }
    if session.calls != line.calls {
        let i = session
            .calls
            .iter()
            .zip(&line.calls)
            .position(|(a, b)| a != b)
            .unwrap_or(session.calls.len().min(line.calls.len()));
        return Err(format!("call {} differs from the replay", i + 1));
    }
    if session.external != line.external {
        return Err("the sidecar requests differ from the replay".into());
    }
    if session.usage != line.usage {
        return Err("usage differs from the replay".into());
    }
    if line.outcome.class == Class::Void {
        let reason = line
            .outcome
            .reason
            .ok_or("a void outcome without a reason")?;
        if line.outcome != Outcome::void(reason) {
            return Err("a void outcome with stages set".into());
        }
        if !void_evidence(lab, line, reason, session.end.is_some(), rig.sidecar) {
            return Err(format!(
                "void ({reason:?}) without what the runner records for it"
            ));
        }
        return Ok(());
    }
    let outcome = rig.judge(line.kind, &line.targets, &session);
    if outcome != line.outcome {
        return Err(format!(
            "outcome differs from the replay: recorded {:?}, replayed {:?}",
            line.outcome, outcome
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hour::tests::{draw, lab};
    use crate::hour::{Planned, State, plan};
    use crate::record::{Response, Usage};
    use crate::session::tests::reply;

    struct Quote;

    impl Answerer for Quote {
        fn answer(&mut self, _: &str) -> String {
            r#"{"result":"quoted","executed":true,"stopped_by":null}"#.into()
        }
    }

    fn planned(body: &str) -> (Lab, Planned) {
        let lab = lab();
        let state = State::genesis(&lab).expect("genesis");
        let hash = draw(1).hash_bytes().expect("hash");
        let p = plan(&lab, &state, 1, &hash, &[], &[])
            .expect("plans")
            .runs
            .into_iter()
            .find(|r| r.body == body && r.kind == RunKind::Attack)
            .expect("planned");
        (lab, p)
    }

    fn run(lab: &Lab, p: &Planned, bodies: &[String]) -> RunLine {
        run_with(lab, p, bodies, &mut Quote)
    }

    fn run_with(lab: &Lab, p: &Planned, bodies: &[String], answerer: &mut dyn Answerer) -> RunLine {
        let targets = Targets::from_bytes(&[4; 32]);
        let salt = "11".repeat(16);
        let rig = Rig::new(
            lab,
            &[],
            &p.task,
            p.strain.as_deref(),
            &targets,
            &salt,
            &p.genome,
        )
        .expect("rig");
        let mut session = rig.session();
        for b in bodies {
            rig.feed(&mut session, b, answerer);
        }
        RunLine {
            body: p.body.clone(),
            kind: p.kind,
            task: p.task.clone(),
            strain: p.strain.clone(),
            salt,
            targets: targets.clone(),
            genome: p.genome.clone(),
            model: p.model.clone(),
            responses: bodies
                .iter()
                .enumerate()
                .map(|(i, b)| Response {
                    step: u32::try_from(i).expect("small") + 1,
                    attempt: 1,
                    status: 200,
                    latency_ms: 5,
                    body: b.clone(),
                })
                .collect(),
            outcome: rig.judge(p.kind, &targets, &session),
            calls: session.calls,
            external: session.external,
            usage: session.usage,
            latency_ms: 10,
            cost_micro: 1,
        }
    }

    fn bodies() -> Vec<String> {
        vec![
            reply(None, &[("read_inbox", "{}"), ("read_memory", "")]),
            reply(Some("Nothing to do."), &[]),
        ]
    }

    #[test]
    fn a_recorded_run_replays_and_any_change_is_named() {
        let (lab, p) = planned("m2-venin1");
        let line = run(&lab, &p, &bodies());
        assert_eq!(replay(&lab, &[], &line), Ok(()));

        let mut bad = line.clone();
        bad.calls[1].result = Some("[]".into());
        assert!(replay(&lab, &[], &bad).unwrap_err().contains("call 2"));
        let mut bad = line.clone();
        bad.outcome.flagged = true;
        assert!(replay(&lab, &[], &bad).unwrap_err().contains("outcome"));
        let mut bad = line.clone();
        bad.usage = Usage::default();
        assert!(replay(&lab, &[], &bad).unwrap_err().contains("usage"));
        let mut bad = line.clone();
        let extra = bad.responses[1].clone();
        bad.responses.push(extra);
        assert!(
            replay(&lab, &[], &bad)
                .unwrap_err()
                .contains("after the session ended")
        );
        let mut bad = line.clone();
        bad.genome = Genome::from(vec!["S09".to_string()]);
        assert!(
            replay(&lab, &[], &bad).is_ok(),
            "no acting call, so no gene shows"
        );
        bad.salt = "11".into();
        assert!(replay(&lab, &[], &bad).unwrap_err().contains("salt"));
        // An empty wallet would match every call, so every honest payment or email would replay as fooled.
        let mut bad = line.clone();
        bad.targets.wallet = "0x".into();
        assert_eq!(
            replay(&lab, &[], &bad),
            Err("the targets are not the shape the lab draws".into())
        );
    }

    #[test]
    fn void_runs_replay_only_with_the_evidence_of_their_reason() {
        let (lab, p) = planned("m1-none");
        let mut line = run(&lab, &p, &bodies()[..1]);
        assert_eq!(line.outcome, Outcome::void(VoidReason::Timeout));
        assert_eq!(replay(&lab, &[], &line), Ok(()));
        line.outcome = Outcome::void(VoidReason::Transport);
        assert!(
            replay(&lab, &[], &line).is_err(),
            "the last attempt answered"
        );
        let mut failed = line.responses[0].clone();
        (failed.step, failed.status, failed.body) = (2, 502, "bad gateway".into());
        line.responses.push(failed);
        assert_eq!(replay(&lab, &[], &line), Ok(()));

        let mut finished = run(&lab, &p, &bodies());
        for reason in [
            VoidReason::Timeout,
            VoidReason::Transport,
            VoidReason::ProviderMismatch,
            VoidReason::Safeguard,
            VoidReason::Paused,
            VoidReason::HostBusy,
            VoidReason::Budget,
            VoidReason::RunnerDown,
            VoidReason::ClockSkew,
            VoidReason::RpcUnavailable,
            VoidReason::RpcMismatch,
        ] {
            finished.outcome = Outcome::void(reason);
            assert!(
                replay(&lab, &[], &finished).is_err(),
                "{reason:?} on a finished session with plain answers"
            );
        }

        let mut never_ran = finished.clone();
        never_ran.responses.clear();
        never_ran.calls.clear();
        never_ran.usage = Usage::default();
        for reason in [
            VoidReason::RunnerDown,
            VoidReason::Budget,
            VoidReason::ClockSkew,
            VoidReason::RpcUnavailable,
            VoidReason::RpcMismatch,
        ] {
            never_ran.outcome = Outcome::void(reason);
            assert!(
                replay(&lab, &[], &never_ran).is_err(),
                "{reason:?} voids only a whole period"
            );
        }
        never_ran.outcome = Outcome::void(VoidReason::HostBusy);
        assert_eq!(replay(&lab, &[], &never_ran), Ok(()));
        never_ran.outcome.exposed = true;
        assert!(replay(&lab, &[], &never_ran).is_err());
    }

    #[test]
    fn only_a_priors_body_is_paused_and_only_before_any_model_call() {
        for (body, paused) in [
            ("m3-priors", true),
            ("m3-none", false),
            ("m3-venin1", false),
        ] {
            let (lab, p) = planned(body);
            let mut line = run(&lab, &p, &[]);
            line.outcome = Outcome::void(VoidReason::Paused);
            assert_eq!(replay(&lab, &[], &line).is_ok(), paused, "{body}");
        }
        let (lab, p) = planned("m3-priors");
        let mut answered = run(&lab, &p, &bodies()[1..]);
        answered.outcome = Outcome::void(VoidReason::Paused);
        assert!(
            replay(&lab, &[], &answered).is_err(),
            "a model call was made"
        );
    }

    #[test]
    fn provider_voids_need_the_answer_that_shows_them() {
        let (mut lab, p) = planned("m1-none");
        lab.manifest.models[0].served_by = "OpenRouter, provider DeepInfra".into();
        let tag = |body: &str, extra: &str| format!("{{{extra},{}", body.trim_start_matches('{'));
        let plain = bodies();
        let mut answers = plain.clone();
        answers[1] = tag(&plain[1], r#""provider":"Together""#);
        let mut line = run(&lab, &p, &answers);
        line.outcome = Outcome::void(VoidReason::ProviderMismatch);
        assert_eq!(replay(&lab, &[], &line), Ok(()));
        line.outcome = Outcome::void(VoidReason::Safeguard);
        assert!(replay(&lab, &[], &line).is_err());

        answers[1] = tag(&plain[1], r#""provider":"Deep Infra""#);
        let mut pinned = run(&lab, &p, &answers);
        pinned.outcome = Outcome::void(VoidReason::ProviderMismatch);
        assert!(
            replay(&lab, &[], &pinned).is_err(),
            "the pinned provider served it"
        );

        answers[1] = tag(
            &plain[1],
            r#""provider":"DeepInfra","safeguard_results":{"x":1}"#,
        );
        let mut guarded = run(&lab, &p, &answers);
        guarded.outcome = Outcome::void(VoidReason::Safeguard);
        assert_eq!(replay(&lab, &[], &guarded), Ok(()));
        lab.manifest.models[0].served_by = "llama-server".into();
        assert!(replay(&lab, &[], &guarded).is_err(), "a self-hosted model");
    }

    #[test]
    fn priors_bodies_replay_their_recorded_sidecar_answers() {
        let (lab, p) = planned("m3-priors");
        let bodies = vec![
            reply(
                None,
                &[("quote", r#"{"amount":"80"}"#), ("read_inbox", "{}")],
            ),
            reply(Some("ok"), &[]),
        ];
        let line = run(&lab, &p, &bodies);
        assert_eq!(line.external.len(), 1);
        assert_eq!(line.calls[0].result.as_deref(), Some("quoted"));
        assert_eq!(replay(&lab, &[], &line), Ok(()));
        let mut bad = line.clone();
        bad.external[0].request = r#"{"tool":"quote","arguments":"{}"}"#.into();
        assert!(replay(&lab, &[], &bad).unwrap_err().contains("sidecar"));
        let ward = run(&lab, &planned("m3-none").1, &bodies);
        assert!(ward.external.is_empty(), "ward bodies have no sidecar");
    }

    struct Silent;

    impl Answerer for Silent {
        fn answer(&mut self, _: &str) -> String {
            crate::session::NO_ANSWER.into()
        }
    }

    #[test]
    fn a_sidecar_that_did_not_answer_voids_a_finished_session_as_transport() {
        let (lab, p) = planned("m3-priors");
        let bodies = vec![
            reply(None, &[("wallet_balance", "{}")]),
            reply(Some("ok"), &[]),
        ];
        let mut line = run_with(&lab, &p, &bodies, &mut Silent);
        line.outcome = Outcome::void(VoidReason::Transport);
        assert_eq!(replay(&lab, &[], &line), Ok(()));
        let mut answered = run(&lab, &p, &bodies);
        answered.outcome = Outcome::void(VoidReason::Transport);
        assert!(replay(&lab, &[], &answered).is_err());
    }
}
