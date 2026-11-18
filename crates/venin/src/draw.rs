//! The draw block of a period: the first block of the draw chain whose timestamp is at or after the period start,
//! found by interpolation, its hash read from a second RPC, and the block and its parent kept for verify.

use crate::net::{Net, quantity, quantity_u64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use venin_core::hour::Draw;
use venin_core::record::VoidReason;
use venin_core::verify::Block as ChainBlock;

/// Blocks per second assumed for the first probe when no earlier draw block is known.
const PROBE_RATE: i64 = 4;
const MAX_PROBES: u32 = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub number: u64,
    pub hash: String,
    pub parent_hash: String,
    pub ts: i64,
}

impl Block {
    pub fn parse(v: &Value) -> Result<Self, String> {
        let hash = |key: &str| {
            v.get(key)
                .and_then(Value::as_str)
                .and_then(|h| h.strip_prefix("0x"))
                .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| format!("block without a {key}"))
        };
        Ok(Block {
            number: quantity_u64(&v["number"])?,
            hash: hash("hash")?,
            parent_hash: hash("parentHash")?,
            ts: i64::try_from(quantity(&v["timestamp"])?).map_err(|e| e.to_string())?,
        })
    }
}

/// The draw block and its parent, as read; written to the data dir for verify.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub block: Block,
    pub parent: Block,
}

impl Saved {
    /// The chain fact verify compares the record with, once the pair is consistent.
    pub fn chain_block(&self) -> Result<(u64, ChainBlock), String> {
        if self.parent.number + 1 != self.block.number || self.block.parent_hash != self.parent.hash
        {
            return Err(format!(
                "draw block {} does not follow its parent",
                self.block.number
            ));
        }
        Ok((
            self.block.number,
            ChainBlock {
                hash: self.block.hash.clone(),
                ts: self.block.ts,
                parent_ts: self.parent.ts,
            },
        ))
    }
}

/// The first block with `ts >= start` and its parent. `head` must already be at or past `start`; `hint` is an
/// earlier block whose number and time are known (its hash may be empty).
pub fn first_at(
    start: i64,
    head: Block,
    hint: Option<(u64, i64)>,
    fetch: &mut dyn FnMut(u64) -> Result<Block, String>,
) -> Result<Saved, String> {
    if head.ts < start {
        return Err("the chain has not reached the period start".into());
    }
    let mut hi = head;
    let mut lo: Option<Block> =
        hint.filter(|&(n, ts)| ts < start && n < hi.number)
            .map(|(number, ts)| Block {
                number,
                hash: String::new(),
                parent_hash: String::new(),
                ts,
            });
    let mut probes = 0;
    let mut probe = |n: u64, probes: &mut u32| {
        *probes += 1;
        if *probes > MAX_PROBES {
            return Err("the draw block search did not converge".to_string());
        }
        let b = fetch(n)?;
        if b.number != n {
            return Err(format!("asked for block {n}, got {}", b.number));
        }
        Ok(b)
    };
    let mut back = u64::try_from((hi.ts - start + 2) * PROBE_RATE).unwrap_or(1);
    while lo.is_none() {
        if hi.number == 0 {
            return Err("the chain starts after the period start".into());
        }
        let b = probe(hi.number.saturating_sub(back), &mut probes)?;
        if b.ts >= start {
            hi = b;
            back = back.saturating_mul(2);
        } else {
            lo = Some(b);
        }
    }
    let mut lo = lo.expect("set above");
    let mut bisect = false;
    while hi.number - lo.number > 1 {
        let width = hi.number - lo.number;
        let guess = if bisect {
            lo.number + width / 2
        } else {
            let span = i128::from(hi.ts - lo.ts).max(1);
            let offset = i128::from(start - lo.ts) * i128::from(width) / span;
            lo.number + u64::try_from(offset).unwrap_or(0)
        }
        .clamp(lo.number + 1, hi.number - 1);
        let b = probe(guess, &mut probes)?;
        if b.ts >= start {
            hi = b;
        } else {
            lo = b;
        }
        bisect = !bisect && (hi.number - lo.number) * 2 > width;
    }
    if lo.hash.is_empty() {
        lo = probe(lo.number, &mut probes)?;
    }
    Ok(Saved {
        block: hi,
        parent: lo,
    })
}

/// Reads the draw chain, at most one request per `pace`.
pub struct DrawChain<'a> {
    net: &'a Net,
    rpcs: &'a [String],
    pace: Duration,
    last: Option<Instant>,
    pub calls: u32,
}

impl<'a> DrawChain<'a> {
    pub fn new(net: &'a Net, rpcs: &'a [String], pace: Duration) -> Self {
        DrawChain {
            net,
            rpcs,
            pace,
            last: None,
            calls: 0,
        }
    }

    fn call(&mut self, rpc: usize, method: &str, params: Value) -> Result<Value, String> {
        if let Some(wait) = self.last.and_then(|t| self.pace.checked_sub(t.elapsed())) {
            std::thread::sleep(wait);
        }
        self.last = Some(Instant::now());
        self.calls += 1;
        self.net.rpc(&self.rpcs[rpc], method, params)
    }

    fn block(&mut self, rpc: usize, tag: Value) -> Result<Block, String> {
        let v = self.call(rpc, "eth_getBlockByNumber", json!([tag, false]))?;
        if v.is_null() {
            return Err("no such block".into());
        }
        Block::parse(&v)
    }

    pub fn chain_id(&mut self) -> Result<u64, String> {
        let v = self.call(0, "eth_chainId", json!([]))?;
        quantity_u64(&v)
    }

    /// The latest block from the first RPC that answers, and that RPC's index.
    pub fn head(&mut self) -> Result<(usize, Block), VoidReason> {
        for rpc in 0..self.rpcs.len() {
            match self.block(rpc, json!("latest")) {
                Ok(b) => return Ok((rpc, b)),
                Err(e) => eprintln!("draw rpc {rpc}: {e}"),
            }
        }
        Err(VoidReason::RpcUnavailable)
    }

    /// The draw block for `start`, with its hash read again from the next RPC that answers.
    pub fn draw(
        &mut self,
        start: i64,
        rpc: usize,
        head: Block,
        hint: Option<(u64, i64)>,
    ) -> Result<(Draw, Saved), VoidReason> {
        let saved = first_at(start, head, hint, &mut |n| {
            self.block(rpc, json!(crate::net::hex_quantity(n.into())))
        })
        .map_err(|e| {
            eprintln!("draw search: {e}");
            VoidReason::RpcUnavailable
        })?;
        let mut agree = 1;
        for other in (0..self.rpcs.len()).filter(|&i| i != rpc) {
            let number = json!(crate::net::hex_quantity(saved.block.number.into()));
            match self.block(other, number) {
                Ok(b) if b.hash == saved.block.hash => {
                    agree += 1;
                    break;
                }
                Ok(b) => {
                    eprintln!(
                        "draw rpc {other} has block {} as {}, rpc {rpc} as {}",
                        b.number, b.hash, saved.block.hash
                    );
                    return Err(VoidReason::RpcMismatch);
                }
                Err(e) => eprintln!("draw rpc {other}: {e}"),
            }
        }
        let draw = Draw {
            block: saved.block.number,
            hash: saved.block.hash.clone(),
            ts: saved.block.ts,
            prev_ts: saved.parent.ts,
            rpc_agree: agree,
        };
        Ok((draw, saved))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::net::tests::serve;
    use std::sync::{Arc, Mutex};

    /// A chain whose block `n` has timestamp `base + n * 2 / 7` (several blocks share a second).
    pub fn block_at(base: i64, n: u64) -> Block {
        Block {
            number: n,
            hash: format!("{n:064x}"),
            parent_hash: format!("{:064x}", n.saturating_sub(1)),
            ts: base + i64::try_from(n * 2 / 7).expect("small"),
        }
    }

    fn block(n: u64) -> Block {
        block_at(1_700_000_000, n)
    }

    pub fn rpc_json(b: &Block, salt: &str) -> String {
        let hash = if salt.is_empty() {
            b.hash.clone()
        } else {
            format!("{salt:0>64}")
        };
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"number":"0x{:x}","hash":"0x{hash}","parentHash":"0x{}","timestamp":"0x{:x}","baseFeePerGas":"0x3b9aca00"}}}}"#,
            b.number, b.parent_hash, b.ts
        )
    }

    fn reference(start: i64, head: u64) -> u64 {
        (0..=head).find(|&n| block(n).ts >= start).expect("exists")
    }

    #[test]
    fn the_search_finds_the_first_block_at_the_start() {
        let head = block(1_000_000);
        for (start, hint) in [
            (head.ts - 30, None),
            (
                head.ts - 30,
                Some((block(980_000).number, block(980_000).ts)),
            ),
            (head.ts, None),
            (head.ts - 5000, Some((10, block(10).ts))),
            (block(999_000).ts, Some((999_900, block(999_900).ts))),
        ] {
            let mut calls = 0;
            let saved = first_at(start, head.clone(), hint, &mut |n| {
                calls += 1;
                Ok(block(n))
            })
            .expect("found");
            let want = reference(start, head.number);
            assert_eq!(saved.block.number, want, "start {start}");
            assert_eq!(saved.parent.number, want - 1);
            assert!(saved.block.ts >= start && saved.parent.ts < start);
            assert!(calls <= 40, "{calls} calls");
            assert!(saved.chain_block().is_ok());
        }
    }

    #[test]
    fn the_search_refuses_what_cannot_hold() {
        let head = block(100);
        let mut fetch = |n| Ok(block(n));
        assert!(first_at(head.ts + 1, head.clone(), None, &mut fetch).is_err());
        let mut wrong = |_| Ok(block(5));
        assert!(first_at(head.ts - 10, head, None, &mut wrong).is_err());
        let saved = Saved {
            block: block(9),
            parent: block(7),
        };
        assert!(saved.chain_block().is_err());
    }

    fn mock(second_salt: &'static str) -> (String, Arc<Mutex<u32>>) {
        let calls = Arc::new(Mutex::new(0));
        let count = calls.clone();
        let (url, _) = serve(move |_, path, body| {
            *count.lock().expect("lock") += 1;
            let v: Value = serde_json::from_str(body).expect("json");
            let tag = v["params"][0].as_str().expect("tag");
            let n = if tag == "latest" {
                1_000_000
            } else {
                u64::from_str_radix(tag.trim_start_matches("0x"), 16).expect("hex")
            };
            let salt = if path == "/second" { second_salt } else { "" };
            (200, Vec::new(), rpc_json(&block(n), salt))
        });
        (url, calls)
    }

    #[test]
    fn a_draw_needs_a_second_rpc_to_agree() {
        let (url, calls) = mock("");
        let rpcs = vec![url.clone(), format!("{url}/second")];
        let net = Net::new(&rpcs).expect("client");
        let mut chain = DrawChain::new(&net, &rpcs, Duration::ZERO);
        let (rpc, head) = chain.head().expect("head");
        let start = head.ts - 30;
        let (draw, saved) = chain.draw(start, rpc, head, None).expect("draw");
        assert_eq!(draw.block, reference(start, 1_000_000));
        assert_eq!(draw.rpc_agree, 2);
        assert_eq!(draw.prev_ts, saved.parent.ts);
        assert_eq!(*calls.lock().expect("lock"), chain.calls);

        let (url, _) = mock("bad");
        let rpcs = vec![url.clone(), format!("{url}/second")];
        let net = Net::new(&rpcs).expect("client");
        let mut chain = DrawChain::new(&net, &rpcs, Duration::ZERO);
        let (rpc, head) = chain.head().expect("head");
        assert_eq!(
            chain.draw(head.ts - 30, rpc, head, None),
            Err(VoidReason::RpcMismatch)
        );
    }

    #[test]
    fn requests_are_paced() {
        let (url, _) = mock("");
        let rpcs = vec![url];
        let net = Net::new(&rpcs).expect("client");
        let mut chain = DrawChain::new(&net, &rpcs, Duration::from_millis(200));
        let t = Instant::now();
        for _ in 0..3 {
            chain.head().expect("head");
        }
        assert!(t.elapsed() >= Duration::from_millis(400));
    }
}
