//! The browser's verify entry point over the golden fixture: whole, scoped to some periods, and tampered.

use serde_json::{Value, json};
use std::path::PathBuf;
use venin_core::wasm::verify_json;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn records(hour1: &str) -> String {
    json!({
        "manifest": read("golden/manifest.json"),
        "exam": read("golden/exam.json"),
        "anchors": read("golden/anchors.json"),
        "head": read("golden/head.json"),
        "hours": {"0": read("golden/hours/0.json"), "1": hour1},
    })
    .to_string()
}

fn library() -> String {
    json!({
        "strains": read("library/strains.jsonl"),
        "tasks": read("library/tasks.jsonl"),
        "genes": read("library/genes.json"),
        "rows": read("library/rows.json"),
        "names": read("library/voc-names.txt"),
    })
    .to_string()
}

fn run(hour1: &str, bundles: &Value) -> Value {
    let out = verify_json(
        &records(hour1),
        &bundles.to_string(),
        &read("golden/chain.json"),
        &library(),
    );
    serde_json::from_str(&out).expect("JSON out")
}

#[test]
fn the_browser_verifies_all_or_some_of_the_record() {
    let hour1 = read("golden/hours/1.json");
    let all = run(
        &hour1,
        &json!({"0": read("golden/runs/0.jsonl"), "1": read("golden/runs/1.jsonl")}),
    );
    assert_eq!(all["ok"], true, "{all}");
    assert_eq!(all["report"]["hours"], 2);
    assert_eq!(all["replayed"], json!([0, 1]));
    assert_eq!(all["not_loaded"], json!([]));
    let some = run(&hour1, &json!({"1": read("golden/runs/1.jsonl")}));
    assert_eq!(some["ok"], true, "{some}");
    assert_eq!(
        (some["replayed"].clone(), some["not_loaded"].clone()),
        (json!([1]), json!([0]))
    );
    let lost = run(
        &hour1,
        &json!({"0": null, "1": read("golden/runs/1.jsonl")}),
    );
    assert_eq!(lost["ok"], false);
    assert!(
        lost["report"]["errors"]
            .to_string()
            .contains("runs/0.jsonl.gz: missing")
    );
    let tampered = run(
        &hour1.replacen("\"exam\":false", "\"exam\":true", 1),
        &json!({}),
    );
    assert_eq!(tampered["ok"], false);
    let errors = tampered["report"]["errors"].to_string();
    assert!(errors.contains("hours/1.json"), "{errors}");
}
