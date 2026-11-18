//! `venin library check`: every library file parsed with the core types, every strain gated and placed in a frame,
//! every source named, and counts by route, carrier, technique, goal and source.

use super::{derive::FIRST_FAMILY, sources};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use venin_core::gene::{parse_genes, parse_rows};
use venin_core::strain::{Strain, parse_strains};
use venin_core::targets::Targets;
use venin_core::world::{self, TaskKind, WorldKind, parse_tasks};

pub struct Files {
    pub sources: String,
    pub strains: String,
    pub genes: String,
    pub rows: String,
    pub tasks: String,
    pub voc: String,
}

pub fn run(library: &Path) -> Result<(), String> {
    let read = |name: &str| {
        let path = library.join(name);
        fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let files = Files {
        sources: read("sources.toml")?,
        strains: read("strains.jsonl")?,
        genes: read("genes.json")?,
        rows: read("rows.json")?,
        tasks: read("tasks.jsonl")?,
        voc: read("voc-names.txt")?,
    };
    match audit(&files) {
        Ok(report) => {
            print!("{report}");
            Ok(())
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("{e}");
            }
            Err(format!(
                "{} problems in {}",
                errors.len(),
                library.display()
            ))
        }
    }
}

fn keep<T>(errors: &mut Vec<String>, result: Result<T, String>) -> Option<T> {
    result.map_err(|e| errors.push(e)).ok()
}

/// The report, or every problem found.
pub fn audit(f: &Files) -> Result<String, Vec<String>> {
    let mut errors = Vec::new();
    let sources = keep(&mut errors, sources::parse(&f.sources));
    let strains = keep(&mut errors, parse_strains(&f.strains)).unwrap_or_default();
    let genes = keep(&mut errors, parse_genes(&f.genes)).unwrap_or_default();
    keep(&mut errors, parse_rows(&f.rows, &genes));
    let tasks = keep(&mut errors, parse_tasks(&f.tasks));

    if let Some(sources) = &sources {
        let ids: BTreeSet<&str> = sources
            .iter()
            .map(|s| s.id.as_str())
            .chain(["venin"])
            .collect();
        let named = strains
            .iter()
            .map(|s| (&s.id, &s.source))
            .chain(tasks.iter().flatten().map(|t| (&t.id, &t.source)));
        for (id, source) in named {
            if !ids.contains(source.as_str()) {
                errors.push(format!("{id}: source {source} is not in sources.toml"));
            }
        }
    }
    for s in &strains {
        if s.source == "venin" && s.family >= FIRST_FAMILY {
            errors.push(format!(
                "{}: families from {FIRST_FAMILY} up are derived from a source, not venin",
                s.id
            ));
        }
    }
    if let Some(tasks) = &tasks {
        let targets = Targets::from_bytes(&[0; 32]);
        for (kind, name) in [(WorldKind::Ward, "ward"), (WorldKind::Priors, "Priors")] {
            let frames: Vec<_> = tasks
                .iter()
                .filter(|t| t.kind == TaskKind::Attack && t.world == kind)
                .collect();
            for s in &strains {
                if !frames
                    .iter()
                    .any(|t| world::build(t, Some(s), &targets, &[0; 16]).is_ok())
                {
                    errors.push(format!("{}: no {name} attack frame can carry it", s.id));
                }
            }
        }
    }
    let voc = voc_names(&f.voc, &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(report(
        &strains,
        genes.len(),
        tasks.map_or(0, |t| t.len()),
        voc,
    ))
}

/// Names are letters, single spaces, hyphens and apostrophes, capitalized, unique; returns how many.
fn voc_names(text: &str, errors: &mut Vec<String>) -> usize {
    let mut seen = BTreeSet::new();
    for (i, name) in text.lines().enumerate() {
        let shaped = name.starts_with(|c: char| c.is_ascii_uppercase())
            && !name.ends_with([' ', '-'])
            && !name.contains("  ")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphabetic() || matches!(b, b' ' | b'-' | b'\''));
        if !shaped || !seen.insert(name.to_ascii_lowercase()) {
            errors.push(format!(
                "voc-names.txt line {}: {name:?} is malformed or repeated",
                i + 1
            ));
        }
    }
    if seen.is_empty() {
        errors.push("voc-names.txt is empty".into());
    }
    seen.len()
}

fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

type Column = (&'static str, fn(&Strain) -> String);

fn report(strains: &[Strain], genes: usize, tasks: usize, voc: usize) -> String {
    let mut out = format!(
        "strains {}, genes {genes}, tasks {tasks}, voc names {voc}\n",
        strains.len()
    );
    let columns: [Column; 5] = [
        ("route", |s| label(&s.route)),
        ("carrier", |s| label(&s.carrier)),
        ("technique", |s| label(&s.technique)),
        ("goal", |s| label(&s.goal)),
        ("source", |s| s.source.clone()),
    ];
    for (name, key) in columns {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for s in strains {
            *counts.entry(key(s)).or_default() += 1;
        }
        let row: Vec<String> = counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        out.push_str(&format!("{name}: {}\n", row.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Files {
        Files {
            sources: include_str!("../../../../library/sources.toml").into(),
            strains: include_str!("../../../../library/strains.jsonl").into(),
            genes: include_str!("../../../../library/genes.json").into(),
            rows: include_str!("../../../../library/rows.json").into(),
            tasks: include_str!("../../../../library/tasks.jsonl").into(),
            voc: include_str!("../../../../library/voc-names.txt").into(),
        }
    }

    #[test]
    fn the_repo_library_passes_and_is_counted() {
        let report = audit(&repo()).unwrap();
        assert!(report.starts_with("strains "));
        for column in ["route: ", "carrier: ", "technique: ", "goal: ", "source: "] {
            assert!(report.contains(column), "{report}");
        }
        assert!(
            report.contains("source: asb ") && report.contains(", venin "),
            "{report}"
        );
    }

    #[test]
    fn problems_are_all_reported() {
        let mut files = repo();
        // The first hand-written line gets an unknown source, and the first EYE line a hand-written id with a derived
        // family number.
        let mut sourced = false;
        let mut renumbered = false;
        files.strains = files
            .strains
            .lines()
            .map(|line| {
                let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
                if !sourced && v["source"] == "venin" {
                    v["source"] = "elsewhere".into();
                    sourced = true;
                } else if !renumbered && v["route"] == "EYE" {
                    v["id"] = "EYE-901.0".into();
                    v["family"] = 901.into();
                    renumbered = true;
                }
                format!("{v}\n")
            })
            .collect();
        files.voc.push_str("Taipan\nlower\n");
        let errors = audit(&files).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("source elsewhere is not in sources.toml"))
        );
        assert!(
            errors
                .iter()
                .any(|e| e.starts_with("EYE-901.0: families from 101 up"))
        );
        assert_eq!(
            errors
                .iter()
                .filter(|e| e.starts_with("voc-names.txt"))
                .count(),
            2
        );
    }

    #[test]
    fn a_strain_no_frame_can_carry_is_an_error() {
        let mut files = repo();
        files.tasks = files
            .tasks
            .lines()
            .filter(|l| !l.contains("\"EYE\""))
            .map(|l| format!("{l}\n"))
            .collect();
        let errors = audit(&files).unwrap_err();
        for world in ["ward", "Priors"] {
            let want = format!("EYE-1.0: no {world} attack frame");
            assert!(errors.iter().any(|e| e.starts_with(&want)), "{want}");
        }
    }

    #[test]
    fn broken_files_are_reported_not_skipped() {
        let mut files = repo();
        files.genes = "[".into();
        files.tasks.push_str("{\"id\": \"X\"}\n");
        let errors = audit(&files).unwrap_err();
        assert!(errors.iter().any(|e| e.starts_with("genes.json")));
        assert!(errors.iter().any(|e| e.starts_with("tasks line")));
    }
}
