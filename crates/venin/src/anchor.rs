//! Anchoring: a zero-value transaction from the anchor address to itself whose calldata commits to a file. The key
//! is read from its 0600 file after start; the chain id, gas limit, fee and the rolling daily spend are checked before
//! anything is signed. A transaction that stays pending is sent again with the same nonce and calldata and a fee at
//! least 12.5% higher. The attack round's burn (`venin attack`) is landed the same way, from its own key and spend log.

use crate::config::{AnchorChain, wei};
use crate::net::{Net, quantity, quantity_u64};
use crate::store::Store;
use alloy::consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy::eips::eip2718::Encodable2718;
use alloy::primitives::{Address, Bytes, TxKind, U256};
use alloy::signers::SignerSync;
use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};
use venin_core::verify::{AnchorEntry, AnchorKind, calldata};

/// The ten accounts anvil derives from its public test mnemonic. Their keys are public, so they sign only on the
/// local test chain.
pub const ANVIL_ACCOUNTS: [&str; 10] = [
    "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
    "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
    "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc",
    "0x90f79bf6eb2c4f870365e785982e1f101e93b906",
    "0x15d34aaf54267db7d7c367839aaf71a00a2c6a65",
    "0x9965507d1a55bcc2695c58ba16fb37d819b0a4dc",
    "0x976ea74026e726554db657fa54763abd0c3a0aa9",
    "0x14dc79964da2c08b23698b3d3cc7ca32193d9955",
    "0x23618e81e3f5cdf7f54c3d65f7fbc0abf5b21e8f",
    "0xa0ee7a142d267c1f36714e4a8f75612f20a79720",
];
pub const TEST_CHAIN: u64 = 31337;
const DAY_S: i64 = 86_400;

/// Reads the anchor key. The file must be readable by its owner only.
pub fn load_key(path: &Path) -> Result<PrivateKeySigner, String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{}: the key file must be mode 0600",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.trim()
        .parse::<PrivateKeySigner>()
        .map_err(|_| format!("{}: not a private key", path.display()))
}

pub fn address(signer: &PrivateKeySigner) -> String {
    format!("{:#x}", signer.address())
}

/// Refuses a public test key anywhere but the local test chain.
pub fn allowed_on(signer: &PrivateKeySigner, chain_id: u64) -> Result<(), String> {
    refuse_test_key(signer, chain_id, "anchor")
}

/// `allowed_on` for the key of `role`, which the refusal names.
pub fn refuse_test_key(signer: &PrivateKeySigner, chain_id: u64, role: &str) -> Result<(), String> {
    if chain_id != TEST_CHAIN && ANVIL_ACCOUNTS.contains(&address(signer).as_str()) {
        return Err(format!(
            "the {role} key is one of anvil's public test keys; it signs only on chain {TEST_CHAIN}"
        ));
    }
    Ok(())
}

/// A fee raised by at least one eighth.
pub fn bump(fee: u128) -> u128 {
    fee + fee.div_ceil(8).max(1)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sent {
    pub tx: String,
    pub max_fee: String,
    pub priority: String,
    pub gas: u64,
    pub ts: i64,
}

/// The anchor being sent: written before the first broadcast, removed once its entry is in anchors.json.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inflight {
    pub kind: AnchorKind,
    pub n: u64,
    pub sha: String,
    pub nonce: u64,
    pub sent: Vec<Sent>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spend {
    pub ts: i64,
    pub wei: String,
}

/// The most a set of sends can cost: gas limit times fee cap, summed over the last 24 hours.
pub fn spent_today(log: &[Spend], now: i64) -> u128 {
    log.iter()
        .filter(|s| now - s.ts < DAY_S)
        .filter_map(|s| s.wei.parse::<u128>().ok())
        .sum()
}

pub struct Anchorer<'a> {
    net: &'a Net,
    config: &'a AnchorChain,
    store: &'a Store,
    signer: PrivateKeySigner,
    /// What the key is for, as its errors name it: `anchor` for the anchor key.
    role: &'static str,
    /// The state file that logs this key's sends for its daily cap.
    spend: &'static str,
    /// How long one send may stay pending before it is sped up.
    pub wait: Duration,
    pub poll: Duration,
    /// How long a watched flight's nonce must stay taken, with no receipt for any attempt on any RPC, before the
    /// flight stops.
    pub taken_after: Duration,
}

impl<'a> Anchorer<'a> {
    pub fn new(net: &'a Net, config: &'a AnchorChain, store: &'a Store) -> Result<Self, String> {
        Self::with_spend(net, config, store, "anchor", "spend.json")
    }

    /// A key that lands other transactions on its chain under the same checks and caps: its errors name `role`, and
    /// its sends are logged in `spend`.
    pub fn with_spend(
        net: &'a Net,
        config: &'a AnchorChain,
        store: &'a Store,
        role: &'static str,
        spend: &'static str,
    ) -> Result<Self, String> {
        let signer = load_key(&config.key_file)?;
        refuse_test_key(&signer, config.chain_id, role)?;
        Ok(Anchorer {
            net,
            config,
            store,
            signer,
            role,
            spend,
            wait: Duration::from_secs(60),
            poll: Duration::from_secs(2),
            taken_after: Duration::from_secs(300),
        })
    }

    /// An EIP-191 `personal_sign` signature over `text`, as `0x` and 130 lower-case hex digits.
    pub fn sign_message(&self, text: &str) -> Result<String, String> {
        let signature = self
            .signer
            .sign_message_sync(text.as_bytes())
            .map_err(|e| format!("signing: {e}"))?;
        Ok(format!("0x{}", venin_core::hex(&signature.as_bytes())))
    }

    pub fn address(&self) -> String {
        address(&self.signer)
    }

    fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut last = String::new();
        for url in &self.config.rpcs {
            match self.net.rpc(url, method, params.clone()) {
                Ok(v) => return Ok(v),
                Err(e) => last = e,
            }
        }
        Err(format!("{} chain: {last}", self.role))
    }

    pub fn check_chain(&self) -> Result<(), String> {
        let id = quantity_u64(&self.rpc("eth_chainId", json!([]))?)?;
        if id != self.config.chain_id {
            return Err(format!(
                "the {} RPC is chain {id}, the configuration says {}",
                self.role, self.config.chain_id
            ));
        }
        Ok(())
    }

    pub fn nonce(&self, tag: &str) -> Result<u64, String> {
        quantity_u64(&self.rpc("eth_getTransactionCount", json!([self.address(), tag]))?)
    }

    /// The key's latest and pending nonces when every configured RPC gives the same pair, each pair read from one
    /// RPC; `None` when one does not answer or they disagree, so nothing is decided on a reading two nodes split.
    pub fn nonces(&self) -> Option<(u64, u64)> {
        let mut agreed = None;
        for url in &self.config.rpcs {
            let read = |tag: &str| {
                let n = self
                    .net
                    .rpc(url, "eth_getTransactionCount", json!([self.address(), tag]));
                n.and_then(|v| quantity_u64(&v)).ok()
            };
            let pair = (read("latest")?, read("pending")?);
            if agreed.is_some_and(|a| a != pair) {
                return None;
            }
            agreed = Some(pair);
        }
        agreed
    }

    /// The timestamp of block `n` of the anchor chain.
    pub fn block_ts(&self, n: u64) -> Result<i64, String> {
        let block = self.rpc(
            "eth_getBlockByNumber",
            json!([crate::net::hex_quantity(n.into()), false]),
        )?;
        i64::try_from(quantity_u64(&block["timestamp"])?).map_err(|e| e.to_string())
    }

    pub fn balance(&self) -> Result<u128, String> {
        quantity(&self.rpc("eth_getBalance", json!([self.address(), "latest"]))?)
    }

    fn fees(&self) -> Result<(u128, u128), String> {
        let block = self.rpc("eth_getBlockByNumber", json!(["latest", false]))?;
        let base = quantity(&block["baseFeePerGas"]).unwrap_or(0);
        let priority = quantity(&self.rpc("eth_maxPriorityFeePerGas", json!([]))?)?;
        Ok((base * 2 + priority, priority))
    }

    /// Signs and broadcasts one attempt. Caps are checked first; `keep` writes the attempts, this one included, before
    /// it leaves.
    fn send(
        &self,
        flight: &mut Flight,
        keep: &dyn Fn(&[Sent]) -> Result<(), String>,
        max_fee: u128,
        priority: u128,
        now: i64,
    ) -> Result<(), String> {
        let input = format!("0x{}", venin_core::hex(flight.input));
        let gas = match flight.sent.last() {
            Some(s) => s.gas,
            None => {
                let estimate = self.rpc(
                    "eth_estimateGas",
                    json!([{"from": self.address(), "to": format!("{:#x}", flight.to), "value": "0x0", "data": input}]),
                )?;
                let estimate = quantity_u64(&estimate)?;
                estimate + (estimate * flight.headroom_pct).div_ceil(100)
            }
        };
        if gas > self.config.gas_limit_max {
            return Err(format!(
                "gas {gas} is above gas_limit_max {}",
                self.config.gas_limit_max
            ));
        }
        let fee_cap = wei(&self.config.max_fee_per_gas_max)?;
        if max_fee > fee_cap {
            return Err(format!(
                "fee {max_fee} wei per gas is above max_fee_per_gas_max {fee_cap}"
            ));
        }
        let cost = u128::from(gas) * max_fee;
        let mut log: Vec<Spend> = self.store.state_json(self.spend)?.unwrap_or_default();
        let cap = wei(&self.config.daily_cap_wei)?;
        if spent_today(&log, now) + cost > cap {
            return Err(format!(
                "the daily {} cap of {cap} wei would be passed",
                self.role
            ));
        }
        self.check_chain()?;
        let tx = TxEip1559 {
            chain_id: self.config.chain_id,
            nonce: flight.nonce,
            gas_limit: gas,
            max_fee_per_gas: max_fee,
            max_priority_fee_per_gas: priority,
            to: TxKind::Call(flight.to),
            value: U256::ZERO,
            access_list: Default::default(),
            input: Bytes::copy_from_slice(flight.input),
        };
        let signature = self
            .signer
            .sign_hash_sync(&tx.signature_hash())
            .map_err(|e| format!("signing: {e}"))?;
        let signed = tx.into_signed(signature);
        let hash = format!("{:#x}", signed.hash());
        let raw = format!(
            "0x{}",
            venin_core::hex(&TxEnvelope::from(signed).encoded_2718())
        );
        flight.sent.push(Sent {
            tx: hash,
            max_fee: max_fee.to_string(),
            priority: priority.to_string(),
            gas,
            ts: now,
        });
        keep(&flight.sent)?;
        log.retain(|s| now - s.ts < DAY_S);
        log.push(Spend {
            ts: now,
            wei: cost.to_string(),
        });
        self.store.write_state_json(self.spend, &log)?;
        match self.broadcast(&raw) {
            Ok(()) => Ok(()),
            Err(Broadcast::Unheard(e)) => Err(e),
            Err(Broadcast::Refused(why)) => {
                // Every RPC refused it, so it never left: it is no attempt to land and no spend against the cap.
                flight.sent.pop();
                keep(&flight.sent)?;
                log.pop();
                self.store.write_state_json(self.spend, &log)?;
                Err(format!(
                    "every {} RPC refused the transaction: {why}",
                    self.role
                ))
            }
        }
    }

    /// Sends a signed transaction to each RPC in turn until one takes it. `Refused` only when every RPC answered with
    /// an error of its own, so the transaction never left; `Unheard` when one gave no answer, as it may have.
    fn broadcast(&self, raw: &str) -> Result<(), Broadcast> {
        let (mut refused, mut unheard) = (Vec::new(), None);
        for url in &self.config.rpcs {
            match self
                .net
                .rpc_answer(url, "eth_sendRawTransaction", json!([raw]))
            {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(e)) => refused.push(format!("{url}: {e}")),
                Err(e) => unheard = Some(e),
            }
        }
        Err(match unheard {
            Some(e) => Broadcast::Unheard(format!("{} chain: {e}", self.role)),
            None => Broadcast::Refused(refused.join("; ")),
        })
    }

    /// Whichever attempt was mined, if any, and whether it ran or reverted (an out-of-gas included).
    fn mined(&self, sent: &[Sent]) -> Result<Option<Landed>, String> {
        for s in sent {
            let receipt = self.rpc("eth_getTransactionReceipt", json!([s.tx]))?;
            if receipt.is_null() {
                continue;
            }
            if quantity(&receipt["status"])? != 1 {
                return Ok(Some(Landed::Reverted(s.tx.clone())));
            }
            let block = quantity_u64(&receipt["blockNumber"])?;
            return Ok(Some(Landed::Mined(s.tx.clone(), block)));
        }
        Ok(None)
    }

    /// Anchors one file and returns its entry for anchors.json. `count` is the number of entries already there, which
    /// is the nonce the anchor must take. Resumes an anchor left in flight by an earlier run.
    pub fn anchor(
        &self,
        kind: AnchorKind,
        n: u64,
        sha: &str,
        count: u64,
    ) -> Result<AnchorEntry, String> {
        let inflight = match self.store.state_json::<Inflight>("inflight.json")? {
            Some(f) if (f.kind, f.n, f.sha.as_str()) == (kind, n, sha) => f,
            Some(f) => {
                return Err(format!(
                    "another anchor is in flight: {:?} {} with nonce {}",
                    f.kind, f.n, f.nonce
                ));
            }
            None => {
                let nonce = self.nonce("latest")?;
                if nonce != count {
                    return Err(format!(
                        "the anchor address has sent {nonce} transactions but anchors.json has {count}"
                    ));
                }
                Inflight {
                    kind,
                    n,
                    sha: sha.to_string(),
                    nonce,
                    sent: Vec::new(),
                }
            }
        };
        let input = venin_core::unhex(&calldata(kind, n, sha)).ok_or("calldata is not hex")?;
        let mut flight = Flight {
            to: self.signer.address(),
            input: &input,
            nonce: inflight.nonce,
            sent: inflight.sent,
            headroom_pct: 0,
            watch_nonce: false,
        };
        let nonce = flight.nonce;
        let keep = |sent: &[Sent]| {
            let inflight = Inflight {
                kind,
                n,
                sha: sha.to_string(),
                nonce,
                sent: sent.to_vec(),
            };
            self.store.write_state_json("inflight.json", &inflight)
        };
        match self.land(&mut flight, &keep)? {
            Landed::Mined(tx, block) => Ok(AnchorEntry { kind, n, tx, block }),
            Landed::Reverted(tx) => Err(format!("anchor {tx} reverted")),
        }
    }

    /// Lands one transaction from the key's address: `flight.input` sent to `flight.to` with `flight.nonce`, until one
    /// attempt is mined. `flight.sent` holds the attempts made so far (an earlier run's, when it resumes), and `keep`
    /// writes them to the state dir before each new one leaves. A pending attempt is sent again after `wait` with the
    /// same nonce and input and a fee at least 12.5% higher. Returns the attempt mined, ran or reverted.
    ///
    /// A watched flight (`watch_nonce`) looks at its nonce before each speed-up. When the nonce has moved on, an
    /// attempt landed on a node the RPC that answered lags behind, or another transaction took it: no speed-up goes
    /// out, every RPC is asked for every attempt's receipt, again each `wait`, and only after `taken_after` without one
    /// does the flight stop, rather than speed up a nonce that is gone into the daily cap. Anchors are not watched.
    pub fn land(
        &self,
        flight: &mut Flight,
        keep: &dyn Fn(&[Sent]) -> Result<(), String>,
    ) -> Result<Landed, String> {
        let mut taken_since: Option<Instant> = None;
        loop {
            if let Some(landed) = self.mined(&flight.sent)? {
                return Ok(landed);
            }
            let now = crate::now();
            let (max_fee, priority) = match flight.sent.last() {
                None => self.fees()?,
                Some(s) if now - s.ts < self.wait.as_secs() as i64 => {
                    self.await_receipt(&flight.sent, s.ts)?;
                    continue;
                }
                Some(_) if flight.watch_nonce && self.nonce("latest")? > flight.nonce => {
                    if let Some(landed) = self.mined_anywhere(&flight.sent)? {
                        return Ok(landed);
                    }
                    let since = *taken_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= self.taken_after {
                        return Err(format!(
                            "nonce {} of {} moved on and no RPC has a receipt for any of its attempts ({}) after {} \
                             s: another transaction took it, and nothing more is sent",
                            flight.nonce,
                            self.address(),
                            flight
                                .sent
                                .iter()
                                .map(|s| s.tx.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            self.taken_after.as_secs()
                        ));
                    }
                    std::thread::sleep(self.wait.min(self.taken_after));
                    continue;
                }
                Some(s) => {
                    taken_since = None;
                    let (fresh_fee, fresh_priority) = self.fees()?;
                    let max_fee = bump(wei(&s.max_fee)?).max(fresh_fee);
                    let priority = bump(wei(&s.priority)?).max(fresh_priority);
                    (max_fee, priority.min(max_fee))
                }
            };
            self.send(flight, keep, max_fee, priority, now)?;
            self.await_receipt(&flight.sent, now)?;
        }
    }

    /// Whichever attempt any configured RPC has a receipt for; `None` only when every RPC answered for every attempt
    /// without one.
    fn mined_anywhere(&self, sent: &[Sent]) -> Result<Option<Landed>, String> {
        for url in &self.config.rpcs {
            for s in sent {
                let receipt = self
                    .net
                    .rpc(url, "eth_getTransactionReceipt", json!([s.tx]))?;
                if receipt.is_null() {
                    continue;
                }
                if quantity(&receipt["status"])? != 1 {
                    return Ok(Some(Landed::Reverted(s.tx.clone())));
                }
                let block = quantity_u64(&receipt["blockNumber"])?;
                return Ok(Some(Landed::Mined(s.tx.clone(), block)));
            }
        }
        Ok(None)
    }

    fn await_receipt(&self, sent: &[Sent], since: i64) -> Result<(), String> {
        let t = Instant::now();
        let left = self.wait.saturating_sub(Duration::from_secs(
            u64::try_from(crate::now() - since).unwrap_or(0),
        ));
        while t.elapsed() < left {
            if self.mined(sent)?.is_some() {
                return Ok(());
            }
            std::thread::sleep(self.poll);
        }
        Ok(())
    }
}

/// A transaction being landed: what it sends, the nonce it takes and every attempt so far.
pub struct Flight<'i> {
    pub to: Address,
    pub input: &'i [u8],
    pub nonce: u64,
    pub sent: Vec<Sent>,
    /// Gas above the estimate the first attempt asks for, in percent: a call whose cost moves between the estimate
    /// and the block (a token transfer, a chain that prices its data) needs room an anchor does not.
    pub headroom_pct: u64,
    /// Looks at the nonce before each speed-up and stops when another transaction took it (see `land`).
    pub watch_nonce: bool,
}

/// Why a broadcast did not go out.
enum Broadcast {
    /// Every RPC answered with an error of its own: the transaction never left.
    Refused(String),
    /// Some RPC gave no answer: the transaction may have left.
    Unheard(String),
}

/// How a transaction landed: mined and run (its hash and block), or mined and reverted (its hash).
#[derive(Debug, PartialEq, Eq)]
pub enum Landed {
    Mined(String, u64),
    Reverted(String),
}

/// The address a key file signs for, after the same checks the runner makes.
pub fn key_address(path: &Path, chain_id: u64) -> Result<Address, String> {
    let signer = load_key(path)?;
    allowed_on(&signer, chain_id)?;
    Ok(signer.address())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::net::hex_quantity;
    use crate::net::tests::serve;
    use alloy::consensus::Transaction;
    use alloy::eips::eip2718::Decodable2718;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "venin-test-{name}-{}-{}",
            std::process::id(),
            crate::now()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Writes a fresh random key with the given mode and returns its path.
    pub fn key_file(dir: &Path, mode: u32) -> std::path::PathBuf {
        let path = dir.join("anchor.key");
        let key = venin_core::hex(&crate::random::<32>());
        std::fs::write(&path, format!("0x{key}\n")).expect("writes");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        path
    }

    #[test]
    fn the_key_file_must_be_private() {
        let dir = temp("key");
        let path = key_file(&dir, 0o644);
        assert!(load_key(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let signer = load_key(&path).expect("loads");
        assert!(allowed_on(&signer, 4663).is_ok());
        std::fs::write(&path, "not a key").expect("writes");
        assert!(load_key(&path).is_err());
    }

    #[test]
    fn fees_rise_by_an_eighth_and_spend_rolls_over_a_day() {
        assert_eq!(bump(8), 9);
        assert_eq!(bump(9), 11);
        assert_eq!(bump(0), 1);
        assert!(bump(1_000_000_007) * 8 >= 1_000_000_007 * 9);
        let log = vec![
            Spend {
                ts: 0,
                wei: "5".into(),
            },
            Spend {
                ts: DAY_S,
                wei: "7".into(),
            },
        ];
        assert_eq!(spent_today(&log, DAY_S), 7);
        assert_eq!(spent_today(&log, DAY_S - 1), 12);
    }

    /// One JSON-RPC endpoint that serves both chains of a test: draw blocks `block_at(base, n)` with the head at
    /// block 1,000,000, and anchor transactions, of which the first `pending` sends are never mined. Every raw
    /// transaction is recorded.
    pub fn chain_mock(
        chain_id: u64,
        pending: usize,
        from: &str,
        base: i64,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let raws: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = raws.clone();
        let from = from.to_string();
        let (url, _) = serve(move |_, _, body| {
            let v: Value = serde_json::from_str(body).expect("json");
            let sent: Vec<TxEnvelope> = log
                .lock()
                .expect("lock")
                .iter()
                .map(|r| {
                    let bytes = venin_core::unhex(r).expect("hex");
                    TxEnvelope::decode_2718(&mut bytes.as_slice()).expect("decodes")
                })
                .collect();
            let find = |hash: &Value| {
                sent.iter()
                    .position(|t| Some(format!("{:#x}", t.tx_hash()).as_str()) == hash.as_str())
                    .filter(|&i| i >= pending)
            };
            let param = &v["params"][0];
            let result = match v["method"].as_str().expect("method") {
                "eth_chainId" => json!(hex_quantity(chain_id.into())),
                "eth_getTransactionCount" => {
                    json!(hex_quantity(sent.len().saturating_sub(pending) as u128))
                }
                "eth_getBalance" => json!("0xde0b6b3a7640000"),
                "eth_getBlockByNumber" => {
                    let n = match param.as_str() {
                        Some("latest") => 1_000_000,
                        _ => quantity_u64(param).expect("number"),
                    };
                    let reply: Value = serde_json::from_str(&crate::draw::tests::rpc_json(
                        &crate::draw::tests::block_at(base, n),
                        "",
                    ))
                    .expect("json");
                    reply["result"].clone()
                }
                "eth_maxPriorityFeePerGas" => json!("0x1"),
                "eth_estimateGas" => json!("0x54d8"),
                "eth_sendRawTransaction" => {
                    let raw = param.as_str().expect("raw").to_string();
                    let bytes = venin_core::unhex(&raw).expect("hex");
                    let tx = TxEnvelope::decode_2718(&mut bytes.as_slice()).expect("decodes");
                    log.lock().expect("lock").push(raw);
                    json!(format!("{:#x}", tx.tx_hash()))
                }
                "eth_getTransactionReceipt" => match find(param) {
                    Some(i) => {
                        json!({"status": "0x1", "blockNumber": hex_quantity(100 + i as u128)})
                    }
                    None => Value::Null,
                },
                "eth_getTransactionByHash" => match find(param) {
                    Some(i) => json!({
                        "from": from,
                        "to": format!("{:#x}", sent[i].to().expect("a call")),
                        "input": format!("0x{}", venin_core::hex(sent[i].input())),
                        "blockNumber": hex_quantity(100 + i as u128),
                    }),
                    None => Value::Null,
                },
                _ => Value::Null,
            };
            let reply = json!({"jsonrpc": "2.0", "id": 1, "result": result});
            (200, Vec::new(), reply.to_string())
        });
        (url, raws)
    }

    /// How `wallet_chain` takes what it is sent.
    #[derive(Clone, Copy, Default)]
    pub struct Script {
        /// The first raw transactions taken are never mined.
        pub stuck: usize,
        /// Of those mined, the first revert.
        pub reverted: usize,
        /// None is mined, but the nonce moves on as if another transaction had taken it.
        pub taken: bool,
        /// Transactions in the mempool beyond those mined.
        pub pending: u64,
        /// The first broadcasts are refused with the node's own error, and never taken.
        pub refused: usize,
    }

    pub fn hash_of(raw: &str) -> String {
        let bytes = venin_core::unhex(raw).expect("hex");
        let tx = TxEnvelope::decode_2718(&mut bytes.as_slice()).expect("decodes");
        format!("{:#x}", tx.tx_hash())
    }

    /// A chain of id 31337 for one sending key, estimating 34,000 gas: each raw transaction taken is mined at once as
    /// `script` says, and each one mined moves the nonce on. An RPC's path changes what it sees: `/lag` has no
    /// receipts, `/split` reports nonces one higher and `/fail` answers no nonce read. Returns its URL and the raw
    /// transactions taken.
    pub fn wallet_chain(script: Script) -> (String, Arc<Mutex<Vec<String>>>) {
        let raws: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = raws.clone();
        let refusals = Arc::new(Mutex::new(script.refused));
        let (url, _) = serve(move |_, path, body| {
            let v: Value = serde_json::from_str(body).expect("json");
            let hashes: Vec<String> = log
                .lock()
                .expect("lock")
                .iter()
                .map(|r| hash_of(r))
                .collect();
            let param = &v["params"];
            let mined = if script.taken {
                hashes.len()
            } else {
                hashes.len().saturating_sub(script.stuck)
            };
            let method = v["method"].as_str().unwrap_or_default();
            if path == "/fail" && method == "eth_getTransactionCount" {
                return (500, Vec::new(), String::new());
            }
            let result = match method {
                "eth_chainId" => json!("0x7a69"),
                "eth_getTransactionCount" => {
                    let more = if param[1] == "pending" {
                        script.pending
                    } else {
                        0
                    };
                    let split = u64::from(path == "/split");
                    json!(hex_quantity(u128::from(mined as u64 + more + split)))
                }
                "eth_getBlockByNumber" => {
                    json!({"number": "0x64", "timestamp": "0x1", "baseFeePerGas": "0x3b9aca00"})
                }
                "eth_maxPriorityFeePerGas" => json!("0x1"),
                "eth_estimateGas" => json!("0x84d0"),
                "eth_sendRawTransaction" => {
                    let mut left = refusals.lock().expect("lock");
                    if *left > 0 {
                        *left -= 1;
                        let no = json!({"jsonrpc": "2.0", "id": 1,
                            "error": {"code": -32000, "message": "insufficient funds for gas * price + value"}});
                        return (200, Vec::new(), no.to_string());
                    }
                    let raw = param[0].as_str().unwrap_or_default().to_string();
                    let hash = hash_of(&raw);
                    log.lock().expect("lock").push(raw);
                    json!(hash)
                }
                "eth_getTransactionReceipt" => match hashes.iter().position(|h| param[0] == *h) {
                    Some(i) if path != "/lag" && !script.taken && i >= script.stuck => json!({
                        "status": if i - script.stuck < script.reverted { "0x0" } else { "0x1" },
                        "blockNumber": hex_quantity(100 + i as u128),
                    }),
                    _ => Value::Null,
                },
                _ => Value::Null,
            };
            let reply = json!({"jsonrpc": "2.0", "id": 1, "result": result});
            (200, Vec::new(), reply.to_string())
        });
        (url, raws)
    }

    /// A broadcast every RPC refuses never left: it is dropped from the anchor in flight and from the spend log, the
    /// node's own error is what the anchor fails with, and the next try sends the same nonce afresh. Anchors that did
    /// go out resume as before.
    #[test]
    fn a_refused_broadcast_leaves_no_attempt_and_no_spend() {
        let dir = temp("refused");
        let key = key_file(&dir, 0o600);
        let (url, raws) = wallet_chain(Script {
            refused: 2,
            ..Script::default()
        });
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        let config = chain_config(&url, &key, TEST_CHAIN);
        let store = Store::new(dir.join("data"), dir.join("state"));
        let mut a = Anchorer::new(&net, &config, &store).expect("anchorer");
        a.wait = Duration::from_secs(1);
        a.poll = Duration::from_millis(100);
        let sha = "ab".repeat(32);
        for _ in 0..2 {
            let e = a.anchor(AnchorKind::Hour, 7, &sha, 0).expect_err("refused");
            assert!(e.contains("insufficient funds"), "{e}");
            let inflight: Inflight = store
                .state_json("inflight.json")
                .expect("reads")
                .expect("kept");
            assert_eq!((inflight.nonce, inflight.sent.len()), (0, 0));
            let log: Vec<Spend> = store
                .state_json("spend.json")
                .expect("reads")
                .expect("kept");
            assert!(
                log.is_empty(),
                "nothing that never left counts against the cap"
            );
        }
        let entry = a.anchor(AnchorKind::Hour, 7, &sha, 0).expect("anchors");
        assert_eq!(entry.tx, hash_of(&raws.lock().expect("lock")[0]));
        assert_eq!(raws.lock().expect("lock").len(), 1);
        let log: Vec<Spend> = store
            .state_json("spend.json")
            .expect("reads")
            .expect("kept");
        assert_eq!(log.len(), 1);
    }

    pub fn chain_config(rpc: &str, key: &Path, chain_id: u64) -> AnchorChain {
        AnchorChain {
            chain_id,
            rpcs: vec![rpc.to_string()],
            key_file: key.to_path_buf(),
            gas_limit_max: 40_000,
            max_fee_per_gas_max: "100000000000".into(),
            daily_cap_wei: "1000000000000000".into(),
        }
    }

    #[test]
    fn a_pending_anchor_is_sped_up_with_the_same_nonce_and_calldata() {
        let dir = temp("anchor");
        let key = key_file(&dir, 0o600);
        let (url, raws) = chain_mock(4663, 2, "", 0);
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        let config = chain_config(&url, &key, 4663);
        let store = Store::new(dir.join("data"), dir.join("state"));
        let mut a = Anchorer::new(&net, &config, &store).expect("anchorer");
        a.wait = Duration::from_secs(1);
        a.poll = Duration::from_millis(100);
        let sha = "ab".repeat(32);
        let entry = a.anchor(AnchorKind::Hour, 7, &sha, 0).expect("anchors");
        assert_eq!(entry.block, 102);
        let raws = raws.lock().expect("lock").clone();
        assert_eq!(raws.len(), 3);
        let txs: Vec<TxEnvelope> = raws
            .iter()
            .map(|r| {
                let bytes = venin_core::unhex(r).expect("hex");
                TxEnvelope::decode_2718(&mut bytes.as_slice()).expect("decodes")
            })
            .collect();
        let want = calldata(AnchorKind::Hour, 7, &sha);
        for (i, tx) in txs.iter().enumerate() {
            assert_eq!(tx.nonce(), 0);
            assert_eq!(format!("0x{}", venin_core::hex(tx.input())), want);
            assert_eq!(tx.to(), Some(a.signer.address()));
            assert_eq!(tx.chain_id(), Some(4663));
            if i > 0 {
                assert!(tx.max_fee_per_gas() * 8 >= txs[i - 1].max_fee_per_gas() * 9);
            }
        }
        assert_eq!(entry.tx, format!("{:#x}", txs[2].tx_hash()));
        assert_eq!(
            store
                .state_json::<Vec<Spend>>("spend.json")
                .expect("reads")
                .expect("written")
                .len(),
            3
        );
    }

    #[test]
    fn caps_and_chain_id_stop_an_anchor_before_signing() {
        let dir = temp("caps");
        let key = key_file(&dir, 0o600);
        let store = Store::new(dir.join("data"), dir.join("state"));
        let (url, raws) = chain_mock(4663, 0, "", 0);
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        let sha = "cd".repeat(32);
        let mut tight = chain_config(&url, &key, 4663);
        tight.gas_limit_max = 21_000;
        let a = Anchorer::new(&net, &tight, &store).expect("anchorer");
        assert!(a.anchor(AnchorKind::Hour, 1, &sha, 0).is_err());
        let mut cheap = chain_config(&url, &key, 4663);
        cheap.max_fee_per_gas_max = "1000".into();
        let a = Anchorer::new(&net, &cheap, &store).expect("anchorer");
        assert!(a.anchor(AnchorKind::Hour, 1, &sha, 0).is_err());
        let mut daily = chain_config(&url, &key, 4663);
        daily.daily_cap_wei = "1000".into();
        let a = Anchorer::new(&net, &daily, &store).expect("anchorer");
        assert!(a.anchor(AnchorKind::Hour, 1, &sha, 0).is_err());
        let wrong = chain_config(&url, &key, 1);
        let a = Anchorer::new(&net, &wrong, &store).expect("anchorer");
        assert!(a.anchor(AnchorKind::Hour, 1, &sha, 0).is_err());
        let plain = chain_config(&url, &key, 4663);
        let a = Anchorer::new(&net, &plain, &store).expect("anchorer");
        assert!(a.anchor(AnchorKind::Hour, 1, &sha, 3).is_err());
        assert!(raws.lock().expect("lock").is_empty());
    }
}

/// Against a real anvil, when one is on PATH: its accounts are the public ones refused off the test chain, and an
/// anchor lands with the calldata verify expects.
#[cfg(test)]
mod anvil {
    use super::tests::{chain_config, key_file, temp};
    use super::*;
    use std::process::{Child, Stdio};

    struct Kill(Child);

    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn anchors_land_on_anvil() {
        if crate::command("anvil".as_ref())
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a free port")
            .port();
        let _anvil = Kill(
            crate::command("anvil".as_ref())
                .args([
                    "--port",
                    &port.to_string(),
                    "--chain-id",
                    "31337",
                    "--silent",
                ])
                .stdout(Stdio::null())
                .spawn()
                .expect("anvil starts"),
        );
        let url = format!("http://127.0.0.1:{port}");
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        let t = Instant::now();
        while net.rpc(&url, "eth_chainId", json!([])).is_err() {
            assert!(t.elapsed() < Duration::from_secs(20), "anvil did not start");
            std::thread::sleep(Duration::from_millis(100));
        }
        let accounts = net.rpc(&url, "eth_accounts", json!([])).expect("accounts");
        let accounts: Vec<String> = accounts
            .as_array()
            .expect("a list")
            .iter()
            .map(|a| a.as_str().expect("text").to_ascii_lowercase())
            .collect();
        assert_eq!(accounts, ANVIL_ACCOUNTS);

        let dir = temp("anvil");
        let key = key_file(&dir, 0o600);
        let store = Store::new(dir.join("data"), dir.join("state"));
        let config = chain_config(&url, &key, TEST_CHAIN);
        let a = Anchorer::new(&net, &config, &store).expect("anchorer");
        net.rpc(
            &url,
            "anvil_setBalance",
            json!([a.address(), "0xde0b6b3a7640000"]),
        )
        .expect("funds");
        let sha = "ef".repeat(32);
        let first = a.anchor(AnchorKind::Manifest, 0, &sha, 0).expect("anchors");
        store.remove_state("inflight.json").expect("clears");
        let second = a.anchor(AnchorKind::Hour, 0, &sha, 1).expect("anchors");
        assert!(second.block > first.block);
        let (t1, t2) = (
            a.block_ts(first.block).expect("a time"),
            a.block_ts(second.block).expect("a time"),
        );
        assert!(t1 > 1_700_000_000 && t2 >= t1);
        assert_eq!(a.nonce("latest").expect("nonce"), 2);
        let tx = net
            .rpc(&url, "eth_getTransactionByHash", json!([second.tx]))
            .expect("tx");
        assert_eq!(tx["input"], calldata(AnchorKind::Hour, 0, &sha));
        assert_eq!(tx["to"], a.address());
        assert!(a.balance().expect("balance") < 1_000_000_000_000_000_000);
    }
}
