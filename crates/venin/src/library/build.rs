//! `venin library build`: fetch the pinned sources, derive strains, and rewrite `strains.jsonl` with the hand-written
//! lines first, unchanged.

use super::{calibrate, derive, sources};
use std::fs;
use std::path::{Path, PathBuf};
use venin_core::json;
use venin_core::strain::{Strain, parse_strains};

// Every candidate a source yields: calibration, not a cap, decides which strains stay.
const LLMAIL_TAKE: usize = usize::MAX;
const INJECAGENT_TAKE: usize = usize::MAX;
const ASB_TAKE: usize = usize::MAX;

pub fn run(library: &Path, cache: Option<PathBuf>) -> Result<(), String> {
    let read = |name: &str| {
        let path = library.join(name);
        fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let sources = sources::parse(&read("sources.toml")?)?;
    let cache = match cache {
        Some(dir) => dir,
        None => default_cache()?,
    };
    fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    outside_repo(&cache, library)?;
    sources::fetch(&sources, &cache)?;

    let text = |id: &str, path: &str| {
        String::from_utf8(sources::load(&sources, &cache, id, path)?)
            .map_err(|e| format!("{id} {path}: {e}"))
    };
    let llmail = derive::llmail(&sources::load(
        &sources,
        &cache,
        "llmail_inject",
        derive::LLMAIL_FILE,
    )?)?;
    let mut injecagent = Vec::new();
    for file in derive::INJECAGENT_FILES {
        injecagent.extend(derive::injecagent(file, &text("injecagent", file)?)?);
    }
    let asb = derive::asb(&text("asb", derive::ASB_FILE)?)?;
    for (id, pool) in [
        ("llmail_inject", &llmail),
        ("injecagent", &injecagent),
        ("asb", &asb),
    ] {
        println!("{id}: {} candidates", pool.len());
    }
    let derived = derive::number(vec![
        derive::select(llmail, LLMAIL_TAKE),
        derive::select(injecagent, INJECAGENT_TAKE),
        derive::select(asb, ASB_TAKE),
    ])?;
    let gone = calibrate::dropped_ids(library)?;
    let derived: Vec<Strain> = derived
        .into_iter()
        .filter(|s| !gone.contains(&s.id))
        .collect();
    if !gone.is_empty() {
        println!("{}: {} strains left out", calibrate::FILE, gone.len());
    }

    let path = library.join("strains.jsonl");
    let out = merge(&read("strains.jsonl")?, &derived)?;
    let total = parse_strains(&out)?.len();
    fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "{}: {} hand-written and {} derived strains",
        path.display(),
        total - derived.len(),
        derived.len()
    );
    Ok(())
}

fn default_cache() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set; pass --cache")?;
    Ok(PathBuf::from(home).join(".cache/venin/sources"))
}

/// The source files are not ours to commit, so the cache may not sit inside the repository that holds `library`.
fn outside_repo(cache: &Path, library: &Path) -> Result<(), String> {
    let canonical = |p: &Path| {
        p.canonicalize()
            .map_err(|e| format!("{}: {e}", p.display()))
    };
    let cache = canonical(cache)?;
    let library = canonical(library)?;
    let repo = library.parent().unwrap_or(&library);
    if cache.starts_with(repo) {
        return Err(format!(
            "the cache {} is inside {}; choose a directory outside the repository",
            cache.display(),
            repo.display()
        ));
    }
    Ok(())
}

/// The hand-written lines of `existing` (families below `FIRST_FAMILY`) byte for byte, then the derived strains from
/// the ASCII writer.
pub fn merge(existing: &str, derived: &[Strain]) -> Result<String, String> {
    let mut out = String::new();
    for (i, line) in existing
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let strain: Strain =
            serde_json::from_str(line).map_err(|e| format!("strains line {}: {e}", i + 1))?;
        if strain.family < derive::FIRST_FAMILY {
            out.push_str(line);
            out.push('\n');
        }
    }
    for strain in derived {
        out.push_str(&json::to_string(strain).map_err(|e| format!("{}: {e}", strain.id))?);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAND: &str = include_str!("../../../../library/strains.jsonl");

    #[test]
    fn merge_keeps_hand_lines_and_replaces_derived_ones() {
        let hand: Vec<&str> = HAND
            .lines()
            .filter(|l| l.contains("\"source\": \"venin\""))
            .collect();
        let mut derived: Strain = serde_json::from_str(hand[0]).unwrap();
        derived.id = "EYE-101.0".into();
        derived.family = 101;
        derived.source = "asb".into();
        derived.source_ref = "data/all_attack_tools.jsonl line 1".into();
        let old = format!("{}\n{}\n", hand[0], json::to_string(&derived).unwrap());
        derived.template.push_str(" Changed.");
        let out = merge(&old, std::slice::from_ref(&derived)).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], hand[0]);
        assert!(lines[1].starts_with("{\"id\":\"EYE-101.0\""));
        assert!(lines[1].contains(" Changed."));
        assert_eq!(merge(&out, std::slice::from_ref(&derived)).unwrap(), out);
    }

    #[test]
    fn the_cache_stays_out_of_the_repository() {
        let root = std::env::temp_dir().join(format!("venin-build-{}", std::process::id()));
        let library = root.join("repo/library");
        let inside = root.join("repo/cache");
        let outside = root.join("cache");
        for dir in [&library, &inside, &outside] {
            fs::create_dir_all(dir).unwrap();
        }
        assert!(outside_repo(&inside, &library).is_err());
        assert!(outside_repo(&outside, &library).is_ok());
        fs::remove_dir_all(&root).unwrap();
    }
}
