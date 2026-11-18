//! `venin verify --data DIR [--rpc URL] [--draw-rpc URL]`: the core verifier over a data dir, with the chain facts it
//! can gather: given the anchor chain's RPC, every anchor transaction and the address nonce; given the draw chain's
//! RPC, every draw block and its parent. The draws the runner saved are never taken as chain facts.

use crate::draw::{Block, Saved};
use crate::net::{Net, quantity_u64};
use crate::store::Store;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use venin_core::hour::HourRecord;
use venin_core::lab::{Exam, Manifest};
use venin_core::verify::{AnchorEntry, Bundle, Chain, Report, Tx, verify};

fn check_chain_id(net: &Net, rpc: &str, want: u64, what: &str) -> Result<(), String> {
    let id = quantity_u64(&net.rpc(rpc, "eth_chainId", json!([]))?)?;
    if id != want {
        return Err(format!(
            "the {what} RPC is chain {id}; the manifest names chain {want}"
        ));
    }
    Ok(())
}

/// What the anchor chain says about the anchors: their transactions and the anchor address's nonce. A transaction
/// the RPC does not know goes to `absent`, one it knows but has not mined to `pending`.
pub fn fetch_anchors(
    net: &Net,
    rpc: &str,
    manifest: &Manifest,
    anchors: &[AnchorEntry],
    chain: &mut Chain,
) -> Result<(), String> {
    check_chain_id(net, rpc, manifest.anchor.chain_id, "anchor")?;
    let mut times: BTreeMap<u64, i64> = BTreeMap::new();
    for a in anchors {
        let tx = net.rpc(rpc, "eth_getTransactionByHash", json!([a.tx]))?;
        if tx.is_null() {
            chain.absent.insert(a.tx.clone());
            continue;
        }
        if tx["blockNumber"].is_null() {
            chain.pending.insert(a.tx.clone());
            continue;
        }
        let block = quantity_u64(&tx["blockNumber"])?;
        let ts = match times.get(&block) {
            Some(ts) => *ts,
            None => {
                let b = net.rpc(
                    rpc,
                    "eth_getBlockByNumber",
                    json!([crate::net::hex_quantity(block.into()), false]),
                )?;
                let ts =
                    i64::try_from(quantity_u64(&b["timestamp"])?).map_err(|e| e.to_string())?;
                times.insert(block, ts);
                ts
            }
        };
        let text = |key: &str| tx[key].as_str().unwrap_or_default().to_ascii_lowercase();
        chain.txs.insert(
            a.tx.clone(),
            Tx {
                from: text("from"),
                to: text("to"),
                input: text("input"),
                block,
                ts,
            },
        );
    }
    let nonce = net.rpc(
        rpc,
        "eth_getTransactionCount",
        json!([manifest.anchor.address, "latest"]),
    )?;
    chain.nonce = Some(quantity_u64(&nonce)?);
    Ok(())
}

/// The draw block numbers the record names: the exam list's and every hour's.
fn draw_blocks(bundle: &Bundle) -> Vec<u64> {
    let exam = serde_json::from_slice::<Exam>(&bundle.exam)
        .ok()
        .map(|e| e.draw.block);
    let hours = bundle.hours.values().filter_map(|b| {
        serde_json::from_slice::<HourRecord>(b)
            .ok()?
            .draw
            .map(|d| d.block)
    });
    let mut out: Vec<u64> = exam.into_iter().chain(hours).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Each draw block and its parent as the draw chain gives them, so verify compares the record with the chain itself.
pub fn fetch_draws(
    net: &Net,
    rpc: &str,
    manifest: &Manifest,
    blocks: &[u64],
    chain: &mut Chain,
) -> Result<(), String> {
    check_chain_id(net, rpc, manifest.draw_chain_id, "draw")?;
    let get = |n: u64| -> Result<Block, String> {
        let v = net.rpc(
            rpc,
            "eth_getBlockByNumber",
            json!([crate::net::hex_quantity(n.into()), false]),
        )?;
        if v.is_null() {
            return Err(format!("the draw chain has no block {n}"));
        }
        Block::parse(&v)
    };
    for &n in blocks {
        let saved = Saved {
            block: get(n)?,
            parent: get(n.checked_sub(1).ok_or("block 0 has no parent")?)?,
        };
        let (number, block) = saved.chain_block()?;
        chain.blocks.insert(number, block);
    }
    Ok(())
}

/// Verifies the data dir and returns the report.
pub fn check(data: &Path, rpc: Option<&str>, draw_rpc: Option<&str>) -> Result<Report, String> {
    let store = Store::new(data.to_path_buf(), data.join("state-unused"));
    let bundle = store.bundle()?;
    let files = store.library()?;
    let mut chain = Chain::default();
    let manifest = || -> Result<Manifest, String> {
        serde_json::from_slice(&bundle.manifest).map_err(|e| format!("manifest.json: {e}"))
    };
    let urls: Vec<String> = rpc.into_iter().chain(draw_rpc).map(String::from).collect();
    let net = Net::new(&urls)?;
    if let Some(rpc) = rpc {
        let anchors: Vec<AnchorEntry> =
            serde_json::from_slice(&bundle.anchors).map_err(|e| format!("anchors.json: {e}"))?;
        fetch_anchors(&net, rpc, &manifest()?, &anchors, &mut chain)?;
    }
    if let Some(draw_rpc) = draw_rpc {
        fetch_draws(
            &net,
            draw_rpc,
            &manifest()?,
            &draw_blocks(&bundle),
            &mut chain,
        )?;
    }
    Ok(verify(&bundle, &chain, &files))
}

pub fn print(report: &Report) {
    println!("hours: {}", report.hours);
    println!("late hours: {:?}", report.late);
    println!("void share: {} ppm", report.void_share_ppm);
    println!("withheld runs: {}", report.withheld);
    println!("taken down: {:?}", report.taken_down);
    println!("state not recomputed: {:?}", report.not_recomputed);
    println!("chain facts not checked: {}", report.unchecked);
    for r in &report.headline {
        println!(
            "row {}: {} exam attacks, {} fooled, {} breached",
            r.row, r.attacks, r.fooled, r.breached
        );
    }
    for e in &report.errors {
        println!("error: {e}");
    }
    println!(
        "{}",
        if report.ok() {
            "verified"
        } else {
            "NOT verified"
        }
    );
}

pub fn run(data: &Path, rpc: Option<&str>, draw_rpc: Option<&str>) -> Result<(), String> {
    let report = check(data, rpc, draw_rpc)?;
    print(&report);
    if report.ok() {
        Ok(())
    } else {
        Err(format!("{} errors", report.errors.len()))
    }
}
