//! The record's public copy: every file of the data folder, sent to an S3-compatible bucket (Cloudflare R2) and signed
//! with AWS Signature Version 4. A file the record hashes is written once and never replaced (`If-None-Match: *`);
//! when the bucket already holds it, its recorded SHA-256 must match. The files that change every hour (the anchors,
//! the status, the head, the takedowns and the cards) are replaced, and a file takedowns.json lists that is gone from
//! the folder is deleted. The pointers (anchors, status, head) are read before the folder is listed and sent last, so
//! the bucket never holds a head before the hour it names, and a failure ends the pass before the head moves.

use crate::config::Config;
use crate::net::{Net, Reply, pct};
use crate::runner::log;
use crate::store::Store;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use venin_core::{hex, sha256_hex};

/// Where the bucket's endpoint (`https://<account id>.r2.cloudflarestorage.com`) and keys come from, so no account id
/// or key sits in the repository.
pub const ENDPOINT_ENV: &str = "VENIN_R2_ENDPOINT";
pub const KEY_ID_ENV: &str = "VENIN_R2_KEY_ID";
pub const SECRET_ENV: &str = "VENIN_R2_SECRET";

/// The files a pass sends last, in this order.
const POINTERS: [&str; 3] = ["anchors.json", "status.json", "head.json"];
/// What the bucket already holds, by path, kept in the runner's state folder.
const STATE: &str = "upload.json";
/// How a deleted file is remembered in the state.
const DELETED: &str = "deleted";
/// The state is saved every this many files, and at the end of a pass; it is only a cache.
const SAVE_EVERY: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(120);

pub struct Bucket {
    /// The S3 endpoint, without a trailing slash or a path.
    pub endpoint: String,
    pub name: String,
    pub region: String,
    pub key_id: String,
    pub secret: String,
}

/// What a bucket holds, as far as earlier passes know. Another bucket starts from nothing.
#[derive(Serialize, Deserialize, Default)]
struct State {
    bucket: String,
    files: BTreeMap<String, Sent>,
}

/// A file as it was sent; its size and modification time (in microseconds, which the store's JSON holds exactly) let
/// a later pass skip it unread.
#[derive(Serialize, Deserialize)]
struct Sent {
    size: u64,
    modified_us: u64,
    sha256: String,
}

pub fn run(config: &Config, store: &Store) -> Result<(), String> {
    let upload = config
        .upload
        .as_ref()
        .ok_or("the configuration has no [upload] section")?;
    let env = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("{name} is not set"))
    };
    let bucket = Bucket {
        endpoint: endpoint(&env(ENDPOINT_ENV)?)?,
        name: upload.bucket.clone(),
        region: upload.region.clone(),
        key_id: env(KEY_ID_ENV)?,
        secret: env(SECRET_ENV)?,
    };
    let net = Net::new(std::slice::from_ref(&bucket.endpoint))?;
    let sent = pass(&net, &bucket, store, &config.data_dir)?;
    if sent > 0 {
        log(&format!("upload: {sent} file(s) written"));
    }
    Ok(())
}

/// The account endpoint without a trailing slash; one naming a bucket or a path would be signed for a path it does not
/// reach.
fn endpoint(url: &str) -> Result<String, String> {
    let u = url::Url::parse(url).map_err(|e| format!("{ENDPOINT_ENV}: {e}"))?;
    if !matches!(u.path(), "" | "/") || u.query().is_some() {
        return Err(format!(
            "{ENDPOINT_ENV} must be the account endpoint alone, without a bucket or a path"
        ));
    }
    Ok(url.trim_end_matches('/').to_string())
}

/// One pass over the data folder; returns how many files it wrote.
fn pass(net: &Net, bucket: &Bucket, store: &Store, data: &Path) -> Result<usize, String> {
    let at = format!("{}/{}", bucket.endpoint, bucket.name);
    let mut state = store
        .state_json::<State>(STATE)
        .ok()
        .flatten()
        .filter(|s| s.bucket == at)
        .unwrap_or_default();
    state.bucket = at;
    let result = send_all(net, bucket, store, data, &mut state);
    let saved = store.write_state_json(STATE, &state);
    let sent = result?;
    saved?;
    Ok(sent)
}

fn send_all(
    net: &Net,
    bucket: &Bucket,
    store: &Store,
    data: &Path,
    state: &mut State,
) -> Result<usize, String> {
    // The runner writes an hour before the head that names it, so pointers read before the listing name only hours the
    // listing holds.
    let mut pointers = Vec::new();
    for path in POINTERS {
        if let Some(file) = read(&data.join(path))? {
            pointers.push((path, file));
        }
    }
    let mut rest = files(data)?;
    rest.retain(|p| !POINTERS.contains(&p.as_str()));
    rest.sort();
    let mut sent = 0;
    for (i, path) in rest.iter().enumerate() {
        let full = data.join(path);
        let meta = match std::fs::metadata(&full) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", full.display())),
        };
        let modified_us = modified_us(&meta);
        if state
            .files
            .get(path)
            .is_some_and(|s| s.size == meta.len() && s.modified_us == modified_us)
        {
            continue;
        }
        if let Some(file) = read(&full)? {
            sent += usize::from(commit(net, bucket, state, path, file)?);
        }
        if i % SAVE_EVERY == SAVE_EVERY - 1 {
            store.write_state_json(STATE, state)?;
        }
    }
    for (path, file) in pointers {
        sent += usize::from(commit(net, bucket, state, path, file)?);
    }
    // After the head, so a takedown list that cannot be read never holds the record back.
    takedowns(net, bucket, state, data)?;
    Ok(sent)
}

/// A file read whole, with the modification time taken before the read.
struct File {
    modified_us: u64,
    bytes: Vec<u8>,
}

fn modified_us(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// The file, or None when it is gone.
fn read(full: &Path) -> Result<Option<File>, String> {
    let gone = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    let meta = match std::fs::metadata(full) {
        Ok(meta) => meta,
        Err(e) if gone(&e) => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", full.display())),
    };
    match std::fs::read(full) {
        Ok(bytes) => Ok(Some(File {
            modified_us: modified_us(&meta),
            bytes,
        })),
        Err(e) if gone(&e) => Ok(None),
        Err(e) => Err(format!("{}: {e}", full.display())),
    }
}

/// Sends a file unless the bucket has these bytes already, and remembers it; true when it was written.
fn commit(
    net: &Net,
    bucket: &Bucket,
    state: &mut State,
    path: &str,
    file: File,
) -> Result<bool, String> {
    let sha256 = sha256_hex(&file.bytes);
    let size = file.bytes.len() as u64;
    let wrote = if state.files.get(path).is_none_or(|s| s.sha256 != sha256) {
        put(net, bucket, path, file.bytes, &sha256)?
    } else {
        false
    };
    state.files.insert(
        path.to_string(),
        Sent {
            size,
            modified_us: file.modified_us,
            sha256,
        },
    );
    Ok(wrote)
}

/// Every file under `dir`, as a path with `/`, leaving out hidden files and the store's temporary files.
fn files(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let entries =
            std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.ends_with(".tmp") {
                continue;
            }
            let path = format!("{prefix}{name}");
            if entry
                .file_type()
                .map_err(|e| format!("{path}: {e}"))?
                .is_dir()
            {
                folders.push((entry.path(), format!("{path}/")));
            } else {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Each path takedowns.json lists that is gone from the data folder is deleted from the bucket, once. Copies cached on
/// the way (served as immutable) still need a purge in Cloudflare.
fn takedowns(net: &Net, bucket: &Bucket, state: &mut State, data: &Path) -> Result<(), String> {
    let Some(file) = read(&data.join("takedowns.json"))? else {
        return Ok(());
    };
    let list: Value =
        serde_json::from_slice(&file.bytes).map_err(|e| format!("takedowns.json: {e}"))?;
    // Anything but an array lists nothing, as the site reads it.
    let paths = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["path"].as_str());
    for path in paths {
        // A path with an empty, `.` or `..` segment names no file of the record, and would be signed for one key and
        // sent for another.
        if path.split('/').any(|s| matches!(s, "" | "." | "..")) {
            return Err(format!(
                "takedowns.json: {path:?} is not a path of the record"
            ));
        }
        if data.join(path).exists() || state.files.get(path).is_some_and(|s| s.sha256 == DELETED) {
            continue;
        }
        let reply = bucket.send(
            net,
            "DELETE",
            path,
            Vec::new(),
            Vec::new(),
            &sha256_hex(b""),
        )?;
        if !matches!(reply.status, 200 | 204 | 404) {
            return Err(format!("{path}: DELETE answered HTTP {}", reply.status));
        }
        state.files.insert(
            path.to_string(),
            Sent {
                size: 0,
                modified_us: 0,
                sha256: DELETED.into(),
            },
        );
        log(&format!(
            "upload: {path} is deleted from the bucket; purge its cached copy in Cloudflare"
        ));
    }
    Ok(())
}

/// The files written again after their first version: the pointers, the takedown list and the cards (rewritten when a
/// caption changes). Every other file is part of the record and written once.
fn replaced(path: &str) -> bool {
    POINTERS.contains(&path) || path == "takedowns.json" || path.starts_with("cards/")
}

fn cache_control(path: &str) -> &'static str {
    if path.starts_with("cards/") {
        "public, max-age=300"
    } else if replaced(path) {
        "public, max-age=60"
    } else {
        "public, max-age=31536000, immutable"
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("json") => "application/json",
        Some("jsonl") => "application/x-ndjson",
        Some("gz") => "application/gzip",
        Some("png") => "image/png",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Sends one file; true when it was written. A file the record hashes goes up only where the bucket has none, so a copy
/// already there is checked by the SHA-256 it was sent with.
fn put(
    net: &Net,
    bucket: &Bucket,
    path: &str,
    bytes: Vec<u8>,
    sha256: &str,
) -> Result<bool, String> {
    let once = !replaced(path);
    let mut headers = vec![
        ("cache-control".to_string(), cache_control(path).to_string()),
        ("content-type".to_string(), content_type(path).to_string()),
        ("x-amz-meta-sha256".to_string(), sha256.to_string()),
    ];
    if once {
        headers.push(("if-none-match".to_string(), "*".to_string()));
    }
    let reply = bucket.send(net, "PUT", path, headers, bytes, sha256)?;
    match reply.status {
        200 => Ok(true),
        412 if once => {
            let held = bucket.send(net, "HEAD", path, Vec::new(), Vec::new(), &sha256_hex(b""))?;
            match held.header("x-amz-meta-sha256") {
                Some(h) if held.status == 200 && h == sha256 => Ok(false),
                None if held.status == 200 => Err(format!(
                    "{path}: the bucket holds a copy without a recorded SHA-256 (sent by another tool?); compare it by \
                     hand, as a file the record hashes is never replaced"
                )),
                _ => Err(format!(
                    "{path}: the bucket already holds a different copy, and a file the record hashes is never replaced"
                )),
            }
        }
        status => Err(format!(
            "{path}: PUT answered HTTP {status}: {}",
            reply.body.chars().take(200).collect::<String>()
        )),
    }
}

impl Bucket {
    /// Signs and sends one request for the object at `key`, dated now; `headers` are the request's own, names in lower
    /// case.
    fn send(
        &self,
        net: &Net,
        method: &str,
        key: &str,
        mut headers: Vec<(String, String)>,
        body: Vec<u8>,
        payload_sha256: &str,
    ) -> Result<Reply, String> {
        let uri = format!("/{}/{}", pct(&self.name, false), pct(key, true));
        let url = format!("{}{uri}", self.endpoint);
        let amz_date = venin_core::time::rfc3339(crate::now())
            .ok_or("the clock is out of range")?
            .replace(['-', ':'], "");
        headers.push((
            "x-amz-content-sha256".to_string(),
            payload_sha256.to_string(),
        ));
        headers.push(("x-amz-date".to_string(), amz_date.clone()));
        let mut signed = headers.clone();
        signed.push(("host".to_string(), host(&url)?));
        let (names, signature) = signature(
            &self.secret,
            &self.region,
            &amz_date,
            method,
            &uri,
            &signed,
            payload_sha256,
        );
        headers.push((
            "authorization".to_string(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{}/{}/s3/aws4_request, SignedHeaders={names}, Signature={signature}",
                self.key_id,
                &amz_date[..8],
                self.region
            ),
        ));
        net.request(method, &url, &headers, body, TIMEOUT)
            .map_err(|e| format!("{method} {key}: {e}"))
    }
}

/// The Host header a client sends for `url`: its host, and its port when the URL names one.
fn host(url: &str) -> Result<String, String> {
    let u = url::Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
    let host = u.host_str().ok_or_else(|| format!("{url}: no host"))?;
    Ok(match u.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// AWS Signature Version 4 for S3: the signed header names and the signature of a request without a query string.
/// `headers` holds every header to sign, `host` among them.
fn signature(
    secret: &str,
    region: &str,
    amz_date: &str,
    method: &str,
    uri: &str,
    headers: &[(String, String)],
    payload_sha256: &str,
) -> (String, String) {
    let mut headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    headers.sort();
    let canonical: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let names = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let request = format!("{method}\n{uri}\n\n{canonical}\n{names}\n{payload_sha256}");
    let date = &amz_date[..8];
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{date}/{region}/s3/aws4_request\n{}",
        sha256_hex(request.as_bytes())
    );
    let mut key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    for part in [region, "s3", "aws4_request"] {
        key = hmac_sha256(&key, part.as_bytes());
    }
    (names, hex(&hmac_sha256(&key, to_sign.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::tests::{Seen, serve_head};
    use std::sync::{Arc, Mutex};

    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// AWS's published Signature Version 4 examples for S3: a ranged GET and a PUT whose key needs encoding.
    #[test]
    fn signatures_match_the_published_examples() {
        let secret = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let date = "20130524T000000Z";
        let get = pairs(&[
            ("host", "examplebucket.s3.amazonaws.com"),
            ("range", "bytes=0-9"),
            ("x-amz-content-sha256", EMPTY),
            ("x-amz-date", date),
        ]);
        assert_eq!(
            signature(secret, "us-east-1", date, "GET", "/test.txt", &get, EMPTY),
            (
                "host;range;x-amz-content-sha256;x-amz-date".to_string(),
                "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41".to_string()
            )
        );
        let body = sha256_hex(b"Welcome to Amazon S3.");
        assert_eq!(
            body,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        let put = pairs(&[
            ("date", "Fri, 24 May 2013 00:00:00 GMT"),
            ("host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-content-sha256", &body),
            ("x-amz-date", date),
            ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
        ]);
        let uri = format!("/{}", pct("test$file.text", true));
        assert_eq!(uri, "/test%24file.text");
        assert_eq!(
            signature(secret, "us-east-1", date, "PUT", &uri, &put, &body).1,
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        );
    }

    /// The objects of the in-memory bucket: path -> the sha256 each was sent with (empty when sent without one).
    type Objects = Arc<Mutex<BTreeMap<String, String>>>;

    /// A bucket in memory that answers a conditional PUT, a HEAD and a DELETE the way R2 does.
    fn in_memory(name: &str) -> (Bucket, Net, Seen, Objects) {
        let objects: Objects = Arc::default();
        let held = objects.clone();
        let (url, seen) = serve_head(move |method, path, head, _| {
            let header = |name: &str| {
                head.lines()
                    .find_map(|l| l.strip_prefix(&format!("{name}:")))
                    .map(|v| v.trim().to_string())
            };
            let mut objects = held.lock().expect("lock");
            match method {
                "PUT"
                    if header("if-none-match").as_deref() == Some("*")
                        && objects.contains_key(path) =>
                {
                    (412, Vec::new(), String::new())
                }
                "PUT" => {
                    objects.insert(
                        path.to_string(),
                        header("x-amz-meta-sha256").unwrap_or_default(),
                    );
                    (200, Vec::new(), String::new())
                }
                "HEAD" => match objects.get(path) {
                    Some(sha) if sha.is_empty() => (200, Vec::new(), String::new()),
                    Some(sha) => (
                        200,
                        vec![("x-amz-meta-sha256".into(), sha.clone())],
                        String::new(),
                    ),
                    None => (404, Vec::new(), String::new()),
                },
                "DELETE" => {
                    objects.remove(path);
                    (204, Vec::new(), String::new())
                }
                _ => (400, Vec::new(), String::new()),
            }
        });
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        let bucket = Bucket {
            endpoint: url,
            name: name.into(),
            region: "auto".into(),
            key_id: "KEYID".into(),
            secret: "secret".into(),
        };
        (bucket, net, seen, objects)
    }

    fn folder(name: &str, files: &[(&str, &str)]) -> (std::path::PathBuf, Store) {
        let dir = crate::anchor::tests::temp(name);
        let data = dir.join("data");
        for (path, text) in files {
            let full = data.join(path);
            std::fs::create_dir_all(full.parent().expect("parent")).expect("folder");
            std::fs::write(full, text).expect("write");
        }
        let store = Store::new(data.clone(), dir.join("state"));
        (data, store)
    }

    fn requests(seen: &Seen) -> Vec<(String, String)> {
        seen.lock()
            .expect("lock")
            .iter()
            .map(|(method, path, _, _)| (method.clone(), path.clone()))
            .collect()
    }

    fn put(path: &str) -> (String, String) {
        ("PUT".to_string(), format!("/venin-record/{path}"))
    }

    #[test]
    fn a_pass_sends_the_head_last_and_each_change_once() {
        let (data, store) = folder(
            "upload-order",
            &[
                ("head.json", r#"{"n":0}"#),
                ("anchors.json", "[]"),
                ("status.json", "{}"),
                ("manifest.json", "{}"),
                ("hours/0.json", "{}"),
                ("runs/0.jsonl.gz", "gz"),
                ("cards/0.json", "{}"),
                (".hidden", "x"),
                ("hours/1.json.tmp", "half"),
            ],
        );
        let (bucket, net, seen, _) = in_memory("venin-record");
        assert_eq!(pass(&net, &bucket, &store, &data).expect("pass"), 7);
        assert_eq!(
            requests(&seen),
            [
                "cards/0.json",
                "hours/0.json",
                "manifest.json",
                "runs/0.jsonl.gz",
                "anchors.json",
                "status.json",
                "head.json"
            ]
            .map(put)
        );
        let date = venin_core::time::rfc3339(crate::now()).expect("date")[..10].replace('-', "");
        for (_, path, _, head) in seen.lock().expect("lock").iter() {
            let once = !(path.contains("/cards/")
                || path.ends_with("anchors.json")
                || path.ends_with("status.json")
                || path.ends_with("head.json"));
            assert_eq!(head.contains("if-none-match: *"), once, "{path}");
            assert!(
                head.contains(&format!(
                    "authorization: aws4-hmac-sha256 credential=keyid/{date}/auto/s3/aws4_request, signedheaders="
                )),
                "{head}"
            );
            let cache = if once {
                "max-age=31536000, immutable"
            } else if path.contains("/cards/") {
                "max-age=300"
            } else {
                "max-age=60"
            };
            assert!(
                head.contains(&format!("cache-control: public, {cache}")),
                "{path}"
            );
        }
        assert_eq!(
            pass(&net, &bucket, &store, &data).expect("pass"),
            0,
            "nothing new"
        );
        std::fs::write(data.join("head.json"), r#"{"n":1}"#).expect("write");
        std::fs::write(data.join("hours/1.json"), "{}").expect("write");
        assert_eq!(pass(&net, &bucket, &store, &data).expect("pass"), 2);
        assert_eq!(
            requests(&seen)[7..],
            [put("hours/1.json"), put("head.json")]
        );
    }

    #[test]
    fn a_recorded_file_is_never_replaced_and_a_refusal_holds_the_head_back() {
        let (data, store) = folder(
            "upload-once",
            &[("head.json", r#"{"n":0}"#), ("hours/0.json", "{}")],
        );
        let (bucket, net, seen, objects) = in_memory("venin-record");
        objects
            .lock()
            .expect("lock")
            .insert("/venin-record/hours/0.json".into(), sha256_hex(b"{}"));
        assert_eq!(
            pass(&net, &bucket, &store, &data).expect("the same bytes are already there"),
            1,
            "only the head is written"
        );
        objects
            .lock()
            .expect("lock")
            .insert("/venin-record/hours/1.json".into(), "another copy".into());
        std::fs::write(data.join("hours/1.json"), "{}").expect("write");
        std::fs::write(data.join("head.json"), r#"{"n":1}"#).expect("write");
        let e = pass(&net, &bucket, &store, &data).expect_err("refused");
        assert!(e.contains("never replaced"), "{e}");
        assert_eq!(
            requests(&seen).last(),
            Some(&("HEAD".to_string(), "/venin-record/hours/1.json".to_string())),
            "the head stays where it was"
        );
        assert_eq!(
            objects.lock().expect("lock")["/venin-record/hours/1.json"],
            "another copy"
        );
        objects
            .lock()
            .expect("lock")
            .insert("/venin-record/hours/1.json".into(), String::new());
        let e = pass(&net, &bucket, &store, &data).expect_err("refused");
        assert!(e.contains("without a recorded SHA-256"), "{e}");
    }

    #[test]
    fn a_taken_down_file_leaves_the_bucket_once_and_another_bucket_starts_afresh() {
        let (data, store) = folder(
            "upload-takedown",
            &[("head.json", r#"{"n":0}"#), ("runs/0.jsonl.gz", "gz")],
        );
        let (bucket, net, seen, objects) = in_memory("venin-record");
        assert_eq!(pass(&net, &bucket, &store, &data).expect("pass"), 2);
        std::fs::remove_file(data.join("runs/0.jsonl.gz")).expect("remove");
        std::fs::write(
            data.join("takedowns.json"),
            r#"[{"path":"runs/0.jsonl.gz","sha":"00","date_s":1,"reason":"legal"}]"#,
        )
        .expect("write");
        assert_eq!(pass(&net, &bucket, &store, &data).expect("pass"), 1);
        assert_eq!(
            requests(&seen)[2..],
            [
                put("takedowns.json"),
                (
                    "DELETE".to_string(),
                    "/venin-record/runs/0.jsonl.gz".to_string()
                )
            ]
        );
        assert!(
            !objects
                .lock()
                .expect("lock")
                .contains_key("/venin-record/runs/0.jsonl.gz")
        );
        assert_eq!(
            pass(&net, &bucket, &store, &data).expect("pass"),
            0,
            "deleted once"
        );
        let (other, net, seen, _) = in_memory("venin-record-new");
        assert_eq!(
            pass(&net, &other, &store, &data).expect("pass"),
            2,
            "a new bucket gets everything"
        );
        assert_eq!(requests(&seen).len(), 3, "and its own takedown");
        std::fs::write(data.join("takedowns.json"), "{}").expect("write");
        std::fs::write(data.join("head.json"), r#"{"n":1}"#).expect("write");
        assert_eq!(
            pass(&net, &other, &store, &data).expect("an object lists nothing"),
            2
        );
        std::fs::write(
            data.join("takedowns.json"),
            r#"[{"path":"./runs/1.jsonl.gz"}]"#,
        )
        .expect("write");
        std::fs::write(data.join("head.json"), r#"{"n":2}"#).expect("write");
        let e = pass(&net, &other, &store, &data).expect_err("an odd path");
        assert!(e.contains("not a path of the record"), "{e}");
        assert_eq!(
            requests(&seen).last(),
            Some(&("PUT".to_string(), "/venin-record-new/head.json".to_string())),
            "the head went up before the takedowns were read"
        );
    }

    #[test]
    fn types_caching_and_the_endpoint_follow_the_rules() {
        assert_eq!(content_type("runs/3.jsonl.gz"), "application/gzip");
        assert_eq!(
            content_type("library/strains.jsonl"),
            "application/x-ndjson"
        );
        assert_eq!(content_type("cards/3.png"), "image/png");
        assert_eq!(cache_control("head.json"), "public, max-age=60");
        assert_eq!(cache_control("takedowns.json"), "public, max-age=60");
        assert_eq!(
            cache_control("hours/3.json"),
            "public, max-age=31536000, immutable"
        );
        assert!(
            replaced("cards/3.png") && !replaced("draws/3.json") && !replaced("library/genes.json")
        );
        assert_eq!(
            host("https://a.r2.cloudflarestorage.com/b/k").expect("host"),
            "a.r2.cloudflarestorage.com"
        );
        assert_eq!(
            host("http://127.0.0.1:9000/b").expect("host"),
            "127.0.0.1:9000"
        );
        assert_eq!(
            endpoint("https://a.r2.cloudflarestorage.com/").expect("endpoint"),
            "https://a.r2.cloudflarestorage.com"
        );
        assert!(endpoint("https://a.r2.cloudflarestorage.com/venin-record").is_err());
    }
}
