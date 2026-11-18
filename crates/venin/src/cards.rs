//! `cards/<n>.json`: the hour's card as the core card module tells it, so the site reads the caption instead of
//! recomputing it. `venin cards --data DIR` writes the card files again after a caption change.

use crate::store::{Store, encode};
use std::path::Path;
use venin_core::card::{LATE_NOTE, file as build};
use venin_core::hour::HourRecord;
use venin_core::lab::Library;

/// Every hour of the data dir, in order.
fn history(store: &Store) -> Result<Vec<HourRecord>, String> {
    store
        .hours()?
        .into_iter()
        .map(|(n, b)| serde_json::from_slice(&b).map_err(|e| format!("hours/{n}.json: {e}")))
        .collect()
}

/// Writes `cards/<n>.json` for the last hour in the data dir.
pub fn write(store: &Store, library: &Library, late: bool) -> Result<Option<u64>, String> {
    let Some(file) = build(&history(store)?, library, late) else {
        return Ok(None);
    };
    store.write(&format!("cards/{}.json", file.n), &encode(&file)?)?;
    Ok(Some(file.n))
}

/// Tells the card of each hour that has a card file again, from the data dir's record and library, with the file's
/// LATE note kept as it was (the runner set it from its own clock). Every card is told before any is written, and only
/// the files that differ are written; returns them and the number of card files read. Pictures and hours without a
/// card file are left alone.
pub fn rewrite(data: &Path) -> Result<(Vec<String>, usize), String> {
    let store = Store::new(data.to_path_buf(), data.join("state-unused"));
    let library = store
        .library()?
        .parse()
        .map_err(|e| format!("library: {e}"))?;
    let history = history(&store)?;
    let (mut changed, mut read) = (Vec::new(), 0);
    for (i, record) in history.iter().enumerate() {
        let path = format!("cards/{}.json", record.n);
        let Some(old) = store.read(&path)? else {
            continue;
        };
        read += 1;
        let was: serde_json::Value =
            serde_json::from_slice(&old).map_err(|e| format!("{path}: {e}"))?;
        let late = was["note"]
            .as_str()
            .is_some_and(|note| note.starts_with(LATE_NOTE));
        let new = build(&history[..=i], &library, late)
            .ok_or_else(|| format!("{path}: no card"))
            .and_then(|f| encode(&f))?;
        if new != old {
            changed.push((path, new));
        }
    }
    for (path, bytes) in &changed {
        store.write(path, bytes)?;
    }
    Ok((changed.into_iter().map(|(path, _)| path).collect(), read))
}

pub fn run(data: &Path) -> Result<(), String> {
    let (changed, read) = rewrite(data)?;
    for path in &changed {
        println!("rewrote {path}");
    }
    println!("{} of {read} card files rewritten", changed.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use venin_core::card::Story;

    #[test]
    fn the_note_line_is_split_from_the_caption() {
        let (lab, state) = crate::model::tests::test_lab();
        let inputs = venin_core::hour::Inputs {
            n: 3,
            prev: "0".repeat(64),
            draw: None,
            void: Some(venin_core::record::VoidReason::RunnerDown),
            flags: Vec::new(),
            holders: Vec::new(),
            withheld: Vec::new(),
        };
        let (record, _) =
            venin_core::hour::advance(&lab, &state, &inputs, &[]).expect("a void hour");
        let on_time = build(std::slice::from_ref(&record), &lab.library, false).expect("a card");
        assert_eq!(
            (on_time.n, on_time.story, on_time.note.as_deref()),
            (3, Story::Void, None)
        );
        assert_eq!(on_time.focal, None, "no hour had a run");
        let late = build(&[record], &lab.library, true).expect("a card");
        assert_eq!(late.caption, on_time.caption);
        assert_eq!(
            late.note.as_deref(),
            Some("the anchor landed late; off the scoreboard.")
        );
        let text = String::from_utf8(encode(&late).expect("encodes")).expect("ascii");
        assert!(
            text.starts_with(r#"{"n":3,"story":"void","caption":"#),
            "{text}"
        );
        assert!(
            text.ends_with(
                r#","note":"the anchor landed late; off the scoreboard.","focal":null}"#
            )
        );
    }

    /// The golden record of the core's tests as a data dir: runs gzipped, the fixture library under `library/`.
    fn golden_dir(name: &str) -> std::path::PathBuf {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures");
        let data = crate::anchor::tests::temp(name).join("data");
        let store = Store::new(data.clone(), data.join("state-unused"));
        let golden = fixtures.join("golden");
        for rel in [
            "manifest.json",
            "exam.json",
            "anchors.json",
            "head.json",
            "hours/0.json",
            "hours/1.json",
            "cards/0.json",
            "cards/1.json",
        ] {
            let bytes = std::fs::read(golden.join(rel)).expect("a golden file");
            store.write(rel, &bytes).expect("writes");
        }
        for n in 0..2 {
            let text = std::fs::read_to_string(golden.join(format!("runs/{n}.jsonl")))
                .expect("a golden runs file");
            store.write_runs(n, &text).expect("writes");
        }
        for name in crate::store::LIBRARY_FILES {
            let bytes = std::fs::read(fixtures.join("library").join(name)).expect("a library file");
            store
                .write(&format!("library/{name}"), &bytes)
                .expect("writes");
        }
        data
    }

    /// Card files told by older templates, or edited, verify again once rewritten; a LATE note stays, an hour
    /// without a card file gets none, the picture is not touched, and a second run changes nothing.
    #[test]
    fn rewritten_cards_verify_and_a_second_run_changes_nothing() {
        let data = golden_dir("cards");
        let verified = |data: &Path| crate::verify::check(data, None, None).expect("reads");
        assert!(verified(&data).ok(), "{:?}", verified(&data).errors);
        let card = |rel: &str| std::fs::read(data.join(rel)).expect("a card file");
        let text = |rel: &str| String::from_utf8(card(rel)).expect("ascii");
        let edited0 = text("cards/0.json").replacen(r#""caption":""#, r#""caption":"Edited. "#, 1);
        let old = text("cards/1.json")
            .replace("left circulation", "died out")
            .replace(
                r#""note":null"#,
                r#""note":"the anchor landed late; off the scoreboard.""#,
            );
        assert_ne!(old, text("cards/1.json"));
        std::fs::write(data.join("cards/1.json"), &old).expect("writes");
        std::fs::remove_file(data.join("cards/0.json")).expect("removes");
        std::fs::write(data.join("cards/1.png"), b"picture").expect("writes");
        assert_eq!(
            verified(&data).errors,
            ["cards/1.json: differs from the card of hours/1.json"]
        );

        assert_eq!(
            rewrite(&data).expect("rewrites"),
            (vec!["cards/1.json".to_string()], 1)
        );
        let report = verified(&data);
        assert!(report.ok(), "{:?}", report.errors);
        let told = text("cards/1.json");
        assert!(told.contains("left circulation"), "{told}");
        assert!(
            told.contains(r#""note":"the anchor landed late; off the scoreboard.""#),
            "{told}"
        );
        assert!(!data.join("cards/0.json").exists());
        assert_eq!(card("cards/1.png"), b"picture");

        assert_eq!(rewrite(&data).expect("rewrites"), (Vec::new(), 1));
        assert_eq!(text("cards/1.json"), told);

        std::fs::write(data.join("cards/0.json"), &edited0).expect("writes");
        std::fs::write(data.join("cards/1.json"), b"not json").expect("writes");
        let err = rewrite(&data).expect_err("a card file that is not JSON");
        assert!(err.starts_with("cards/1.json: "), "{err}");
        assert_eq!(
            text("cards/0.json"),
            edited0,
            "nothing is written when a card fails"
        );
    }
}
