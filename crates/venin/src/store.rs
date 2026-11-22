//! The data dir (the served record) and the private state dir. Record files are written by the core's ASCII writer,
//! each through a temporary file and a rename so a reader never sees half a file.

use crate::draw::Saved;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use venin_core::sha256_hex;
use venin_core::verify::{AnchorEntry, AnchorKind, Bundle, LibraryFiles, Takedown};

pub const LIBRARY_FILES: [&str; 5] = [
    "strains.jsonl",
    "tasks.jsonl",
    "genes.json",
    "rows.json",
    "voc-names.txt",
];

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("a file needs a directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, String> {
    venin_core::json::to_vec(value).map_err(|e| e.to_string())
}

pub fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|e| format!("gunzip: {e}"))?;
    Ok(out)
}

pub struct Store {
    pub data: PathBuf,
    pub state: PathBuf,
}

impl Store {
    pub fn new(data: PathBuf, state: PathBuf) -> Self {
        Store { data, state }
    }

    pub fn read(&self, rel: &str) -> Result<Option<Vec<u8>>, String> {
        read(&self.data.join(rel))
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) -> Result<(), String> {
        write_atomic(&self.data.join(rel), bytes)
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.data.join(rel).exists()
    }

    pub fn state_json<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, String> {
        let path = self.state.join(name);
        read(&path)?
            .map(|b| serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", path.display())))
            .transpose()
    }

    pub fn write_state_json<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        write_atomic(&self.state.join(name), &encode(value)?)
    }

    pub fn remove_state(&self, name: &str) -> Result<(), String> {
        let path = self.state.join(name);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", path.display()))
            }
            _ => Ok(()),
        }
    }

    pub fn state_exists(&self, name: &str) -> bool {
        self.state.join(name).exists()
    }

    /// Writes `runs/<n>.jsonl.gz`, or removes a stale one when the period published no lines.
    pub fn write_runs(&self, n: u64, text: &str) -> Result<(), String> {
        let rel = format!("runs/{n}.jsonl.gz");
        if text.is_empty() {
            let path = self.data.join(&rel);
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            return Ok(());
        }
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        self.write(&rel, &gz.finish().map_err(|e| e.to_string())?)
    }

    pub fn read_runs(&self, n: u64) -> Result<Option<Vec<u8>>, String> {
        self.read(&format!("runs/{n}.jsonl.gz"))?
            .map(|b| gunzip(&b))
            .transpose()
    }

    /// Every hour file, by period.
    pub fn hours(&self) -> Result<BTreeMap<u64, Vec<u8>>, String> {
        self.numbered("hours")
    }

    /// Every `<n>.json` in the data dir's `dir`, by `n`.
    pub fn numbered(&self, dir: &str) -> Result<BTreeMap<u64, Vec<u8>>, String> {
        let dir = self.data.join(dir);
        let mut out = BTreeMap::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(out);
        };
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let name = entry.file_name();
            let Some(n) = name
                .to_str()
                .and_then(|s| s.strip_suffix(".json"))
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            let bytes = std::fs::read(entry.path()).map_err(|e| format!("{name:?}: {e}"))?;
            out.insert(n, bytes);
        }
        Ok(out)
    }

    pub fn anchors(&self) -> Result<Vec<AnchorEntry>, String> {
        match self.read("anchors.json")? {
            None => Ok(Vec::new()),
            Some(b) => serde_json::from_slice(&b).map_err(|e| format!("anchors.json: {e}")),
        }
    }

    pub fn push_anchor(&self, entry: AnchorEntry) -> Result<(), String> {
        let mut anchors = self.anchors()?;
        anchors.push(entry);
        self.write("anchors.json", &encode(&anchors)?)
    }

    /// Files written but not yet anchored, in the order they must be anchored.
    pub fn due(&self) -> Result<Vec<(AnchorKind, u64, String)>, String> {
        let anchored: Vec<(AnchorKind, u64)> =
            self.anchors()?.iter().map(|a| (a.kind, a.n)).collect();
        let mut out = Vec::new();
        for (kind, rel) in [
            (AnchorKind::Manifest, "manifest.json"),
            (AnchorKind::Exam, "exam.json"),
        ] {
            if let Some(b) = self.read(rel)?
                && !anchored.contains(&(kind, 0))
            {
                out.push((kind, 0, sha256_hex(&b)));
            }
        }
        for (n, bytes) in self.hours()? {
            if !anchored.contains(&(AnchorKind::Hour, n)) {
                out.push((AnchorKind::Hour, n, sha256_hex(&bytes)));
            }
        }
        Ok(out)
    }

    pub fn write_draw(&self, name: &str, saved: &Saved) -> Result<(), String> {
        self.write(&format!("draws/{name}.json"), &encode(saved)?)
    }

    pub fn library(&self) -> Result<LibraryFiles, String> {
        read_library(&self.data.join("library"))
    }

    /// The published record as verify reads it.
    pub fn bundle(&self) -> Result<Bundle, String> {
        let need = |rel: &str| {
            self.read(rel)?
                .ok_or_else(|| format!("{}: missing", self.data.join(rel).display()))
        };
        let hours = self.hours()?;
        let mut runs = BTreeMap::new();
        for n in hours.keys() {
            if let Some(b) = self.read_runs(*n)? {
                runs.insert(*n, b);
            }
        }
        let takedowns: Vec<Takedown> = match self.read("takedowns.json")? {
            None => Vec::new(),
            Some(b) => serde_json::from_slice(&b).map_err(|e| format!("takedowns.json: {e}"))?,
        };
        Ok(Bundle {
            manifest: need("manifest.json")?,
            exam: need("exam.json")?,
            hours,
            runs,
            cards: self.numbered("cards")?,
            anchors: need("anchors.json")?,
            head: need("head.json")?,
            takedowns,
        })
    }
}

pub fn read_library(dir: &Path) -> Result<LibraryFiles, String> {
    let read = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .map_err(|e| format!("{}: {e}", dir.join(name).display()))
    };
    Ok(LibraryFiles {
        strains: read(LIBRARY_FILES[0])?,
        tasks: read(LIBRARY_FILES[1])?,
        genes: read(LIBRARY_FILES[2])?,
        rows: read(LIBRARY_FILES[3])?,
        names: read(LIBRARY_FILES[4])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::tests::temp;

    #[test]
    fn due_lists_unanchored_files_in_anchor_order() {
        let dir = temp("store");
        let store = Store::new(dir.join("data"), dir.join("state"));
        assert!(store.due().expect("due").is_empty());
        store.write("manifest.json", b"{}").expect("writes");
        store.write("exam.json", b"[]").expect("writes");
        store.write("hours/10.json", b"10").expect("writes");
        store.write("hours/9.json", b"9").expect("writes");
        store.write("hours/x.json", b"x").expect("writes");
        let kinds: Vec<(AnchorKind, u64)> = store
            .due()
            .expect("due")
            .into_iter()
            .map(|(k, n, _)| (k, n))
            .collect();
        assert_eq!(
            kinds,
            [
                (AnchorKind::Manifest, 0),
                (AnchorKind::Exam, 0),
                (AnchorKind::Hour, 9),
                (AnchorKind::Hour, 10)
            ]
        );
        store
            .push_anchor(AnchorEntry {
                kind: AnchorKind::Manifest,
                n: 0,
                tx: "0x01".into(),
                block: 1,
            })
            .expect("pushes");
        assert_eq!(store.due().expect("due")[0].0, AnchorKind::Exam);
        assert_eq!(store.due().expect("due")[0].2, sha256_hex(b"[]"));
    }

    #[test]
    fn runs_round_trip_through_gzip_and_empty_runs_remove_the_file() {
        let dir = temp("runs");
        let store = Store::new(dir.join("data"), dir.join("state"));
        store.write_runs(3, "a\nb\n").expect("writes");
        assert_eq!(
            store.read_runs(3).expect("reads").expect("present"),
            b"a\nb\n"
        );
        store.write_runs(3, "").expect("removes");
        assert!(store.read_runs(3).expect("reads").is_none());
        store
            .write_state_json("x.json", &vec![1u64, 2])
            .expect("writes");
        assert_eq!(
            store.state_json::<Vec<u64>>("x.json").expect("reads"),
            Some(vec![1, 2])
        );
        store.remove_state("x.json").expect("removes");
        store.remove_state("x.json").expect("idempotent");
        assert!(!store.state_exists("x.json"));
    }
}
