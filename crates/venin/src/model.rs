//! The model client and the runs of a period: OpenAI-compatible chat completions with tools, one retry honouring
//! Retry-After, every attempt kept; runs interleaved across model columns, each column at its own concurrency, and
//! nothing started or waited for past the deadline. A model served through OpenRouter is pinned to one provider with
//! fallbacks off and Armor off; a response from another provider, or one carrying a safeguard result, voids its run.

use crate::config::{Config, ModelConfig};
use crate::net::Net;
use crate::priors::Priors;
use serde_json::json;
use std::time::{Duration, Instant};
use venin_core::hour::Planned;
use venin_core::lab::Lab;
use venin_core::record::{Response, RunLine, Usage, VoidReason};
use venin_core::replay::Rig;
use venin_core::session::{Answerer, Session};
use venin_core::strain::Strain;
use venin_core::targets::Targets;
use venin_core::verdict::Outcome;

const RETRY_WAIT_MAX_S: u64 = 30;
const RETRY_WAIT_DEFAULT_S: u64 = 2;

/// The request body: the session's messages and tools (with Priors' for a Priors body), the model id and its sampling.
pub fn request_body(session: &Session, model: &ModelConfig, priors: Option<&Priors>) -> Vec<u8> {
    let mut body = session.request();
    if let Some(priors) = priors {
        priors.augment(&mut body);
    }
    let s = &model.sampling;
    body["model"] = model.id.clone().into();
    body["temperature"] = (f64::from(s.temperature_milli) / 1000.0).into();
    body["top_p"] = (f64::from(s.top_p_milli) / 1000.0).into();
    body["top_k"] = s.top_k.into();
    body["max_tokens"] = s.max_tokens.into();
    if let Some(or) = &model.openrouter {
        body["provider"] = json!({"order": [or.provider], "allow_fallbacks": false});
        body["plugins"] = json!([{"id": "armor", "enabled": false}]);
    }
    body.to_string().into_bytes()
}

/// The headers of a model request: the key and the routing metadata switch for OpenRouter, none otherwise.
fn headers(config: &Config, model: &ModelConfig) -> Vec<(&'static str, String)> {
    if model.openrouter.is_none() {
        return Vec::new();
    }
    let key = config.openrouter_key.as_deref().unwrap_or_default();
    vec![
        ("authorization", format!("Bearer {key}")),
        ("x-openrouter-metadata", "enabled".into()),
    ]
}

/// Why an answered call voids its run, by the core's rule; only a model served through OpenRouter is checked.
pub fn served(model: &ModelConfig, body: &str) -> Option<VoidReason> {
    venin_core::replay::served(&model.openrouter.as_ref()?.provider, body)
}

fn retryable(status: u16) -> bool {
    status == 0 || status == 429 || status >= 500
}

pub struct Period<'a> {
    pub lab: &'a Lab,
    pub mutants: &'a [Strain],
    pub config: &'a Config,
    pub net: &'a Net,
    pub priors: &'a Priors,
    /// Unix seconds after which nothing is started and nothing is waited for.
    pub deadline: i64,
}

impl Period<'_> {
    fn left(&self) -> Option<Duration> {
        u64::try_from(self.deadline - crate::now())
            .ok()
            .filter(|&s| s > 0)
            .map(Duration::from_secs)
    }

    /// One model call: up to two attempts. Returns the 200 body, or the void reason that ends the run.
    fn call(
        &self,
        model: &ModelConfig,
        step: u32,
        body: &[u8],
        responses: &mut Vec<Response>,
    ) -> Result<String, VoidReason> {
        let url = format!("{}/chat/completions", model.endpoint.trim_end_matches('/'));
        for attempt in 1..=2 {
            let left = self.left().ok_or(VoidReason::Timeout)?;
            let timeout = left.min(Duration::from_secs(self.config.call_timeout_s.into()));
            let t = Instant::now();
            let sent =
                self.net
                    .post_with(&url, body.to_vec(), &headers(self.config, model), timeout);
            let (status, text, retry_after) = match sent {
                Ok(r) => (r.status, r.body, r.retry_after_s),
                Err(e) => (0, e, None),
            };
            responses.push(Response {
                step,
                attempt,
                status,
                latency_ms: u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX),
                body: text.clone(),
            });
            if status == 200 {
                return Ok(text);
            }
            if self.left().is_none() {
                return Err(VoidReason::Timeout);
            }
            if attempt == 1 && retryable(status) {
                let wait = Duration::from_secs(
                    retry_after
                        .unwrap_or(RETRY_WAIT_DEFAULT_S)
                        .min(RETRY_WAIT_MAX_S),
                );
                if self.left().is_none_or(|l| l <= wait) {
                    return Err(VoidReason::Timeout);
                }
                std::thread::sleep(wait);
                continue;
            }
            break;
        }
        Err(VoidReason::Transport)
    }

    /// Runs one planned run to its line.
    pub fn run(&self, planned: &Planned) -> Result<RunLine, String> {
        let model = self
            .config
            .model(&planned.model)
            .ok_or_else(|| format!("no configured model {}", planned.model))?;
        let salt = venin_core::hex(&crate::random::<16>());
        let targets = Targets::from_bytes(&crate::random::<32>());
        let mut rig = Rig::new(
            self.lab,
            self.mutants,
            &planned.task,
            planned.strain.as_deref(),
            &targets,
            &salt,
            &planned.genome,
        )?;
        if let Some(rules) = &planned.rules {
            rig.system = venin_core::gene::defend_prompt(&rig.world.base_prompt(), rules);
        }
        if rig.sidecar && self.priors.is_out() {
            return Ok(Self::void_line(planned, VoidReason::Paused));
        }
        if self.left().is_none() {
            return Ok(Self::void_line(planned, VoidReason::Timeout));
        }
        while !crate::runner::host_ok(self.net, &self.config.host_check) {
            if crate::now() + crate::runner::HOST_POLL_S >= self.deadline {
                return Ok(Self::void_line(planned, VoidReason::HostBusy));
            }
            std::thread::sleep(Duration::from_secs(
                crate::runner::HOST_POLL_S.unsigned_abs(),
            ));
        }
        let start = self.deadline - i64::from(self.lab.manifest.deadline_s);
        let mut answer = if rig.sidecar {
            Some(self.priors.answerer(&rig.world, &salt, start)?)
        } else {
            None
        };
        let started = Instant::now();
        let mut session = rig.session();
        let mut responses = Vec::new();
        let mut void = None;
        while session.end.is_none() {
            let step = session.steps + 1;
            let body = request_body(&session, model, answer.is_some().then_some(self.priors));
            match self.call(model, step, &body, &mut responses) {
                Ok(body) => {
                    let answerer = answer.as_mut().map(|a| a as &mut dyn Answerer);
                    session.feed_with(&body, &rig.world, &rig.skin, answerer);
                    if let Some(reason) = served(model, &body) {
                        void = Some(reason);
                        break;
                    }
                    if answer.as_ref().is_some_and(|a| a.timed_out) {
                        void = Some(VoidReason::Transport);
                        break;
                    }
                }
                Err(reason) => {
                    void = Some(reason);
                    break;
                }
            }
        }
        if let Some(answer) = answer {
            answer.finish();
        }
        let outcome = match void {
            Some(reason) => Outcome::void(reason),
            None => rig.judge(planned.kind, &targets, &session),
        };
        let price = &model.price_micro_per_mtok;
        let cost_micro = (session.usage.prompt_tokens * price.prompt
            + session.usage.completion_tokens * price.completion)
            / 1_000_000;
        Ok(RunLine {
            body: planned.body.clone(),
            kind: planned.kind,
            task: planned.task.clone(),
            strain: planned.strain.clone(),
            salt,
            targets,
            genome: planned.genome.clone(),
            model: planned.model.clone(),
            responses,
            calls: session.calls,
            external: session.external,
            usage: session.usage,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            cost_micro,
            outcome,
        })
    }

    /// A run that never started: the deadline came first (`timeout`), or its row is paused.
    fn void_line(planned: &Planned, reason: VoidReason) -> RunLine {
        RunLine {
            body: planned.body.clone(),
            kind: planned.kind,
            task: planned.task.clone(),
            strain: planned.strain.clone(),
            salt: venin_core::hex(&crate::random::<16>()),
            targets: Targets::from_bytes(&crate::random::<32>()),
            genome: planned.genome.clone(),
            model: planned.model.clone(),
            responses: Vec::new(),
            calls: Vec::new(),
            external: Vec::new(),
            usage: Usage::default(),
            latency_ms: 0,
            cost_micro: 0,
            outcome: Outcome::void(reason),
        }
    }

    /// Every planned run, in plan order. Columns take turns, starting with column `n mod 4`; each turn runs up to the
    /// column's concurrency at once. `log` hears each finished line. A run the deadline reaches first is `timeout`,
    /// except a Priors run while Priors is out of the ward, which is `paused` all the same.
    pub fn run_all(
        &self,
        n: u64,
        runs: &[Planned],
        log: &(dyn Fn(&RunLine) + Sync),
    ) -> Result<Vec<RunLine>, String> {
        let order = interleave(n, runs, |p| {
            self.config
                .model(&p.model)
                .map_or(1, |m| m.concurrency as usize)
        });
        let mut lines: Vec<Option<RunLine>> = vec![None; runs.len()];
        for batch in order {
            let done: Vec<(usize, Result<RunLine, String>)> = std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .iter()
                    .map(|&i| {
                        let planned = &runs[i];
                        scope.spawn(move || self.run(planned))
                    })
                    .collect();
                batch
                    .iter()
                    .zip(handles)
                    .map(|(&i, h)| (i, h.join().unwrap_or_else(|_| Err("a run panicked".into()))))
                    .collect()
            });
            for (i, line) in done {
                let line = line?;
                log(&line);
                lines[i] = Some(line);
            }
        }
        Ok(lines.into_iter().flatten().collect())
    }
}

/// Batches of run indices: columns in turn from column `n mod 4`, each batch up to that column's concurrency.
pub fn interleave(
    n: u64,
    runs: &[Planned],
    concurrency: impl Fn(&Planned) -> usize,
) -> Vec<Vec<usize>> {
    let mut queues: Vec<Vec<usize>> = venin_core::lab::COLUMNS
        .iter()
        .map(|c| {
            let prefix = format!("{c}-");
            runs.iter()
                .enumerate()
                .filter(|(_, p)| p.body.starts_with(&prefix))
                .map(|(i, _)| i)
                .rev()
                .collect()
        })
        .collect();
    let first = usize::try_from(n % 4).expect("small");
    queues.rotate_left(first);
    let mut out = Vec::new();
    while queues.iter().any(|q| !q.is_empty()) {
        for q in &mut queues {
            let Some(&next) = q.last() else { continue };
            let take = concurrency(&runs[next]).max(1).min(q.len());
            out.push(q.split_off(q.len() - take).into_iter().rev().collect());
        }
    }
    out
}

/// Seconds per run of each model, from finished lines: (model, runs, mean ms, max ms).
pub fn timing(lines: &[RunLine]) -> Vec<(String, usize, u64, u64)> {
    let mut by: std::collections::BTreeMap<&str, Vec<u64>> = Default::default();
    for l in lines {
        by.entry(&l.model).or_default().push(l.latency_ms);
    }
    by.into_iter()
        .map(|(m, ms)| {
            let mean = ms.iter().sum::<u64>() / ms.len() as u64;
            (
                m.to_string(),
                ms.len(),
                mean,
                ms.iter().copied().max().unwrap_or(0),
            )
        })
        .collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::tests::text;
    use crate::net::tests::serve;
    use serde_json::Value;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use venin_core::hour::{State, plan};
    use venin_core::record::RunKind;
    use venin_core::verdict::Class;

    fn planned(body: &str) -> Planned {
        Planned {
            body: body.into(),
            kind: RunKind::Benign,
            task: "t".into(),
            strain: None,
            genome: Default::default(),
            model: "m".into(),
            rules: None,
        }
    }

    #[test]
    fn interleaving_rotates_columns_and_respects_concurrency() {
        let runs: Vec<Planned> = ["m1-a", "m1-b", "m1-c", "m2-a", "m3-a", "m4-a", "m4-b"]
            .into_iter()
            .map(planned)
            .collect();
        let two = |_: &Planned| 2;
        assert_eq!(
            interleave(0, &runs, two),
            vec![vec![0, 1], vec![3], vec![4], vec![5, 6], vec![2]]
        );
        assert_eq!(
            interleave(3, &runs, |_| 1),
            vec![
                vec![5],
                vec![0],
                vec![3],
                vec![4],
                vec![6],
                vec![1],
                vec![2]
            ]
        );
        let mut all: Vec<usize> = interleave(1, &runs, two).concat();
        all.sort_unstable();
        assert_eq!(all, (0..runs.len()).collect::<Vec<_>>());
    }

    /// A scripted model server: the first call of every run reads the inbox, the second answers in text. The first
    /// `fail` requests get a 429 with Retry-After 0.
    pub fn model_server(fail: u32) -> (String, crate::net::tests::Seen) {
        let count = Arc::new(AtomicU32::new(0));
        serve(move |_, _, body| {
            let i = count.fetch_add(1, Ordering::SeqCst);
            if i < fail {
                return (
                    429,
                    vec![("retry-after".into(), "0".into())],
                    r#"{"error":{"message":"busy"}}"#.into(),
                );
            }
            let v: Value = serde_json::from_str(body).expect("json");
            let tool_turns = v["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .filter(|m| m["role"] == "tool")
                .count();
            let message = if tool_turns == 0 {
                json!({"role": "assistant", "content": null, "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "read_inbox", "arguments": "{}"}}]})
            } else {
                json!({"role": "assistant", "content": "Done."})
            };
            let reply = json!({"choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
                               "usage": {"prompt_tokens": 1000, "completion_tokens": 10}});
            (200, Vec::new(), reply.to_string())
        })
    }

    pub fn test_lab() -> (Lab, State) {
        let files = crate::store::read_library(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../library"
        )))
        .expect("library");
        crate::genesis::tests::lab_with_priors(&files)
    }

    #[test]
    fn runs_retry_once_and_record_every_attempt() {
        let (url, seen) = model_server(1);
        let dir = crate::anchor::tests::temp("model");
        let config =
            Config::parse(&text(&dir, "http://127.0.0.1:1", &format!("{url}/v1"))).expect("config");
        let net = Net::new(&config.urls()).expect("client");
        let (lab, state) = test_lab();
        let hash = [7u8; 32];
        let p = plan(&lab, &state, 1, &hash, &[], &[]).expect("plans");
        let priors = crate::priors::tests::priors();
        let period = Period {
            lab: &lab,
            mutants: &state.mutants,
            config: &config,
            net: &net,
            priors: &priors,
            deadline: crate::now() + 60,
        };
        let line = period.run(&p.runs[1]).expect("runs");
        assert_eq!(line.responses[0].status, 429);
        assert_eq!(
            (line.responses[1].status, line.responses[1].attempt),
            (200, 2)
        );
        assert_eq!(line.usage.prompt_tokens, 2000);
        assert_eq!(line.cost_micro, 2000 + 40);
        assert_ne!(line.outcome.class, Class::Void);
        venin_core::replay::replay(&lab, &state.mutants, &line).expect("replays");
        let (_, path, body, _) = seen.lock().expect("lock")[1].clone();
        assert_eq!(path, "/v1/chat/completions");
        let sent: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(sent["model"], p.runs[1].model.as_str());
        assert_eq!(sent["temperature"], 0.7);
        assert_eq!(sent["top_k"], 40);
        assert!(sent.get("provider").is_none() && sent.get("plugins").is_none());
    }

    #[test]
    fn a_dead_endpoint_voids_a_run_and_the_deadline_stops_new_ones() {
        let (url, _) = model_server(u32::MAX);
        let dir = crate::anchor::tests::temp("dead");
        let config =
            Config::parse(&text(&dir, "http://127.0.0.1:1", &format!("{url}/v1"))).expect("config");
        let net = Net::new(&config.urls()).expect("client");
        let (lab, state) = test_lab();
        let p = plan(&lab, &state, 1, &[9u8; 32], &[], &[]).expect("plans");
        let priors = crate::priors::tests::priors();
        let mut period = Period {
            lab: &lab,
            mutants: &state.mutants,
            config: &config,
            net: &net,
            priors: &priors,
            deadline: crate::now() + 60,
        };
        let line = period.run(&p.runs[0]).expect("runs");
        assert_eq!(line.outcome.reason, Some(VoidReason::Transport));
        assert_eq!(line.responses.len(), 2);
        venin_core::replay::replay(&lab, &state.mutants, &line).expect("replays");
        period.deadline = crate::now() - 1;
        let lines = period.run_all(1, &p.runs[..3], &|_| {}).expect("runs");
        assert!(
            lines
                .iter()
                .all(|l| l.outcome.reason == Some(VoidReason::Timeout))
        );
        assert_eq!(lines[2].kind, p.runs[2].kind);
    }

    /// A Priors body sees Priors' tools, its Priors calls go to the sidecar and are kept for replay, and a Priors
    /// version out of the ward leaves its runs paused.
    #[test]
    fn priors_bodies_call_the_sidecar_and_pause_when_priors_is_out() {
        let (url, seen) = serve(|_, _, body| {
            let v: Value = serde_json::from_str(body).expect("json");
            let first = !v["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .any(|m| m["role"] == "tool");
            let message = if first {
                json!({"role": "assistant", "content": null, "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "wallet_balance", "arguments": "{}"}}]})
            } else {
                json!({"role": "assistant", "content": "Done."})
            };
            let reply =
                json!({"choices": [{"index": 0, "message": message, "finish_reason": "stop"}]});
            (200, Vec::new(), reply.to_string())
        });
        let dir = crate::anchor::tests::temp("priors-run");
        let config =
            Config::parse(&text(&dir, "http://127.0.0.1:1", &format!("{url}/v1"))).expect("config");
        let net = Net::new(&config.urls()).expect("client");
        let (lab, state) = test_lab();
        let p = plan(&lab, &state, 1, &[7u8; 32], &[], &[]).expect("plans");
        let planned = p
            .runs
            .iter()
            .find(|r| r.body == "m1-priors" && r.kind == RunKind::Benign)
            .expect("a Priors run");
        let begins = Arc::new(AtomicU32::new(0));
        let priors = crate::priors::tests::mocked(begins.clone());
        let period = Period {
            lab: &lab,
            mutants: &state.mutants,
            config: &config,
            net: &net,
            priors: &priors,
            deadline: crate::now() + 60,
        };
        let line = period.run(planned).expect("runs");
        assert_eq!(line.external.len(), 1);
        assert!(line.external[0].request.contains("wallet_balance"));
        assert!(line.calls[0].executed);
        assert_eq!(begins.load(Ordering::SeqCst), 1);
        venin_core::replay::replay(&lab, &state.mutants, &line).expect("replays");
        let sent: Value = serde_json::from_str(&seen.lock().expect("lock")[0].2).expect("json");
        let tools: Vec<&str> = sent["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(tools.contains(&"pay_url") && tools.contains(&"send_funds"));

        priors.set_out(true);
        let paused = period.run(planned).expect("runs");
        assert_eq!(paused.outcome.reason, Some(VoidReason::Paused));
        assert!(paused.responses.is_empty());
        venin_core::replay::replay(&lab, &state.mutants, &paused).expect("replays");
        let late = Period {
            deadline: crate::now() - 1,
            ..period
        };
        let row: Vec<Planned> = p
            .runs
            .iter()
            .filter(|r| r.body.starts_with("m1-"))
            .cloned()
            .collect();
        let lines = late.run_all(1, &row, &|_| {}).expect("runs");
        assert_eq!(lines.len(), 15);
        for l in &lines {
            let want = if l.body.ends_with("-priors") {
                VoidReason::Paused
            } else {
                VoidReason::Timeout
            };
            assert_eq!(l.outcome.reason, Some(want), "{} {:?}", l.body, l.kind);
        }

        let silent = crate::priors::Priors::with_spawn(
            &crate::priors::tests::library(),
            Box::new(|| {
                let (read, write) = std::io::pipe().map_err(|e| e.to_string())?;
                std::mem::forget(write);
                let mut sidecar = crate::priors::Sidecar::new(
                    Box::new(std::io::sink()),
                    Box::new(std::io::BufReader::new(read)),
                );
                sidecar.timeout = std::time::Duration::from_millis(50);
                Ok(sidecar)
            }),
        )
        .expect("loads");
        let period = Period {
            priors: &silent,
            ..period
        };
        let calls = seen.lock().expect("lock").len();
        let line = period.run(planned).expect("runs");
        assert_eq!(line.outcome.reason, Some(VoidReason::Transport));
        assert_eq!(
            seen.lock().expect("lock").len(),
            calls + 1,
            "no model call after it"
        );
        assert_eq!(line.external[0].response, venin_core::session::NO_ANSWER);
        venin_core::replay::replay(&lab, &state.mutants, &line).expect("replays");
    }

    /// The test config with every model served through OpenRouter by DeepInfra, and a key.
    fn hosted(dir: &std::path::Path, url: &str) -> Config {
        let text = text(dir, "http://127.0.0.1:1", &format!("{url}/v1"))
            .replace(
                "gguf = \"/nonexistent\"",
                "openrouter = { provider = \"DeepInfra\" }",
            )
            .replace(
                "served_by = \"test\"",
                "served_by = \"OpenRouter, provider DeepInfra\"",
            );
        let mut config = Config::parse(&text).expect("config");
        config.openrouter_key = Some("test-key".into());
        config
    }

    /// A mock of OpenRouter's chat completions: the first call reads the inbox, the next answers in text; each reply
    /// names the served provider at the top and in the routing metadata, and carries a safeguard result when asked.
    fn openrouter_server(
        provider: &'static str,
        safeguard: bool,
    ) -> (String, crate::net::tests::Seen) {
        serve(move |_, _, body| {
            let v: Value = serde_json::from_str(body).expect("json");
            let first = !v["messages"].to_string().contains("\"role\":\"tool\"");
            let message = if first {
                json!({"role": "assistant", "content": null, "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "read_inbox", "arguments": "{}"}}]})
            } else {
                json!({"role": "assistant", "content": "Done."})
            };
            let mut reply = json!({
                "id": "gen-1",
                "provider": provider,
                "model": v["model"],
                "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1000, "completion_tokens": 10},
                "openrouter_metadata": {"endpoints": {"available": [
                    {"provider": "Other", "model": v["model"], "selected": false},
                    {"provider": provider, "model": v["model"], "selected": true}]}}
            });
            if safeguard {
                reply["safeguard_results"] = json!([{"type": "dangerous_tool_use", "status": {"type": "available",
                    "tool_uses": {"c1": {"type": "evaluated", "outcome": "not_flagged"}}}}]);
            }
            (200, Vec::new(), reply.to_string())
        })
    }

    #[test]
    fn openrouter_runs_are_pinned_and_void_on_another_provider_or_a_safeguard() {
        let (mut lab, state) = test_lab();
        for m in &mut lab.manifest.models {
            m.served_by = "OpenRouter, provider DeepInfra".into();
        }
        let p = plan(&lab, &state, 1, &[7u8; 32], &[], &[]).expect("plans");
        let planned = p
            .runs
            .iter()
            .find(|r| r.body == "m1-none" && r.kind == RunKind::Benign)
            .expect("a run");
        let priors = crate::priors::tests::priors();
        for (provider, safeguard, want) in [
            ("DeepInfra", false, None),
            ("Together", false, Some(VoidReason::ProviderMismatch)),
            ("DeepInfra", true, Some(VoidReason::Safeguard)),
        ] {
            let (url, seen) = openrouter_server(provider, safeguard);
            let config = hosted(&crate::anchor::tests::temp("openrouter"), &url);
            let net = Net::new(&config.urls()).expect("client");
            let period = Period {
                lab: &lab,
                mutants: &state.mutants,
                config: &config,
                net: &net,
                priors: &priors,
                deadline: crate::now() + 60,
            };
            let line = period.run(planned).expect("runs");
            assert_eq!(line.outcome.reason, want, "{provider} {safeguard}");
            venin_core::replay::replay(&lab, &state.mutants, &line).expect("replays");
            let seen = seen.lock().expect("lock").clone();
            assert_eq!(seen.len(), if want.is_some() { 1 } else { 2 });
            let (_, _, body, headers) = &seen[0];
            assert!(headers.contains("authorization: bearer test-key\r\n"));
            assert!(headers.contains("x-openrouter-metadata: enabled\r\n"));
            let sent: Value = serde_json::from_str(body).expect("json");
            assert_eq!(
                sent["provider"],
                json!({"order": ["DeepInfra"], "allow_fallbacks": false})
            );
            assert_eq!(sent["plugins"], json!([{"id": "armor", "enabled": false}]));
        }
    }

    #[test]
    fn served_providers_are_checked_only_for_openrouter_models() {
        let config = hosted(std::path::Path::new("/tmp/x"), "http://127.0.0.1:2");
        let model = &config.models[0];
        let mismatch = Some(VoidReason::ProviderMismatch);
        for (body, want) in [
            (r#"{"provider":"deepinfra"}"#, None),
            (
                r#"{"provider":"Deep Infra","safeguard_results":null}"#,
                None,
            ),
            (r#"{"provider":"Together"}"#, mismatch),
            (r#"{"choices":[]}"#, mismatch),
            (
                r#"{"openrouter_metadata":{"endpoints":{"available":[{"provider":"DeepInfra","selected":true}]}}}"#,
                None,
            ),
            (
                r#"{"provider":"DeepInfra","openrouter_metadata":{"endpoints":{"available":[{"provider":"Novita","selected":true}]}}}"#,
                mismatch,
            ),
            (r#"{"provider":"DeepInfra","safeguard_results":[]}"#, None),
            (
                r#"{"provider":"DeepInfra","safeguard_results":[{"type":"dangerous_tool_use"}]}"#,
                Some(VoidReason::Safeguard),
            ),
            (
                r#"{"provider":"DeepInfra","safeguard_results":{"flagged":true}}"#,
                Some(VoidReason::Safeguard),
            ),
            (
                r#"{"provider":"Together","safeguard_results":[]}"#,
                mismatch,
            ),
            ("not json", None),
        ] {
            assert_eq!(served(model, body), want, "{body}");
        }
        let local = Config::parse(&text(
            std::path::Path::new("/tmp/x"),
            "http://127.0.0.1:1",
            "http://127.0.0.1:2/v1",
        ))
        .expect("config");
        assert_eq!(served(&local.models[0], r#"{"provider":"Together"}"#), None);
        assert!(headers(&local, &local.models[0]).is_empty());
    }

    #[test]
    fn timing_sums_runs_per_model() {
        let mut a = Period::void_line(&planned("m1-a"), VoidReason::Timeout);
        a.latency_ms = 1000;
        let mut b = a.clone();
        b.latency_ms = 3000;
        assert_eq!(timing(&[a, b]), vec![("m".into(), 2, 2000, 3000)]);
    }
}
