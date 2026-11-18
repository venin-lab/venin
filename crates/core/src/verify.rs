//! The verifier: from the published files, the library and what the chain says, it checks that the record chains,
//! that every file hashes to its anchor, that every run replays, and that every hour follows from the one before.

use crate::hour::{Flag, HourRecord, Inputs, OutcomeRef, State, Withheld, advance, plan};
use crate::lab::{
    COLUMNS, Exam, Lab, Library, Manifest, RULES_VERSION, Row, exam_families, roster,
};
use crate::record::{RunKind, RunLine, VoidReason};
use crate::replay::replay;
use crate::verdict::{Class, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The share of void runs above which an exam hour leaves the headline.
pub const HEADLINE_VOID_PPM: u64 = 100_000;

fn on_priors_row(o: &OutcomeRef) -> bool {
    o.body.ends_with("-priors")
}

fn paused(o: &OutcomeRef) -> bool {
    o.outcome.reason == Some(VoidReason::Paused)
}

/// The void share the headline limit is held to: void runs over all runs. `paused` runs are left out only when every
/// Priors run is void and no other run is paused (Priors out of the ward, which the lab decided on rather than
/// suffered; a Priors run the deadline reached first may be `timeout` instead); otherwise each counts as void.
pub fn headline_void_ppm(record: &HourRecord) -> u64 {
    let priors_out = record.outcomes.iter().all(|o| {
        if on_priors_row(o) {
            o.outcome.class == Class::Void
        } else {
            !paused(o)
        }
    });
    let voids = record
        .outcomes
        .iter()
        .filter(|o| o.outcome.class == Class::Void && !(priors_out && paused(o)))
        .count() as u64;
    (voids * 1_000_000)
        .checked_div(record.outcomes.len() as u64)
        .unwrap_or(0)
}

/// Whether Priors may be out of the ward in the hour after `record`, given whether it may be in `record`. The runner
/// takes Priors out only when it seals an hour, in a file it keeps private, so verify follows the record instead: a
/// sealed hour opens the window, and the first hour with a Priors run that is not void closes it.
fn priors_out_after(out: bool, record: &HourRecord) -> bool {
    record.flags.contains(&Flag::Sealed)
        || (out
            && record
                .outcomes
                .iter()
                .filter(|o| on_priors_row(o))
                .all(|o| o.outcome.class == Class::Void))
}

/// True for an hour with a `paused` Priors run that is not a paused period.
fn pauses_priors(record: &HourRecord) -> bool {
    record.void_reason().is_none()
        && record
            .outcomes
            .iter()
            .any(|o| on_priors_row(o) && paused(o))
}

/// True for an exam hour whose published attack runs count in the headline: on time, not sealed, and at most 10%
/// void by `headline_void_ppm`.
pub fn in_headline(record: &HourRecord, late: bool) -> bool {
    record.exam
        && !late
        && !record.flags.contains(&Flag::Sealed)
        && headline_void_ppm(record) <= HEADLINE_VOID_PPM
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    Manifest,
    Exam,
    Hour,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorEntry {
    pub kind: AnchorKind,
    pub n: u64,
    pub tx: String,
    pub block: u64,
}

/// The 46 bytes of an anchor transaction's calldata, as `0x` hex.
pub fn calldata(kind: AnchorKind, n: u64, sha: &str) -> String {
    let mut bytes = b"venin".to_vec();
    bytes.push(kind as u8);
    bytes.extend_from_slice(&n.to_be_bytes());
    bytes.extend(crate::unhex(sha).unwrap_or_default());
    format!("0x{}", crate::hex(&bytes))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub n: u64,
    pub sha: String,
    pub written_s: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelHealth {
    pub column: String,
    pub valid: u32,
    pub void: u32,
    pub malformed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub n: u64,
    pub anchor_lag_s: i64,
    pub runway_days: u32,
    pub void_share_ppm: u64,
    pub models: Vec<ModelHealth>,
    pub updated_s: i64,
}

/// Valid, void and malformed runs per column over the last 24 records.
pub fn model_health(history: &[HourRecord]) -> Vec<ModelHealth> {
    let recent = &history[history.len().saturating_sub(24)..];
    COLUMNS
        .iter()
        .map(|column| {
            let mut health = ModelHealth {
                column: column.to_string(),
                valid: 0,
                void: 0,
                malformed: 0,
            };
            let prefix = format!("{column}-");
            for o in recent
                .iter()
                .flat_map(|r| &r.outcomes)
                .filter(|o| o.body.starts_with(&prefix))
            {
                match o.outcome.class {
                    Class::Void => health.void += 1,
                    Class::Malformed => {
                        health.malformed += 1;
                        health.valid += 1;
                    }
                    _ => health.valid += 1,
                }
            }
            health
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Takedown {
    pub path: String,
    pub sha: String,
    pub date_s: i64,
    pub reason: String,
}

/// The library files as published, byte for byte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryFiles {
    pub strains: String,
    pub tasks: String,
    pub genes: String,
    pub rows: String,
    pub names: String,
}

impl LibraryFiles {
    pub fn parse(&self) -> Result<Library, String> {
        Library::parse(
            &self.strains,
            &self.tasks,
            &self.genes,
            &self.rows,
            &self.names,
        )
    }
}

/// The published files. `runs` holds each period's runs file after gunzip; `cards` the card files present.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bundle {
    pub manifest: Vec<u8>,
    pub exam: Vec<u8>,
    pub hours: BTreeMap<u64, Vec<u8>>,
    pub runs: BTreeMap<u64, Vec<u8>>,
    pub cards: BTreeMap<u64, Vec<u8>>,
    pub anchors: Vec<u8>,
    pub head: Vec<u8>,
    pub takedowns: Vec<Takedown>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tx {
    pub from: String,
    pub to: String,
    pub input: String,
    pub block: u64,
    pub ts: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub hash: String,
    pub ts: i64,
    pub parent_ts: i64,
}

/// What the chain says, as far as it was reachable. Missing entries are counted as unchecked, not as errors; a
/// transaction the chain answered for is in `txs`, `pending` (known, not mined) or `absent` (unknown to it).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chain {
    pub nonce: Option<u64>,
    pub txs: BTreeMap<String, Tx>,
    pub blocks: BTreeMap<u64, Block>,
    #[serde(default)]
    pub pending: BTreeSet<String>,
    #[serde(default)]
    pub absent: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowRate {
    pub row: String,
    pub attacks: u64,
    pub fooled: u64,
    pub breached: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// The rules version of a file made under other rules; verification stopped there.
    pub made_by: Option<u32>,
    pub hours: u64,
    pub late: Vec<u64>,
    pub void_share_ppm: u64,
    pub withheld: u64,
    pub taken_down: Vec<String>,
    /// Hours whose runs were not available, so their state was taken as written instead of recomputed.
    pub not_recomputed: Vec<u64>,
    /// Chain facts that could not be read.
    pub unchecked: u64,
    /// Published, replayed attack runs per row over the hours `in_headline` keeps.
    pub headline: Vec<RowRate>,
    /// The exam hours `headline` counts.
    pub headline_hours: Vec<u64>,
    pub errors: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.made_by.is_none() && self.errors.is_empty()
    }
}

fn rules_version(bytes: &[u8]) -> Option<u64> {
    serde_json::from_slice::<Value>(bytes)
        .ok()?
        .get("rules_version")?
        .as_u64()
}

fn same_address(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The first top-level field in which two JSON documents differ.
fn first_difference(a: &[u8], b: &[u8]) -> String {
    let (Ok(Value::Object(x)), Ok(Value::Object(y))) = (
        serde_json::from_slice::<Value>(a),
        serde_json::from_slice::<Value>(b),
    ) else {
        return "the file".into();
    };
    x.iter()
        .find(|(k, v)| y.get(*k) != Some(v))
        .map_or_else(|| "the byte layout".into(), |(k, _)| k.clone())
}

struct Verifier<'a> {
    bundle: &'a Bundle,
    chain: &'a Chain,
    report: Report,
}

impl Verifier<'_> {
    fn err(&mut self, message: String) {
        self.report.errors.push(message);
    }

    /// True when the file was made under other rules; verification stops there.
    fn other_version(&mut self, path: &str, bytes: &[u8]) -> bool {
        match rules_version(bytes) {
            Some(v) if v != u64::from(RULES_VERSION) => {
                self.report.made_by = u32::try_from(v).ok();
                self.err(format!(
                    "{path}: made by venin v{v}, verify with that version"
                ));
                true
            }
            _ => false,
        }
    }

    /// Checks the draw block against the chain and, when `start` is known, that it is the first block at `start`.
    fn check_draw(&mut self, path: &str, draw: &crate::hour::Draw, start: Option<i64>) {
        match start {
            Some(start) if draw.ts < start || draw.prev_ts >= start => self.err(format!(
                "{path}: the draw block is not the first block at its draw time"
            )),
            Some(_) => {}
            None => self.report.unchecked += 1,
        }
        match self.chain.blocks.get(&draw.block) {
            None => self.report.unchecked += 1,
            Some(b)
                if b.hash.trim_start_matches("0x") == draw.hash.trim_start_matches("0x")
                    && b.ts == draw.ts
                    && b.parent_ts == draw.prev_ts => {}
            Some(_) => self.err(format!(
                "{path}: draw block {} differs from the chain",
                draw.block
            )),
        }
    }

    /// Checks the anchor list; returns each anchor's block time where the chain had it. Only the head hour's anchor
    /// may still be pending.
    fn check_anchors(
        &mut self,
        manifest: &Manifest,
        shas: &BTreeMap<(AnchorKind, u64), String>,
        head: u64,
    ) -> BTreeMap<(AnchorKind, u64), Option<i64>> {
        let anchors: Vec<AnchorEntry> = match serde_json::from_slice(&self.bundle.anchors) {
            Ok(a) => a,
            Err(e) => {
                self.err(format!("anchors.json: {e}"));
                return BTreeMap::new();
            }
        };
        let mut last: BTreeMap<AnchorKind, u64> = BTreeMap::new();
        let mut times = BTreeMap::new();
        for (i, a) in anchors.iter().enumerate() {
            let file = match a.kind {
                AnchorKind::Manifest => "manifest.json".to_string(),
                AnchorKind::Exam => "exam.json".to_string(),
                AnchorKind::Hour => format!("hours/{}.json", a.n),
            };
            let at = format!("anchors.json entry {} ({file})", i + 1);
            if let Some(prev) = last.insert(a.kind, a.n)
                && a.n <= prev
            {
                self.err(format!("{at}: n does not rise within kind {:?}", a.kind));
            }
            let Some(sha) = shas.get(&(a.kind, a.n)) else {
                self.err(format!("{at}: anchors a file that is not published"));
                continue;
            };
            let ts = match self.chain.txs.get(&a.tx) {
                None if self.chain.absent.contains(&a.tx) => {
                    self.err(format!("{at}: anchor tx not on chain"));
                    None
                }
                None if self.chain.pending.contains(&a.tx)
                    && (a.kind, a.n) != (AnchorKind::Hour, head) =>
                {
                    self.err(format!("{at}: anchor tx not mined"));
                    None
                }
                None => {
                    self.report.unchecked += 1;
                    None
                }
                Some(tx) => {
                    let address = &manifest.anchor.address;
                    if !same_address(&tx.from, address) || !same_address(&tx.to, address) {
                        self.err(format!("{at}: not sent from and to the anchor address"));
                    }
                    if !tx.input.eq_ignore_ascii_case(&calldata(a.kind, a.n, sha)) {
                        self.err(format!("{at}: calldata does not commit to {file}"));
                    }
                    if tx.block != a.block {
                        self.err(format!("{at}: block differs from the chain"));
                    }
                    Some(tx.ts)
                }
            };
            times.insert((a.kind, a.n), ts);
        }
        for kind in [AnchorKind::Manifest, AnchorKind::Exam] {
            if !last.contains_key(&kind) {
                self.err(format!("anchors.json: no {kind:?} anchor"));
            }
        }
        match self.chain.nonce {
            Some(nonce) if nonce != anchors.len() as u64 => self.err(format!(
                "anchors.json: {} entries, but the anchor address has sent {nonce} transactions",
                anchors.len()
            )),
            Some(_) => {}
            None => self.report.unchecked += 1,
        }
        times
    }
}

/// Verifies the published record. Every failure is named in `errors` with the file it was found in.
pub fn verify(bundle: &Bundle, chain: &Chain, files: &LibraryFiles) -> Report {
    let mut v = Verifier {
        bundle,
        chain,
        report: Report::default(),
    };
    if v.other_version("manifest.json", &bundle.manifest) {
        return v.report;
    }
    let manifest: Manifest = match serde_json::from_slice(&bundle.manifest) {
        Ok(m) => m,
        Err(e) => {
            v.err(format!("manifest.json: {e}"));
            return v.report;
        }
    };
    let library = match files.parse() {
        Ok(l) => l,
        Err(e) => {
            v.err(format!("library: {e}"));
            return v.report;
        }
    };
    for (field, sha, path) in [
        ("library_sha", &manifest.library_sha, &files.strains),
        ("tasks_sha", &manifest.tasks_sha, &files.tasks),
        ("genes_sha", &manifest.genes_sha, &files.genes),
        ("rows_sha", &manifest.rows_sha, &files.rows),
        ("names_sha", &manifest.names_sha, &files.names),
    ] {
        if *sha != crate::sha256_hex(path.as_bytes()) {
            v.err(format!(
                "manifest.json: {field} does not match the library file"
            ));
        }
    }
    if manifest.bodies != roster() {
        v.err("manifest.json: bodies are not the 20 of the roster".into());
    }
    let columns: Vec<&str> = manifest.models.iter().map(|m| m.column.as_str()).collect();
    if columns != COLUMNS {
        v.err("manifest.json: models are not one per column m1..m4".into());
    }
    let exam: Exam = match serde_json::from_slice(&bundle.exam) {
        Ok(e) => e,
        Err(e) => {
            v.err(format!("exam.json: {e}"));
            return v.report;
        }
    };
    match exam.draw.hash_bytes() {
        Ok(hash) if exam.families == exam_families(&library.strains, &hash) => {}
        _ => v.err("exam.json: families are not the ones drawn from its block".into()),
    }
    let head: Head = match serde_json::from_slice(&bundle.head) {
        Ok(h) => h,
        Err(e) => {
            v.err(format!("head.json: {e}"));
            return v.report;
        }
    };
    let mut shas = BTreeMap::from([
        (
            (AnchorKind::Manifest, 0),
            crate::sha256_hex(&bundle.manifest),
        ),
        ((AnchorKind::Exam, 0), crate::sha256_hex(&bundle.exam)),
    ]);
    for (n, bytes) in &bundle.hours {
        shas.insert((AnchorKind::Hour, *n), crate::sha256_hex(bytes));
    }
    match shas.get(&(AnchorKind::Hour, head.n)) {
        Some(sha) if *sha != head.sha => v.err(format!(
            "head.json: sha is not the hash of hours/{}.json",
            head.n
        )),
        _ => {}
    }
    let anchored = v.check_anchors(&manifest, &shas, head.n);
    let anchor_ts = |kind| anchored.get(&(kind, 0)).copied().flatten();
    let draw_at = anchor_ts(AnchorKind::Manifest).map(|ts| ts + i64::from(manifest.exam_delay_s));
    v.check_draw("exam.json", &exam.draw, draw_at);
    if anchor_ts(AnchorKind::Exam).is_some_and(|ts| ts >= manifest.genesis) {
        v.err("exam.json: anchored after the genesis time".into());
    }
    let lab = Lab {
        manifest,
        library,
        exam,
    };
    let mut state = match State::genesis(&lab) {
        Ok(s) => s,
        Err(e) => {
            v.err(format!("genesis: {e}"));
            return v.report;
        }
    };
    let mut prev_sha = crate::sha256_hex(&bundle.exam);
    let (mut voids, mut outcomes) = (0u64, 0u64);
    let mut headline: BTreeMap<Row, RowRate> = BTreeMap::new();
    let mut history: Vec<HourRecord> = Vec::new();
    let mut priors_out = false;
    for n in 0..=head.n {
        let path = format!("hours/{n}.json");
        let Some(bytes) = bundle.hours.get(&n) else {
            v.err(format!("{path}: missing"));
            break;
        };
        if v.other_version(&path, bytes) {
            return v.report;
        }
        let sha = crate::sha256_hex(bytes);
        let record: HourRecord = match serde_json::from_slice(bytes) {
            Ok(r) => r,
            Err(e) => {
                v.err(format!("{path}: {e}"));
                prev_sha = sha;
                continue;
            }
        };
        v.report.hours += 1;
        let start = crate::time::period_start(n, lab.manifest.genesis, lab.manifest.period_s);
        if record.n != n || Some(record.start) != start {
            v.err(format!("{path}: n or start do not follow the clock"));
        }
        if record.prev != prev_sha {
            let before = n
                .checked_sub(1)
                .map_or("exam.json".into(), |p| format!("hours/{p}.json"));
            v.err(format!("{path}: prev does not chain to {before}"));
        }
        let deadline =
            record.start + i64::from(lab.manifest.deadline_s) + i64::from(lab.manifest.late_s);
        let late = match anchored.get(&(AnchorKind::Hour, n)) {
            Some(Some(ts)) => *ts > deadline,
            Some(None) => false,
            None if n == head.n => {
                v.report.unchecked += 1;
                false
            }
            None => {
                v.err(format!("{path}: no anchor"));
                false
            }
        };
        if late {
            v.report.late.push(n);
        }
        if let Some(draw) = &record.draw {
            v.check_draw(&path, draw, Some(record.start));
        }
        if pauses_priors(&record) && !priors_out {
            v.err(format!(
                "{path}: Priors runs are paused, but no sealed hour took Priors out of the ward"
            ));
        }
        priors_out = priors_out_after(priors_out, &record);
        v.report.withheld += record.withheld.len() as u64;
        voids += record
            .outcomes
            .iter()
            .filter(|o| o.outcome.class == Class::Void)
            .count() as u64;
        outcomes += record.outcomes.len() as u64;
        let runs_path = format!("runs/{n}.jsonl.gz");
        let taken_down = bundle.takedowns.iter().any(|t| t.path == runs_path);
        match v.period_lines(n, &record, &lab, &state, taken_down) {
            Some(lines) => {
                v.recompute(&path, bytes, &record, &lab, &state, &lines);
                if in_headline(&record, late) {
                    v.report.headline_hours.push(n);
                    let attacks = lines
                        .iter()
                        .filter(|l| l.kind == RunKind::Attack && l.outcome.class != Class::Void);
                    for l in attacks {
                        let Some(body) = crate::lab::Body::parse(&l.body) else {
                            continue;
                        };
                        let rate = headline.entry(body.row).or_insert_with(|| RowRate {
                            row: body.row.as_str().into(),
                            ..RowRate::default()
                        });
                        rate.attacks += 1;
                        rate.fooled += u64::from(l.outcome.fooled);
                        rate.breached += u64::from(l.outcome.breached);
                    }
                }
            }
            None if taken_down || !bundle.runs.contains_key(&n) => {
                v.report.not_recomputed.push(n);
                if taken_down {
                    v.report.withheld += record.runs.len() as u64;
                }
                v.check_unrecomputed(&path, &record, &lab, &state);
            }
            None => {}
        }
        state = record.state.clone();
        history.push(record);
        v.check_card(&history, &lab.library);
        prev_sha = sha;
    }
    for t in &bundle.takedowns {
        if !v.report.taken_down.contains(&t.path) {
            v.report.taken_down.push(t.path.clone());
        }
    }
    v.report.void_share_ppm = (voids * 1_000_000).checked_div(outcomes).unwrap_or(0);
    v.report.headline = headline.into_values().collect();
    v.report
}

impl Verifier<'_> {
    /// The period's run lines, each hashed and replayed; `None` when they cannot be recomputed from. A runs file
    /// `takedowns.json` lists is taken as taken down even while it is still served, as the site counts it.
    fn period_lines(
        &mut self,
        n: u64,
        record: &HourRecord,
        lab: &Lab,
        state: &State,
        taken_down: bool,
    ) -> Option<Vec<RunLine>> {
        let path = format!("runs/{n}.jsonl.gz");
        if record.runs.is_empty() {
            return Some(Vec::new());
        }
        if taken_down {
            return None;
        }
        let Some(raw) = self.bundle.runs.get(&n) else {
            self.err(format!("{path}: missing"));
            return None;
        };
        let Ok(text) = std::str::from_utf8(raw) else {
            self.err(format!("{path}: not text"));
            return None;
        };
        let texts: Vec<&str> = text.split_terminator('\n').collect();
        if texts.len() != record.runs.len() || !text.ends_with('\n') {
            self.err(format!(
                "{path}: {} lines for {} runs in hours/{n}.json",
                texts.len(),
                record.runs.len()
            ));
            return None;
        }
        let known = crate::hour::known_strains(state, &record.holders);
        let mut lines = Vec::new();
        let mut ok = true;
        for (i, (t, r)) in texts.iter().zip(&record.runs).enumerate() {
            let at = format!("{path} line {}", i + 1);
            if crate::sha256_hex(t.as_bytes()) != r.sha {
                self.err(format!("{at}: hash does not match hours/{n}.json"));
                ok = false;
                continue;
            }
            let line: RunLine = match serde_json::from_str(t) {
                Ok(l) => l,
                Err(e) => {
                    self.err(format!("{at}: {e}"));
                    ok = false;
                    continue;
                }
            };
            if (line.body.as_str(), line.kind) != (r.body.as_str(), r.kind) {
                self.err(format!("{at}: body or kind differ from hours/{n}.json"));
                ok = false;
            }
            if let Err(e) = replay(lab, &known, &line) {
                self.err(format!("{at}: replay: {e}"));
                ok = false;
            }
            lines.push(line);
        }
        ok.then_some(lines)
    }

    /// Every run of the record, published and withheld, as a commitment with its outcome and whether it was withheld;
    /// `None` when the outcomes do not line up with them.
    fn commitments(
        &mut self,
        path: &str,
        record: &HourRecord,
    ) -> Option<Vec<(Withheld, Outcome, bool)>> {
        let mut merged: Vec<(&str, RunKind, &str, bool)> = record
            .runs
            .iter()
            .map(|r| (r.body.as_str(), r.kind, r.sha.as_str(), false))
            .chain(
                record
                    .withheld
                    .iter()
                    .map(|w| (w.body.as_str(), w.kind, w.commit.as_str(), true)),
            )
            .collect();
        merged.sort();
        if merged.len() != record.outcomes.len() && record.void_reason().is_none() {
            self.err(format!("{path}: outcomes do not match runs and withheld"));
            return None;
        }
        Some(
            merged
                .iter()
                .zip(&record.outcomes)
                .map(|((body, kind, commit, kept), o)| {
                    let w = Withheld {
                        body: body.to_string(),
                        kind: *kind,
                        commit: commit.to_string(),
                    };
                    (w, o.outcome.clone(), *kept)
                })
                .collect(),
        )
    }

    /// What an hour whose runs are not available still proves: its holder entries follow the cursor and the gate,
    /// its runs are the planned ones, it ran the drawn strain, and it credits the right cursor, burns and payloads.
    fn check_unrecomputed(&mut self, path: &str, record: &HourRecord, lab: &Lab, state: &State) {
        let Some(all) = self.commitments(path, record) else {
            return;
        };
        let mut flags = record.flags.clone();
        flags.push(Flag::Sealed);
        let inputs = Inputs {
            n: record.n,
            prev: record.prev.clone(),
            draw: record.draw.clone(),
            void: None,
            flags,
            holders: record.holders.clone(),
            withheld: all.into_iter().map(|(w, o, _)| (w, o)).collect(),
        };
        let again = match advance(lab, state, &inputs, &[]) {
            Ok((again, _)) => again,
            Err(e) => return self.err(format!("{path}: {e}")),
        };
        let (a, b) = (&again.state, &record.state);
        if (&a.holder_cursor, &a.burns, &a.payloads) != (&b.holder_cursor, &b.burns, &b.payloads) {
            self.err(format!(
                "{path}: holder_cursor, burns or payloads differ from its holder entries"
            ));
        }
        let drawn = record
            .draw
            .as_ref()
            .and_then(|d| d.hash_bytes().ok())
            .and_then(|hash| plan(lab, state, record.n, &hash, &record.holders).ok())
            .map(|p| p.strain.id);
        if !record.flags.contains(&Flag::Sealed) && record.strain != drawn {
            self.err(format!("{path}: strain is not the one drawn"));
        }
    }

    /// Compares the card file of the last hour of `history`, when published, with the card the core tells. The runner
    /// writes the LATE note from its own clock as the anchor goes out, so the card may carry it or not.
    fn check_card(&mut self, history: &[HourRecord], library: &Library) {
        let Some(n) = history.last().map(|r| r.n) else {
            return;
        };
        let Some(bytes) = self.bundle.cards.get(&n) else {
            return;
        };
        let told = |late| {
            crate::card::file(history, library, late).and_then(|f| crate::json::to_vec(&f).ok())
        };
        if ![false, true]
            .into_iter()
            .any(|late| told(late).as_deref() == Some(bytes.as_slice()))
        {
            self.err(format!(
                "cards/{n}.json: differs from the card of hours/{n}.json"
            ));
        }
    }

    fn recompute(
        &mut self,
        path: &str,
        bytes: &[u8],
        record: &HourRecord,
        lab: &Lab,
        state: &State,
        lines: &[RunLine],
    ) {
        let Some(all) = self.commitments(path, record) else {
            return;
        };
        let withheld = all
            .into_iter()
            .filter(|(_, _, kept)| *kept)
            .map(|(w, o, _)| (w, o))
            .collect();
        let inputs = Inputs {
            n: record.n,
            prev: record.prev.clone(),
            draw: record.draw.clone(),
            void: record.void_reason(),
            flags: record.flags.clone(),
            holders: record.holders.clone(),
            withheld,
        };
        match advance(lab, state, &inputs, lines) {
            Err(e) => self.err(format!("{path}: {e}")),
            Ok((again, _)) => match again.encode() {
                Ok((written, _)) if written == bytes => {}
                Ok((written, _)) => self.err(format!(
                    "{path}: {} differs from the recomputation",
                    first_difference(bytes, &written)
                )),
                Err(e) => self.err(format!("{path}: {e}")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hour::tests::{inputs, lab, lines};

    fn exam_hour(void: impl Fn(&RunLine) -> Option<VoidReason>) -> HourRecord {
        let lab = lab();
        let state = State::genesis(&lab).expect("genesis");
        let mut lines = lines(&lab, &state, 0, |_| false);
        for l in &mut lines {
            if let Some(reason) = void(l) {
                l.outcome = Outcome::void(reason);
            }
        }
        advance(&lab, &state, &inputs(0), &lines)
            .expect("advances")
            .0
    }

    #[test]
    fn the_headline_keeps_on_time_unsealed_hours_and_forgives_only_the_priors_row_paused() {
        let clean = exam_hour(|_| None);
        assert!(in_headline(&clean, false));
        assert!(!in_headline(&clean, true), "LATE");
        let mut sealed = clean.clone();
        sealed.flags.push(Flag::Sealed);
        assert!(!in_headline(&sealed, false));
        let mut training = clean.clone();
        training.exam = false;
        assert!(!in_headline(&training, false));

        let priors_out = exam_hour(|l| l.body.ends_with("-priors").then_some(VoidReason::Paused));
        assert_eq!(priors_out.void_share_ppm, 200_000);
        assert_eq!(headline_void_ppm(&priors_out), 0);
        assert!(
            in_headline(&priors_out, false),
            "the Priors row out of the ward"
        );
        let timed_out = exam_hour(|l| l.body.ends_with("-priors").then_some(VoidReason::Timeout));
        assert_eq!(headline_void_ppm(&timed_out), 200_000);
        assert!(!in_headline(&timed_out, false));

        let attack = |l: &RunLine| l.kind == RunKind::Attack;
        let relabelled = exam_hour(|l| {
            (attack(l) && !l.body.ends_with("-priors")).then_some(VoidReason::Paused)
        });
        assert_eq!(headline_void_ppm(&relabelled), 266_666);
        assert!(
            !in_headline(&relabelled, false),
            "paused runs off the Priors row"
        );
        let one_more = exam_hour(|l| {
            (l.body.ends_with("-priors") || (attack(l) && l.body == "m1-none"))
                .then_some(VoidReason::Paused)
        });
        assert_eq!(headline_void_ppm(&one_more), 216_666);
        let cut = exam_hour(|l| {
            l.body.ends_with("-priors").then_some(if attack(l) {
                VoidReason::Timeout
            } else {
                VoidReason::Paused
            })
        });
        assert_eq!(
            headline_void_ppm(&cut),
            66_666,
            "Priors out, its attack runs cut by the deadline: only those count"
        );
        let part = exam_hour(|l| {
            (l.body.ends_with("-priors") && !attack(l)).then_some(VoidReason::Paused)
        });
        assert_eq!(
            headline_void_ppm(&part),
            133_333,
            "part of the Priors row paused while the rest ran"
        );

        let lab = lab();
        let state = State::genesis(&lab).expect("genesis");
        let mut i = inputs(0);
        (i.void, i.draw) = (Some(VoidReason::Paused), None);
        let paused = advance(&lab, &state, &i, &[]).expect("advances").0;
        assert_eq!(headline_void_ppm(&paused), 1_000_000);
        assert!(!in_headline(&paused, false), "a paused period");
    }

    #[test]
    fn priors_may_be_paused_from_a_sealed_hour_until_a_priors_run_counts() {
        let clean = exam_hour(|_| None);
        let out = exam_hour(|l| l.body.ends_with("-priors").then_some(VoidReason::Paused));
        let mut sealed = clean.clone();
        sealed.flags.push(Flag::Sealed);
        let lab = lab();
        let state = State::genesis(&lab).expect("genesis");
        let mut i = inputs(0);
        (i.void, i.draw) = (Some(VoidReason::Paused), None);
        let pause = advance(&lab, &state, &i, &[]).expect("advances").0;
        i.void = Some(VoidReason::RunnerDown);
        let down = advance(&lab, &state, &i, &[]).expect("advances").0;

        assert!(pauses_priors(&out) && !pauses_priors(&clean));
        assert!(!pauses_priors(&pause), "a paused period may come any time");
        let mut window = false;
        for (record, open) in [
            (&clean, false),
            (&sealed, true),
            (&out, true),
            (&down, true),
            (&pause, true),
            (&clean, false),
            (&out, false),
        ] {
            window = priors_out_after(window, record);
            assert_eq!(window, open);
        }
    }
}
