//! `library/sources.toml`: the pinned public sources, and fetching them into a cache with their hashes checked.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use venin_core::sha256_hex;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    source: Vec<Source>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    pub commit: String,
    pub files: Vec<Pinned>,
    pub license: String,
    #[serde(rename = "use")]
    pub purpose: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pinned {
    pub path: String,
    pub sha256: String,
}

fn lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A relative path that stays under the directory it is joined to.
fn contained(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

pub fn parse(toml: &str) -> Result<Vec<Source>, String> {
    let file: File = toml::from_str(toml).map_err(|e| format!("sources.toml: {e}"))?;
    let mut ids = BTreeSet::new();
    for s in &file.source {
        let id = &s.id;
        if !ids.insert(id.as_str()) || id == "venin" {
            return Err(format!("sources.toml: {id}: duplicate or reserved id"));
        }
        if !lower_hex(&s.commit, 40) {
            return Err(format!(
                "sources.toml: {id}: commit is not 40 lower-case hex"
            ));
        }
        if s.files.is_empty()
            || [&s.name, &s.license, &s.purpose]
                .iter()
                .any(|v| v.trim().is_empty())
        {
            return Err(format!(
                "sources.toml: {id}: needs files, a name, a license and a use"
            ));
        }
        for f in &s.files {
            if !contained(&f.path) || !lower_hex(&f.sha256, 64) {
                return Err(format!(
                    "sources.toml: {id}: {}: needs a relative path and a 64-hex sha256",
                    f.path
                ));
            }
        }
        s.raw_url(&s.files[0].path)?;
    }
    Ok(file.source)
}

impl Source {
    /// Where one pinned file is downloaded from, at the pinned commit.
    pub fn raw_url(&self, path: &str) -> Result<String, String> {
        let url = self.url.trim_end_matches('/');
        if let Some(repo) = url.strip_prefix("https://github.com/") {
            Ok(format!(
                "https://raw.githubusercontent.com/{repo}/{}/{path}",
                self.commit
            ))
        } else if url.starts_with("https://huggingface.co/datasets/") {
            Ok(format!("{url}/resolve/{}/{path}", self.commit))
        } else {
            Err(format!(
                "sources.toml: {}: no download rule for {url}",
                self.id
            ))
        }
    }
}

/// `<cache>/<source id>/<commit>/<path>`.
pub fn cached(cache: &Path, source: &Source, path: &str) -> PathBuf {
    cache.join(&source.id).join(&source.commit).join(path)
}

pub fn verify(bytes: &[u8], sha256: &str) -> Result<(), String> {
    let got = sha256_hex(bytes);
    if got == sha256 {
        Ok(())
    } else {
        Err(format!("sha256 {got}, pinned {sha256}"))
    }
}

/// Downloads every pinned file not already in the cache with the pinned hash. Files land under a temporary name and
/// are renamed only after their hash matches.
pub fn fetch(sources: &[Source], cache: &Path) -> Result<(), String> {
    for source in sources {
        for pinned in &source.files {
            let target = cached(cache, source, &pinned.path);
            if fs::read(&target).is_ok_and(|b| verify(&b, &pinned.sha256).is_ok()) {
                continue;
            }
            let url = source.raw_url(&pinned.path)?;
            let dir = target.parent().expect("a cached path has a parent");
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let mut part = target.clone().into_os_string();
            part.push(".part");
            let part = PathBuf::from(part);
            eprintln!("fetching {url}");
            let status = Command::new("curl")
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .args(["-q", "--fail", "--silent", "--show-error", "--location"])
                .args([
                    "--proto",
                    "=https",
                    "--proto-redir",
                    "=https",
                    "--retry",
                    "3",
                ])
                .arg("--output")
                .arg(&part)
                .arg(&url)
                .status()
                .map_err(|e| format!("curl: {e}"))?;
            if !status.success() {
                return Err(format!("{url}: curl exited with {status}"));
            }
            let bytes = fs::read(&part).map_err(|e| format!("{}: {e}", part.display()))?;
            if let Err(e) = verify(&bytes, &pinned.sha256) {
                let _ = fs::remove_file(&part);
                return Err(format!("{url}: {e}"));
            }
            fs::rename(&part, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// The bytes of one pinned file from the cache, hash checked again.
pub fn load(sources: &[Source], cache: &Path, id: &str, path: &str) -> Result<Vec<u8>, String> {
    let source = sources
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("sources.toml has no source {id}"))?;
    let pinned = source
        .files
        .iter()
        .find(|f| f.path == path)
        .ok_or_else(|| format!("sources.toml: {id} does not pin {path}"))?;
    let file = cached(cache, source, path);
    let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    verify(&bytes, &pinned.sha256).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn toml(url: &str, sha: &str) -> String {
        format!(
            "[[source]]\nid = \"demo\"\nname = \"Demo\"\nurl = \"{url}\"\n\
             commit = \"{}\"\nfiles = [{{ path = \"data/a.jsonl\", sha256 = \"{sha}\" }}]\n\
             license = \"MIT\"\nuse = \"tests\"\n",
            "0".repeat(40)
        )
    }

    #[test]
    fn parses_the_repo_file() {
        let sources = parse(include_str!("../../../../library/sources.toml")).unwrap();
        assert!(sources.iter().any(|s| s.id == "llmail_inject"));
        assert!(sources.iter().all(|s| s.raw_url(&s.files[0].path).is_ok()));
    }

    #[test]
    fn rejects_bad_pins() {
        let sha = "a".repeat(64);
        assert!(parse(&toml("https://github.com/o/r", &sha)).is_ok());
        assert!(parse(&toml("https://github.com/o/r", "abc")).is_err());
        assert!(parse(&toml("https://example.com/o/r", &sha)).is_err());
        let traversal = toml("https://github.com/o/r", &sha).replace("data/a.jsonl", "../a");
        assert!(parse(&traversal).is_err());
        let twice = toml("https://github.com/o/r", &sha).repeat(2);
        assert!(parse(&twice).is_err());
    }

    #[test]
    fn download_urls_pin_the_commit() {
        let sha = "a".repeat(64);
        let zero = "0".repeat(40);
        let gh = &parse(&toml("https://github.com/o/r", &sha)).unwrap()[0];
        assert_eq!(
            gh.raw_url("data/a.jsonl").unwrap(),
            format!("https://raw.githubusercontent.com/o/r/{zero}/data/a.jsonl")
        );
        let hf = &parse(&toml("https://huggingface.co/datasets/o/r", &sha)).unwrap()[0];
        assert_eq!(
            hf.raw_url("data/a.jsonl").unwrap(),
            format!("https://huggingface.co/datasets/o/r/resolve/{zero}/data/a.jsonl")
        );
    }

    #[test]
    fn cached_files_are_used_and_checked() {
        let cache = std::env::temp_dir().join(format!("venin-sources-{}", std::process::id()));
        let body = b"{\"a\":1}\n";
        let sources = parse(&toml("https://github.com/o/r", &sha256_hex(body))).unwrap();
        let file = cached(&cache, &sources[0], "data/a.jsonl");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, body).unwrap();
        fetch(&sources, &cache).unwrap();
        assert_eq!(
            load(&sources, &cache, "demo", "data/a.jsonl").unwrap(),
            body
        );
        fs::write(&file, b"changed").unwrap();
        assert!(load(&sources, &cache, "demo", "data/a.jsonl").is_err());
        assert!(load(&sources, &cache, "demo", "data/b.jsonl").is_err());
        fs::remove_dir_all(&cache).unwrap();
    }
}
