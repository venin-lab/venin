//! The synthetic lab end to end: a golden fixture checked byte for byte, verify over a whole history, tamper
//! detection, and story coverage.

#[path = "../examples/synthetic/fake.rs"]
mod fake;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use venin_core::card::{Story, caption_ok, card};
use venin_core::hour::{Flag, HourRecord, Inputs, Withheld, advance, focal};
use venin_core::lab::{Lab, RULES_VERSION};
use venin_core::record::{RunKind, RunLine, Usage, VoidReason};
use venin_core::verdict::{Class, Outcome};
use venin_core::verify::{
    AnchorEntry, AnchorKind, Bundle, Chain, Head, LibraryFiles, Report, calldata,
    headline_void_ppm, in_headline, verify,
};

type Files = BTreeMap<String, Vec<u8>>;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn library() -> LibraryFiles {
    fake::read_library(&fixtures().join("library")).expect("the fixture library reads")
}

fn generate(periods: u64) -> Files {
    fake::generate(&library(), periods).expect("generates")
}

fn number(path: &str, prefix: &str, suffix: &str) -> Option<u64> {
    path.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse()
        .ok()
}

fn bundle(files: &Files) -> (Bundle, Chain) {
    let mut b = Bundle {
        manifest: files["manifest.json"].clone(),
        exam: files["exam.json"].clone(),
        anchors: files["anchors.json"].clone(),
        head: files["head.json"].clone(),
        ..Bundle::default()
    };
    for (path, bytes) in files {
        if let Some(n) = number(path, "hours/", ".json") {
            b.hours.insert(n, bytes.clone());
        }
        if let Some(n) = number(path, "runs/", ".jsonl") {
            b.runs.insert(n, bytes.clone());
        }
        if let Some(n) = number(path, "cards/", ".json") {
            b.cards.insert(n, bytes.clone());
        }
    }
    let chain = serde_json::from_slice(&files["chain.json"]).expect("chain parses");
    (b, chain)
}

fn check(files: &Files) -> Report {
    let (b, chain) = bundle(files);
    verify(&b, &chain, &library())
}

fn records(files: &Files) -> Vec<HourRecord> {
    let mut out: Vec<(u64, HourRecord)> = files
        .iter()
        .filter_map(|(p, bytes)| {
            Some((
                number(p, "hours/", ".json")?,
                serde_json::from_slice(bytes).expect("parses"),
            ))
        })
        .collect();
    out.sort_by_key(|(n, _)| *n);
    out.into_iter().map(|(_, r)| r).collect()
}

#[test]
fn the_golden_fixture_is_byte_exact() {
    let golden = fixtures().join("golden");
    let files: Files = generate(2)
        .into_iter()
        .filter(|(p, _)| !p.starts_with("library/"))
        .collect();
    if std::env::var_os("VENIN_BLESS").is_some() {
        for (path, bytes) in &files {
            let target = golden.join(path);
            std::fs::create_dir_all(target.parent().expect("a parent")).expect("mkdir");
            std::fs::write(target, bytes).expect("writes");
        }
    }
    let mut on_disk = BTreeSet::new();
    for dir in ["", "cards", "hours", "runs"] {
        for entry in std::fs::read_dir(golden.join(dir)).expect("golden dir") {
            let path = entry.expect("entry").path();
            if path.is_file() {
                let rel = path.strip_prefix(&golden).expect("inside");
                on_disk.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    assert_eq!(on_disk, files.keys().cloned().collect::<BTreeSet<_>>());
    for (path, bytes) in &files {
        let expected = std::fs::read(golden.join(path)).expect("golden file");
        assert!(expected == *bytes, "{path} differs from the golden fixture");
    }
    let runs = String::from_utf8(files["runs/1.jsonl"].clone()).expect("ascii");
    assert!(
        runs.contains("\\ud834\\udd1e"),
        "an astral character, escaped"
    );
    assert!(
        runs.bytes()
            .all(|b| b == b'\n' || (0x20..0x7f).contains(&b))
    );
    assert!(check(&generate(2)).ok());
}

#[test]
fn a_synthetic_history_verifies_and_tells_every_story() {
    let files = generate(48);
    let report = check(&files);
    assert!(report.ok(), "{:?}", report.errors);
    assert_eq!(report.hours, 48);
    assert_eq!(report.late, [20]);
    assert_eq!(
        report.headline_hours,
        [0, 5, 10, 15, 25, 35, 40, 45],
        "not LATE 20 nor void 30"
    );
    assert_eq!(report.withheld, 4);
    assert_eq!(report.unchecked, 0);
    assert!(report.void_share_ppm > 0);
    assert_eq!(report.headline.len(), 5);
    let manifest: serde_json::Value =
        serde_json::from_slice(&files["manifest.json"]).expect("json");
    assert_eq!(manifest["league"], fake::LEAGUE);

    let history = records(&files);
    let library = library().parse().expect("library");
    let mut stories = BTreeSet::new();
    let mut events = BTreeSet::new();
    for i in 0..history.len() {
        let c = card(&history[..=i], &library, report.late.contains(&(i as u64))).expect("a card");
        assert_eq!(caption_ok(&c.caption), Ok(()), "{}", c.caption);
        let file: serde_json::Value =
            serde_json::from_slice(&files[&format!("cards/{i}.json")]).expect("a card file");
        let told = match &file["note"] {
            serde_json::Value::String(note) => format!(
                "{}\nNote: {note}",
                file["caption"].as_str().expect("caption")
            ),
            _ => file["caption"].as_str().expect("caption").to_string(),
        };
        assert_eq!(told, c.caption);
        assert_eq!(
            file["focal"]["body"].as_str(),
            focal(&history[..=i]).as_ref().map(|f| f.1.as_str())
        );
        stories.insert(c.story);
        assert!(focal(&history[..=i]).is_some());
        for e in &history[i].events {
            let tag = serde_json::to_value(e).expect("json")["type"].clone();
            events.insert(tag.as_str().expect("a tag").to_string());
        }
    }
    let all = [
        Story::Advisory,
        Story::Void,
        Story::Concern,
        Story::Outbreak,
        Story::Exam,
        Story::Gene,
        Story::Extinction,
        Story::Move,
        Story::Quiet,
    ];
    assert_eq!(stories, all.into_iter().collect(), "{stories:?}");
    for kind in [
        "concern",
        "mutant",
        "extinct",
        "kept",
        "replaced",
        "remutated",
    ] {
        assert!(events.contains(kind), "{kind} in {events:?}");
    }
}

fn flipped(files: &Files, path: &str, at: impl Fn(&[u8]) -> usize) -> Files {
    let mut out = files.clone();
    let bytes = out.get_mut(path).expect("the file");
    let i = at(bytes);
    bytes[i] = if bytes[i] == b'7' { b'8' } else { b'7' };
    out
}

fn named(files: &Files, what: &str) {
    let report = check(files);
    assert!(
        report.errors.iter().any(|e| e.contains(what)),
        "{what} not in {:?}",
        report.errors
    );
}

#[test]
fn one_flipped_byte_is_named() {
    let files = generate(4);
    assert!(check(&files).ok());
    named(
        &flipped(&files, "hours/2.json", |b| b.len() / 2),
        "hours/2.json",
    );
    named(
        &flipped(&files, "hours/2.json", |b| b.len() / 2),
        "hours/3.json: prev",
    );
    let second_line = |b: &[u8]| b.iter().position(|c| *c == b'\n').expect("a line") + 40;
    named(
        &flipped(&files, "runs/3.jsonl", second_line),
        "runs/3.jsonl.gz line 2",
    );
    named(
        &flipped(&files, "manifest.json", |b| b.len() - 30),
        "manifest.json",
    );
    named(&flipped(&files, "exam.json", |b| b.len() - 5), "exam.json");
    let at = |key: &'static str, skip: usize| {
        move |b: &[u8]| {
            std::str::from_utf8(b)
                .expect("ascii")
                .find(key)
                .expect("a key")
                + skip
        }
    };
    named(
        &flipped(&files, "anchors.json", at("\"block\":", 10)),
        "anchors.json entry 1 (manifest.json): block differs",
    );
    let swapped = flipped(&files, "anchors.json", at("\"tx\":\"0x", 12));
    let unknown_tx = check(&swapped);
    assert!(unknown_tx.ok(), "{:?}", unknown_tx.errors);
    assert_eq!(
        unknown_tx.unchecked, 2,
        "a transaction the chain facts lack is unchecked, and so is the exam draw time it fixes"
    );
}

fn anchor_txs(files: &Files) -> Vec<String> {
    let anchors: Vec<venin_core::verify::AnchorEntry> =
        serde_json::from_slice(&files["anchors.json"]).expect("anchors");
    anchors.into_iter().map(|a| a.tx).collect()
}

#[test]
fn an_anchor_the_chain_does_not_hold_is_an_error() {
    let files = generate(3);
    let txs = anchor_txs(&files);
    let (b, chain) = bundle(&files);
    let mut absent = chain.clone();
    absent.txs.remove(&txs[2]);
    absent.absent.insert(txs[2].clone());
    let report = verify(&b, &absent, &library());
    assert!(
        report
            .errors
            .contains(&"anchors.json entry 3 (hours/0.json): anchor tx not on chain".to_string()),
        "{:?}",
        report.errors
    );

    let mut pending = chain.clone();
    for tx in [&txs[2], &txs[4]] {
        pending.txs.remove(tx);
        pending.pending.insert(tx.clone());
    }
    let report = verify(&b, &pending, &library());
    assert_eq!(
        report.errors,
        ["anchors.json entry 3 (hours/0.json): anchor tx not mined"],
        "only the head hour may be pending"
    );
    assert_eq!(report.unchecked, 1);
}

#[test]
fn edits_that_rehash_are_still_caught_by_recomputation() {
    let files = generate(3);
    let mut forged = files.clone();
    let text = String::from_utf8(forged["hours/2.json"].clone()).expect("ascii");
    let edited = text.replacen("\"spend_micro\":", "\"spend_micro\":1", 1);
    forged.insert("hours/2.json".into(), edited.into_bytes());
    named(
        &forged,
        "hours/2.json: spend_micro differs from the recomputation",
    );

    let ours = format!("\"rules_version\":{RULES_VERSION}");
    for v in (1..RULES_VERSION).chain([RULES_VERSION + 4]) {
        for path in ["manifest.json", "hours/1.json"] {
            let mut wrong_version = files.clone();
            let text = String::from_utf8(wrong_version[path].clone()).expect("ascii");
            assert!(text.contains(&ours), "{path}");
            wrong_version.insert(
                path.into(),
                text.replacen(&ours, &format!("\"rules_version\":{v}"), 1)
                    .into_bytes(),
            );
            let report = check(&wrong_version);
            assert_eq!(report.made_by, Some(v), "{path}");
            assert!(
                report
                    .errors
                    .iter()
                    .any(|e| e.contains(&format!("made by venin v{v}"))),
                "{path}: {:?}",
                report.errors
            );
            assert!(!report.ok());
        }
    }
}

#[test]
fn a_missing_runs_file_is_an_error_unless_taken_down() {
    let files = generate(3);
    let (mut b, chain) = bundle(&files);
    b.runs.remove(&1);
    let report = verify(&b, &chain, &library());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e == "runs/1.jsonl.gz: missing")
    );
    b.takedowns.push(venin_core::verify::Takedown {
        path: "runs/1.jsonl.gz".into(),
        sha: "0".repeat(64),
        date_s: 0,
        reason: "legal".into(),
    });
    let report = verify(&b, &chain, &library());
    assert!(report.ok(), "{:?}", report.errors);
    assert_eq!(report.taken_down, ["runs/1.jsonl.gz"]);
    assert!(Path::new(env!("CARGO_MANIFEST_DIR")).exists());
}

#[test]
fn a_taken_down_hour_is_checked_without_its_runs_and_left_out_of_the_headline() {
    let files = generate(3);
    let hour0 = &records(&files)[0];
    let listed = |files: &Files, served: bool| {
        let (mut b, chain) = bundle(files);
        if !served {
            b.runs.remove(&0);
        }
        b.takedowns.push(venin_core::verify::Takedown {
            path: "runs/0.jsonl.gz".into(),
            sha: "0".repeat(64),
            date_s: 0,
            reason: "legal".into(),
        });
        verify(&b, &chain, &library())
    };
    let takedown = |files: &Files| listed(files, false);
    assert!(!check(&files).headline.is_empty());
    let report = takedown(&files);
    assert!(report.ok(), "{:?}", report.errors);
    assert_eq!(report.not_recomputed, [0]);
    assert_eq!(report.withheld, hour0.runs.len() as u64);
    assert!(
        report.headline.is_empty() && report.headline_hours.is_empty(),
        "hour 0 is the only exam hour, and none of its runs replayed"
    );
    assert_eq!(
        listed(&files, true),
        report,
        "a runs file listed as taken down counts as taken down while it is still served"
    );

    let strain = format!(
        "\"strain\":\"{}\"",
        hour0.strain.as_deref().expect("a strain")
    );
    for (from, to, what) in [
        (
            "\"holder_cursor\":\"\"",
            "\"holder_cursor\":\"1-0xab\"",
            "hours/0.json: holder_cursor, burns or payloads differ",
        ),
        (
            strain.as_str(),
            "\"strain\":\"EYE-1-0\"",
            "hours/0.json: strain is not the one drawn",
        ),
    ] {
        let mut forged = files.clone();
        let text = String::from_utf8(forged["hours/0.json"].clone()).expect("ascii");
        assert!(text.contains(from), "{from}");
        forged.insert(
            "hours/0.json".into(),
            text.replacen(from, to, 1).into_bytes(),
        );
        let report = takedown(&forged);
        assert!(
            report.errors.iter().any(|e| e.starts_with(what)),
            "{what} not in {:?}",
            report.errors
        );
    }
}

#[test]
fn head_exam_timing_and_cards_are_checked() {
    let files = generate(3);
    let mut head = files.clone();
    let text = String::from_utf8(head["head.json"].clone()).expect("ascii");
    let sha = venin_core::sha256_hex(&files["hours/2.json"]);
    head.insert(
        "head.json".into(),
        text.replace(&sha, &"0".repeat(64)).into_bytes(),
    );
    named(&head, "head.json: sha is not the hash of hours/2.json");

    let txs = anchor_txs(&files);
    let (b, chain) = bundle(&files);
    let shifted = |i: usize, by: i64| {
        let mut c = chain.clone();
        c.txs.get_mut(&txs[i]).expect("a tx").ts += by;
        verify(&b, &c, &library()).errors
    };
    assert!(
        shifted(0, 1).contains(
            &"exam.json: the draw block is not the first block at its draw time".to_string()
        ),
        "the exam block must be the first at the manifest anchor's time plus exam_delay_s"
    );
    assert!(shifted(1, 3000).contains(&"exam.json: anchored after the genesis time".to_string()));
    assert!(shifted(1, 2999).is_empty());

    let mut card = files.clone();
    let text = String::from_utf8(card["cards/1.json"].clone()).expect("ascii");
    card.insert(
        "cards/1.json".into(),
        text.replacen("\"caption\":\"", "\"caption\":\"Edited. ", 1)
            .into_bytes(),
    );
    named(&card, "cards/1.json: differs from the card of hours/1.json");
}

fn run_lines(files: &Files, n: u64) -> Vec<RunLine> {
    std::str::from_utf8(&files[&format!("runs/{n}.jsonl")])
        .expect("ascii")
        .lines()
        .map(|l| serde_json::from_str(l).expect("a run line"))
        .collect()
}

/// A run as the runner writes one that never started: no model call, no tokens, no cost.
fn never_ran(line: &mut RunLine, reason: VoidReason) {
    line.responses.clear();
    line.calls.clear();
    line.external.clear();
    line.usage = Usage::default();
    (line.latency_ms, line.cost_micro) = (0, 0);
    line.outcome = Outcome::void(reason);
}

/// Writes hour `n` again from `lines` and the inputs `edit` leaves, as its publisher could: the runs file, the hour,
/// its card, head.json when it names the hour, and the anchor's calldata all follow. A later hour must then be written
/// again too, as it no longer chains.
fn republish(
    files: &mut Files,
    n: u64,
    lines: &[RunLine],
    edit: impl FnOnce(&mut Inputs),
) -> HourRecord {
    let lab = Lab {
        manifest: serde_json::from_slice(&files["manifest.json"]).expect("manifest"),
        exam: serde_json::from_slice(&files["exam.json"]).expect("exam"),
        library: library().parse().expect("library"),
    };
    let mut history = records(files);
    let at = usize::try_from(n).expect("small");
    let mut inputs = Inputs {
        n,
        prev: venin_core::sha256_hex(&files[&format!("hours/{}.json", n - 1)]),
        draw: history[at].draw.clone(),
        void: None,
        flags: Vec::new(),
        holders: Vec::new(),
        defenders: Vec::new(),
        withheld: Vec::new(),
    };
    edit(&mut inputs);
    let (record, _) = advance(&lab, &history[at - 1].state, &inputs, lines).expect("advances");
    let (bytes, sha) = record.encode().expect("encodes");
    let texts: BTreeMap<String, String> = lines
        .iter()
        .map(|l| {
            let (text, sha) = l.encode().expect("encodes");
            (sha, text)
        })
        .collect();
    let runs: String = record
        .runs
        .iter()
        .map(|r| format!("{}\n", texts[&r.sha]))
        .collect();
    files.insert(format!("runs/{n}.jsonl"), runs.into_bytes());
    files.insert(format!("hours/{n}.json"), bytes);
    let anchors: Vec<AnchorEntry> =
        serde_json::from_slice(&files["anchors.json"]).expect("anchors");
    let anchor = anchors
        .iter()
        .find(|a| a.kind == AnchorKind::Hour && a.n == n)
        .expect("anchored");
    let mut chain: Chain = serde_json::from_slice(&files["chain.json"]).expect("chain");
    chain.txs.get_mut(&anchor.tx).expect("its tx").input = calldata(AnchorKind::Hour, n, &sha);
    files.insert(
        "chain.json".into(),
        venin_core::json::to_vec(&chain).expect("writes"),
    );
    let head: Head = serde_json::from_slice(&files["head.json"]).expect("head");
    if head.n == n {
        files.insert(
            "head.json".into(),
            venin_core::json::to_vec(&Head { sha, ..head }).expect("writes"),
        );
    }
    history[at] = record.clone();
    let card = venin_core::card::file(&history[..=at], &lab.library, false).expect("a card");
    files.insert(
        format!("cards/{n}.json"),
        venin_core::json::to_vec(&card).expect("writes"),
    );
    record
}

/// `paused` is what the runner writes for every run of the Priors row while Priors is out of the ward, which only a
/// seal starts, and for the whole period after `venin pause`. Both verify, and the exam hour keeps its place in the
/// headline only in the first; a Priors row paused with no sealed hour before it does not verify. Ward runs relabelled
/// `paused` fail replay, and would void the hour out of the headline besides.
#[test]
fn paused_runs_verify_only_where_the_runner_writes_them() {
    let n = 15;
    let base = generate(n + 1);
    let before = check(&base);
    assert!(before.ok(), "{:?}", before.errors);
    let rows = |report: &Report| -> BTreeMap<String, u64> {
        report
            .headline
            .iter()
            .map(|r| (r.row.clone(), r.attacks))
            .collect()
    };
    let priors = |cut: bool| {
        let mut lines = run_lines(&base, n);
        for l in lines.iter_mut().filter(|l| l.body.ends_with("-priors")) {
            let deadline = cut && l.kind == RunKind::Attack;
            never_ran(
                l,
                if deadline {
                    VoidReason::Timeout
                } else {
                    VoidReason::Paused
                },
            );
        }
        lines
    };
    let priors_attacks = run_lines(&base, n)
        .iter()
        .filter(|l| l.body.ends_with("-priors") && l.kind == RunKind::Attack)
        .filter(|l| l.outcome.class != Class::Void)
        .count() as u64;
    assert!(priors_attacks > 0);

    let mut files = base.clone();
    let record = republish(&mut files, n, &priors(false), |_| {});
    assert!(record.exam && in_headline(&record, false));
    assert_eq!(
        check(&files).errors,
        [format!(
            "hours/{n}.json: Priors runs are paused, but no sealed hour took Priors out of the ward"
        )]
    );

    let mut sealed = base.clone();
    let withheld: Vec<(Withheld, Outcome)> = run_lines(&base, n - 1)
        .into_iter()
        .map(|l| {
            let (_, commit) = l.encode().expect("encodes");
            let w = Withheld {
                body: l.body,
                kind: l.kind,
                commit,
            };
            (w, l.outcome)
        })
        .collect();
    let record = republish(&mut sealed, n - 1, &[], |i| {
        i.flags = vec![Flag::Sealed];
        i.withheld = withheld;
    });
    assert_eq!(
        (record.flags.as_slice(), record.strain),
        (&[Flag::Sealed][..], None)
    );
    for cut in [false, true] {
        let mut files = sealed.clone();
        let record = republish(&mut files, n, &priors(cut), |_| {});
        assert!(record.exam && in_headline(&record, false), "cut: {cut}");
        let report = check(&files);
        assert!(report.ok(), "{:?}", report.errors);
        let mut expected = rows(&before);
        *expected.get_mut("priors").expect("the Priors row") -= priors_attacks;
        assert_eq!(
            rows(&report),
            expected,
            "only the Priors row lost its attacks"
        );
    }

    let mut files = base.clone();
    let record = republish(&mut files, n, &[], |i| {
        i.void = Some(VoidReason::Paused);
        i.draw = None;
    });
    assert_eq!(record.flags, [Flag::Paused]);
    assert_eq!(headline_void_ppm(&record), 1_000_000);
    let report = check(&files);
    assert!(report.ok(), "{:?}", report.errors);

    let mut files = base.clone();
    let mut lines = run_lines(&files, n);
    for l in lines
        .iter_mut()
        .filter(|l| l.kind == RunKind::Attack && !l.body.ends_with("-priors"))
    {
        never_ran(l, VoidReason::Paused);
    }
    let record = republish(&mut files, n, &lines, |_| {});
    assert!(!in_headline(&record, false), "16 of 60 runs void");
    let report = check(&files);
    assert_eq!(report.errors.len(), 16, "{:?}", report.errors);
    let refused = "replay: void (Paused) without what the runner records for it";
    assert!(
        report.errors.iter().all(|e| e.ends_with(refused)),
        "{:?}",
        report.errors
    );
}
