//! `venin genesis`: pin the lab (library, code, models, anchor) in manifest.json and anchor it, wait, draw the
//! held-out families from the first draw-chain block after the wait, and write and anchor exam.json. It refuses to
//! touch a data dir that already holds a genesis.

use crate::anchor::{Anchorer, key_address};
use crate::config::Config;
use crate::draw::DrawChain;
use crate::net::Net;
use crate::store::{LIBRARY_FILES, Store, encode, read_library};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use venin_core::lab::{Anchor, Code, Exam, Manifest, Model, RULES_VERSION, exam_families, roster};
use venin_core::sha256_hex;
use venin_core::verify::LibraryFiles;

/// Time kept between the exam anchor and period 0 for anchoring and the draw.
const MARGIN_S: i64 = 300;
/// The draw waits until the chain head is this far past the draw time.
pub const HEAD_LEAD_S: i64 = 30;

/// The first period boundary that leaves room for the exam draw.
pub fn genesis_time(now: i64, period_s: u32, exam_draw_delay_s: u32) -> i64 {
    let period = i64::from(period_s);
    let earliest = now + i64::from(exam_draw_delay_s) + HEAD_LEAD_S + MARGIN_S;
    (earliest / period + 1) * period
}

fn file_sha(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(venin_core::hex(&hasher.finalize()))
}

fn output(program: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::command(program.as_os_str())
        .args(args)
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} {}: {}",
            program.display(),
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
}

/// The git commit and tree of the working directory; a tree with changes is refused unless allowed.
pub fn code(allow_dirty: bool) -> Result<Code, String> {
    let git = Path::new("git");
    let dirty = !output(git, &["status", "--porcelain"])?.trim().is_empty();
    if dirty && !allow_dirty {
        return Err("the working tree has changes; commit them or pass --allow-dirty".into());
    }
    let lines = output(git, &["rev-parse", "HEAD", "HEAD^{tree}"])?;
    let mut ids = lines.split_whitespace().map(String::from);
    match (ids.next(), ids.next()) {
        (Some(commit), Some(tree)) => Ok(Code { tree, commit }),
        _ => Err("git rev-parse gave no commit and tree".into()),
    }
}

/// `llama-server --version` as one line: `llama-server 0.9.0 (build 12110, commit cd959bae9)`.
pub fn runtime(llama_server: &Path) -> Result<String, String> {
    let text = output(llama_server, &["--version"])?;
    text.lines()
        .find_map(|l| l.trim().strip_prefix("version: "))
        .map(|v| format!("llama-server {v}"))
        .ok_or_else(|| format!("{}: no version line", llama_server.display()))
}

/// The runtime and each model's weights hash: llama-server's version and the GGUF hashes for a self-hosted league;
/// `openrouter` and no hashes for a league served through OpenRouter, which publishes no weights files.
pub fn model_pins(config: &Config) -> Result<(String, Vec<String>), String> {
    let server = match &config.llama_server {
        Some(server) if !config.hosted() => server,
        _ => {
            return Ok((
                "openrouter".into(),
                vec![String::new(); config.models.len()],
            ));
        }
    };
    let mut weights = Vec::new();
    for m in &config.models {
        let gguf = m
            .gguf
            .as_deref()
            .ok_or_else(|| format!("model {}: no gguf", m.id))?;
        println!("hashing {}", gguf.display());
        weights.push(file_sha(gguf)?);
    }
    Ok((runtime(server)?, weights))
}

pub struct Pins {
    pub code: Code,
    pub runtime: String,
    pub weights: Vec<String>,
    pub address: String,
    pub genesis: i64,
}

pub fn manifest(config: &Config, files: &LibraryFiles, pins: Pins) -> Manifest {
    Manifest {
        rules_version: RULES_VERSION,
        league: config.league.clone(),
        genesis: pins.genesis,
        period_s: config.period_s,
        deadline_s: config.deadline_s,
        late_s: config.late_s,
        exam_delay_s: config.exam_draw_delay_s,
        k: config.k,
        library_sha: sha256_hex(files.strains.as_bytes()),
        tasks_sha: sha256_hex(files.tasks.as_bytes()),
        genes_sha: sha256_hex(files.genes.as_bytes()),
        rows_sha: sha256_hex(files.rows.as_bytes()),
        names_sha: sha256_hex(files.names.as_bytes()),
        code: pins.code,
        models: config
            .models
            .iter()
            .zip(pins.weights)
            .map(|(m, weights_sha)| Model {
                column: m.column.clone(),
                id: m.id.clone(),
                label: m.label.clone(),
                served_by: m.served_by.clone(),
                weights_sha,
                runtime: pins.runtime.clone(),
                sampling: m.sampling.clone(),
            })
            .collect(),
        bodies: roster(),
        anchor: Anchor {
            address: pins.address,
            chain_id: config.anchor.chain_id,
        },
        draw_chain_id: config.draw.chain_id,
        release_chain_id: config
            .release
            .as_ref()
            .map_or(config.draw.chain_id, |r| r.chain_id),
    }
}

/// Anchors every written file that has no anchor yet, in order.
pub fn flush(store: &Store, anchorer: &Anchorer) -> Result<(), String> {
    for (kind, n, sha) in store.due()? {
        let count = store.anchors()?.len() as u64;
        let entry = anchorer.anchor(kind, n, &sha, count)?;
        println!(
            "anchored {kind:?} {n}: {} in block {}",
            entry.tx, entry.block
        );
        store.push_anchor(entry)?;
        store.remove_state("inflight.json")?;
    }
    Ok(())
}

pub fn run(config: &Config, allow_dirty: bool) -> Result<(), String> {
    let store = Store::new(config.data_dir.clone(), config.state_dir.clone());
    if store.exists("manifest.json") {
        return Err(format!(
            "{} already holds a genesis; it is never overwritten",
            config.data_dir.display()
        ));
    }
    // An earlier lab's anchor in flight or Priors pause would carry into this record, and verify refuses a paused
    // Priors row that no sealed hour explains.
    for name in ["inflight.json", crate::priors::OUT] {
        if store.state_exists(name) {
            return Err(format!(
                "{} holds {name} from an earlier lab; a genesis starts without it",
                config.state_dir.display()
            ));
        }
    }
    let files = read_library(&config.library_dir)?;
    files.parse()?;
    let code = code(allow_dirty)?;
    let (runtime, weights) = model_pins(config)?;
    let address = format!(
        "{:#x}",
        key_address(&config.anchor.key_file, config.anchor.chain_id)?
    );
    let net = Net::new(&config.urls())?;
    let mut draw_chain = DrawChain::new(&net, &config.draw.rpcs, Duration::from_secs(1));
    let id = draw_chain.chain_id()?;
    if id != config.draw.chain_id {
        return Err(format!(
            "the draw RPC is chain {id}, the configuration says {}",
            config.draw.chain_id
        ));
    }
    let anchorer = Anchorer::new(&net, &config.anchor, &store)?;
    anchorer.check_chain()?;
    let genesis = genesis_time(crate::now(), config.period_s, config.exam_draw_delay_s);
    let manifest = manifest(
        config,
        &files,
        Pins {
            code,
            runtime,
            weights,
            address,
            genesis,
        },
    );
    for name in LIBRARY_FILES {
        let bytes =
            std::fs::read(config.library_dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        store.write(&format!("library/{name}"), &bytes)?;
    }
    store.write("manifest.json", &encode(&manifest)?)?;
    flush(&store, &anchorer)?;

    let manifest_block = store
        .anchors()?
        .first()
        .map(|a| a.block)
        .ok_or("the manifest has no anchor")?;
    let draw_at = anchorer.block_ts(manifest_block)? + i64::from(manifest.exam_delay_s);
    crate::sleep_until(draw_at + HEAD_LEAD_S);
    let (rpc, head) = draw_chain.head().map_err(|r| format!("exam draw: {r:?}"))?;
    let (draw, saved) = draw_chain
        .draw(draw_at, rpc, head, None)
        .map_err(|r| format!("exam draw: {r:?}"))?;
    let library = files.parse()?;
    let exam = Exam {
        families: exam_families(&library.strains, &draw.hash_bytes()?),
        draw,
    };
    store.write_draw("exam", &saved)?;
    store.write("exam.json", &encode(&exam)?)?;
    flush(&store, &anchorer)?;
    if crate::now() >= genesis {
        return Err(
            "the exam was anchored after the genesis time; start over in a new data dir".into(),
        );
    }
    println!(
        "genesis at {} ({}); held out: {}",
        genesis,
        venin_core::time::rfc3339(genesis).unwrap_or_default(),
        exam.families.join(", ")
    );
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::tests::text;
    use venin_core::hour::{Draw, State};
    use venin_core::lab::Lab;
    use venin_core::world::{WorldKind, parse_tasks};

    #[test]
    fn genesis_is_a_boundary_after_the_draw() {
        let g = genesis_time(1_000_000, 3600, 60);
        assert_eq!(g % 3600, 0);
        assert!(g > 1_000_000 + 60 + HEAD_LEAD_S + MARGIN_S);
        assert!(g <= 1_000_000 + 60 + HEAD_LEAD_S + MARGIN_S + 3600);
        assert_eq!(genesis_time(3209, 3600, 60), 3600);
        assert_eq!(genesis_time(3210, 3600, 60), 7200);
    }

    /// Production keeps `[release]` out until the token exists, so its manifest names the draw chain, the chain a
    /// later `[release]` must be on for the runner to take releases.
    #[test]
    fn the_release_chain_is_the_release_section_else_the_draw_chain() {
        let release_chain = |text: &str| {
            let config = Config::parse(text).expect("parses");
            let pins = Pins {
                code: Code {
                    tree: String::new(),
                    commit: String::new(),
                },
                runtime: String::new(),
                weights: vec![String::new(); 4],
                address: String::new(),
                genesis: 0,
            };
            manifest(&config, &LibraryFiles::default(), pins).release_chain_id
        };
        assert_eq!(
            release_chain(include_str!("../../../deploy/prod/venin.toml")),
            4663
        );
        let dev = include_str!("../../../deploy/dev/venin.toml");
        assert_eq!(release_chain(dev), 31337);
        let (head, tail) = dev.split_once("[release]").expect("a release section");
        let models = tail.find("[[models]]").expect("models after it");
        assert_eq!(
            release_chain(&format!("{head}{}", &tail[models..])),
            4663,
            "the dev draw chain, not its anchor chain 31337"
        );
    }

    #[test]
    fn a_genesis_refuses_state_an_earlier_lab_left() {
        let dir = crate::anchor::tests::temp("stale-state");
        let mut config =
            Config::parse(include_str!("../../../deploy/dev/venin.toml")).expect("parses");
        (config.data_dir, config.state_dir) = (dir.join("data"), dir.join("state"));
        let store = Store::new(config.data_dir.clone(), config.state_dir.clone());
        for name in ["inflight.json", crate::priors::OUT] {
            store
                .write_state_json(name, &serde_json::json!({"n": 3}))
                .expect("writes");
            let e = run(&config, true).expect_err("refuses");
            assert!(e.contains(name), "{e}");
            assert!(!store.exists("manifest.json"));
            store.remove_state(name).expect("removes");
        }
    }

    #[test]
    fn versions_and_file_hashes() {
        let dir = crate::anchor::tests::temp("pins");
        let script = dir.join("server");
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'version: 1.2.3 (build 4)' >&2\necho \"$HOME\" >&2\n",
        )
        .expect("writes");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert_eq!(
            runtime(&script).expect("version"),
            "llama-server 1.2.3 (build 4)"
        );
        let out = output(&script, &[]).expect("runs");
        assert!(!out.contains(&std::env::var("HOME").expect("HOME")));
        std::fs::write(dir.join("w"), b"abc").expect("writes");
        assert_eq!(
            file_sha(&dir.join("w")).expect("hash"),
            venin_core::sha256_hex(b"abc")
        );
        let c = code(true).expect("git");
        assert_eq!((c.commit.len(), c.tree.len()), (40, 40));
    }

    /// The library with its ward tasks copied as Priors tasks, while the library has none of its own.
    pub fn with_priors(files: &LibraryFiles) -> LibraryFiles {
        let parsed = parse_tasks(&files.tasks).expect("tasks");
        if parsed.iter().any(|t| t.world == WorldKind::Priors) {
            return files.clone();
        }
        let mut tasks = files.tasks.clone();
        for mut task in parsed {
            task.id.push('p');
            task.world = WorldKind::Priors;
            tasks.push_str(&venin_core::json::to_string(&task).expect("writes"));
            tasks.push('\n');
        }
        LibraryFiles {
            tasks,
            ..files.clone()
        }
    }

    pub fn exam_draw() -> Draw {
        Draw {
            block: 100,
            hash: "11".repeat(32),
            ts: 1_799_000_000,
            prev_ts: 1_798_999_999,
            rpc_agree: 2,
        }
    }

    pub fn lab_with_priors(files: &LibraryFiles) -> (Lab, State) {
        let files = with_priors(files);
        let config = Config::parse(&text(
            Path::new("/tmp/venin-lab"),
            "http://127.0.0.1:1",
            "http://127.0.0.1:2/v1",
        ))
        .expect("config");
        let pins = Pins {
            code: Code {
                tree: "0".repeat(40),
                commit: "0".repeat(40),
            },
            runtime: "test".into(),
            weights: vec!["0".repeat(64); 4],
            address: format!("0x{}", "5".repeat(40)),
            genesis: 1_800_000_000,
        };
        let library = files.parse().expect("library");
        let draw = exam_draw();
        let lab = Lab {
            manifest: manifest(&config, &files, pins),
            exam: Exam {
                families: exam_families(&library.strains, &draw.hash_bytes().expect("hash")),
                draw,
            },
            library,
        };
        let state = State::genesis(&lab).expect("genesis");
        (lab, state)
    }
}
