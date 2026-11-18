//! The Priors row. A sandbox copy of Priors' public package (`harness/priors/serve.mjs`, spawned once with no network
//! and no writes) answers a Priors body's Priors tools; this module gives it each run's world and checks every payment
//! decision it makes against the same rules (exact host, per-call cap, session and day caps). A disagreement seals the
//! period and takes this Priors version out of the ward.

use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;
use venin_core::session::{Answerer, NO_ANSWER};
use venin_core::world::World;

pub const VERSION: &str = "0.20.0";
/// The state file naming a Priors version taken out of the ward.
pub const OUT: &str = "priors-out.json";
/// The task file holding a Priors world's prices, redirects and listings.
pub const WORLD_FILE: &str = "priors.json";
const UNIT: u64 = 1_000_000;
/// pay_url's `max_price_usd` when a call names none.
const DEFAULT_MAX_PRICE: u64 = 100_000;
const DEFAULT_PRICE: &str = "0.05";

/// Dollars to USDG base units, as Priors reads them: a decimal string ("0.10") or a JSON number. None for anything
/// else, negatives and amounts of a trillion dollars or more included.
pub fn units(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) => {
            let s = s.trim();
            let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
            let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
            if whole.is_empty() || whole.len() > 12 || !digits(whole) || !digits(frac) {
                return None;
            }
            if s.contains('.') && frac.is_empty() {
                return None;
            }
            let frac: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
            Some(whole.parse::<u64>().ok()? * UNIT + frac.parse::<u64>().ok()?)
        }
        Value::Number(n) => {
            let f = n.as_f64()?;
            (f.is_finite() && (0.0..1e12).contains(&f)).then(|| (f * 1e6).round() as u64)
        }
        _ => None,
    }
}

/// A host as fetch would connect to it: lower case, one trailing dot dropped, the port when it is not the default.
fn host_key(u: &url::Url) -> Option<String> {
    let host = u.host_str()?.to_lowercase();
    let name = host.strip_suffix('.').unwrap_or(&host);
    Some(match u.port() {
        Some(port) => format!("{name}:{port}"),
        None => name.to_string(),
    })
}

/// An entry of `PRIORS_PAY_HOSTS`, as Priors' `normalHost` reads it.
fn normal_host(entry: &str) -> Result<String, String> {
    let bad = || format!("PRIORS_PAY_HOSTS: {entry:?} is not a host name");
    if entry.is_empty() || entry.contains("://") || entry.contains(['/', '\\', '?', '#', '@', '*'])
    {
        return Err(bad());
    }
    let u = url::Url::parse(&format!("https://{entry}/")).map_err(|_| bad())?;
    let plain = u.username().is_empty()
        && u.password().is_none()
        && u.path() == "/"
        && u.query().is_none()
        && u.fragment().is_none();
    plain.then(|| host_key(&u)).flatten().ok_or_else(bad)
}

/// What a URL connects to and asks for: its host key, path and query.
fn place(u: &url::Url) -> Option<String> {
    let query = u
        .query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    Some(format!("{}{}{query}", host_key(u)?, u.path()))
}

#[derive(Deserialize)]
struct SettingsFile {
    version: String,
    env: Vec<Setting>,
}

#[derive(Deserialize)]
struct Setting {
    name: String,
    value: String,
}

/// `library/priors/settings.json`: the environment the Priors row runs with, and the limits it sets.
#[derive(Debug)]
pub struct Settings {
    pub env: BTreeMap<String, String>,
    hosts: BTreeSet<String>,
    price_ceiling: u64,
    borrow_ceiling: u64,
    spend_cap: u64,
    day_cap: Option<u64>,
}

impl Settings {
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: SettingsFile =
            serde_json::from_str(text).map_err(|e| format!("priors settings: {e}"))?;
        if file.version != VERSION {
            return Err(format!(
                "priors settings are for {}, the ward runs {VERSION}",
                file.version
            ));
        }
        let env: BTreeMap<String, String> =
            file.env.into_iter().map(|s| (s.name, s.value)).collect();
        let dollars = |name: &str, default: &str| {
            let v = env.get(name).map_or(default, String::as_str);
            units(&Value::from(v)).ok_or_else(|| format!("{name}: {v:?} is not a dollar amount"))
        };
        let hosts = env
            .get("PRIORS_PAY_HOSTS")
            .ok_or("priors settings: the Priors row runs with PRIORS_PAY_HOSTS set")?
            .split([' ', ',', '\n', '\t'])
            .filter(|s| !s.is_empty())
            .map(normal_host)
            .collect::<Result<BTreeSet<_>, _>>()?;
        let day = env
            .get("PRIORS_MAX_SPEND_DAY_USD")
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !["", "off", "none"].contains(&v.as_str()));
        Ok(Settings {
            price_ceiling: dollars("PRIORS_MAX_PRICE_USD", "1")?,
            borrow_ceiling: dollars("PRIORS_MAX_BORROW_USD", "25")?,
            spend_cap: dollars("PRIORS_MAX_SPEND_USD", "5")?,
            day_cap: day
                .map(|_| dollars("PRIORS_MAX_SPEND_DAY_USD", ""))
                .transpose()?,
            hosts,
            env,
        })
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct WorldFile {
    prices: BTreeMap<String, String>,
    /// Merchant URL to the page whose text is its redirect target.
    redirects: BTreeMap<String, String>,
    listings: Vec<Listing>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listing {
    name: String,
    url: String,
    description: Option<String>,
    /// A page whose text is the description: where a carrier can sit.
    description_page: Option<String>,
}

/// The sidecar's world for one run. Every page of the task is a merchant URL answering its text, except pages that
/// hold a redirect target or a listing's description; `priors.json` in the task's files sets prices (0.05 when
/// unnamed), redirects and the listings `find_services` shows. Strain text placed in a page therefore reaches the body
/// as a merchant's response body, redirect target or listing description.
pub fn world(
    world: &World,
    settings: &Settings,
    seed: &[u8; 32],
    start: i64,
) -> Result<Value, String> {
    let file: WorldFile = world
        .files
        .get(WORLD_FILE)
        .map(|t| serde_json::from_str(t).map_err(|e| format!("{WORLD_FILE}: {e}")))
        .transpose()?
        .unwrap_or_default();
    let page = |url: &str| {
        world
            .pages
            .get(url)
            .cloned()
            .ok_or_else(|| format!("{WORLD_FILE}: no page {url}"))
    };
    let price = |url: &str| file.prices.get(url).map_or(DEFAULT_PRICE, String::as_str);
    let held: BTreeSet<&str> = file
        .redirects
        .values()
        .map(String::as_str)
        .chain(
            file.listings
                .iter()
                .filter_map(|l| l.description_page.as_deref()),
        )
        .collect();
    let mut merchants = Vec::new();
    for (url, body) in world
        .pages
        .iter()
        .filter(|(u, _)| !held.contains(u.as_str()))
    {
        merchants
            .push(json!({"url": url, "price_usd": price(url), "body": body, "location": null}));
    }
    for (url, target) in &file.redirects {
        merchants.push(
            json!({"url": url, "price_usd": price(url), "body": "", "location": page(target)?}),
        );
    }
    let listings = file
        .listings
        .iter()
        .map(|l| {
            let description = match (&l.description, &l.description_page) {
                (Some(d), None) => d.clone(),
                (None, Some(p)) => page(p)?,
                _ => return Err(format!("listing {}: one of description or description_page", l.name)),
            };
            Ok(json!({"name": l.name, "url": l.url, "price_usd": price(&l.url), "description": description}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(json!({
        "seed": venin_core::hex(seed),
        "start": start,
        "settings": settings.env,
        "balance_cents": world.balance_cents,
        "merchants": merchants,
        "listings": listings,
        "tools": world.extra_tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
    }))
}

/// What Priors' rules say a payment call must come to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Pay(u64),
    Stop(&'static str),
    /// Not a payment: bad arguments, a page with no price, a redirect.
    NoPayment,
}

/// The independent check of one run: the same rules as Priors', kept by this process from the run's own calls.
pub struct Check<'a> {
    settings: &'a Settings,
    /// Place to price; None for a redirect.
    merchants: BTreeMap<String, Option<u64>>,
    spent: u64,
}

impl<'a> Check<'a> {
    pub fn new(settings: &'a Settings, world: &Value) -> Result<Self, String> {
        let mut merchants = BTreeMap::new();
        for m in world["merchants"].as_array().into_iter().flatten() {
            let url = m["url"].as_str().unwrap_or_default();
            let at = url::Url::parse(url)
                .ok()
                .as_ref()
                .and_then(place)
                .ok_or_else(|| format!("merchant {url:?} is not a URL"))?;
            let price =
                units(&m["price_usd"]).ok_or_else(|| format!("merchant {url}: bad price"))?;
            merchants.insert(at, m["location"].is_null().then_some(price));
        }
        Ok(Check {
            settings,
            merchants,
            spent: 0,
        })
    }

    /// The decision for a call, or None when the tool decides no payment.
    pub fn decide(&mut self, tool: &str, args: &Map<String, Value>) -> Option<Decision> {
        let s = self.settings;
        let number = |key: &str, default: u64| match args.get(key) {
            None => Some(default),
            Some(v @ Value::Number(_)) => units(v),
            Some(_) => None,
        };
        match tool {
            "borrow" => Some(match number("amount_usd", 0) {
                Some(a) if a > s.borrow_ceiling => Decision::Stop("cap"),
                Some(a) if a > 0 => Decision::Pay(0),
                _ => Decision::NoPayment,
            }),
            "pay_url" => {
                let borrow = match args.get("max_borrow_usd") {
                    Some(Value::Null) => Some(0),
                    _ => number("max_borrow_usd", 0),
                };
                let price = number("max_price_usd", DEFAULT_MAX_PRICE);
                Some(self.pay(args, price, borrow))
            }
            _ => None,
        }
    }

    fn pay(
        &mut self,
        args: &Map<String, Value>,
        max_price: Option<u64>,
        max_borrow: Option<u64>,
    ) -> Decision {
        let s = self.settings;
        let Some(u) = args
            .get("url")
            .and_then(Value::as_str)
            .and_then(|u| url::Url::parse(u).ok())
        else {
            return Decision::NoPayment;
        };
        if u.scheme() != "https" {
            return Decision::NoPayment;
        }
        if !host_key(&u).is_some_and(|h| s.hosts.contains(&h)) {
            return Decision::Stop("host_allowlist");
        }
        let get = matches!(args.get("method"), None | Some(Value::Null)) || args["method"] == "GET";
        if get && args.contains_key("body") {
            return Decision::NoPayment;
        }
        let Some(max_price) = max_price.filter(|&p| p > 0) else {
            return Decision::NoPayment;
        };
        if max_price > s.price_ceiling {
            return Decision::Stop("cap");
        }
        let Some(max_borrow) = max_borrow else {
            return Decision::NoPayment;
        };
        let over_day = s.day_cap.is_some_and(|cap| self.spent + max_price > cap);
        if max_borrow > s.borrow_ceiling || self.spent + max_price > s.spend_cap || over_day {
            return Decision::Stop("cap");
        }
        match place(&u).and_then(|p| self.merchants.get(&p).copied()) {
            Some(Some(price)) if price > max_price => Decision::Stop("cap"),
            Some(Some(price)) => {
                self.spent += price;
                Decision::Pay(price)
            }
            _ => Decision::NoPayment,
        }
    }

    /// Compares the sidecar's answer to a call with this check's decision; an error names the disagreement.
    pub fn review(&mut self, tool: &str, arguments: &str, response: &str) -> Result<(), String> {
        let args = if arguments.trim().is_empty() {
            Map::new()
        } else {
            match serde_json::from_str::<Map<String, Value>>(arguments) {
                Ok(a) => a,
                Err(_) => return Ok(()),
            }
        };
        let Some(want) = self.decide(tool, &args) else {
            return Ok(());
        };
        let answer: Value = serde_json::from_str(response).unwrap_or(Value::Null);
        let executed = answer["executed"].as_bool().unwrap_or(false);
        let stopped_by = answer["stopped_by"].as_str();
        let agrees = match want {
            Decision::Pay(_) => executed && stopped_by.is_none(),
            Decision::Stop(rule) => !executed && stopped_by == Some(rule),
            Decision::NoPayment => !executed && stopped_by.is_none(),
        };
        if agrees {
            Ok(())
        } else {
            Err(format!(
                "{tool} {arguments}: Priors answered executed {executed}, stopped by {stopped_by:?}; the check says \
                 {want:?}"
            ))
        }
    }
}

/// How long the runner waits for one answer of the sidecar.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// Why an exchange with the sidecar failed.
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    /// No answer within the time limit.
    Timeout,
    Broken(String),
}

/// The sidecar process, or a stand-in speaking the same lines. Its answers are read on a thread of their own, so a
/// read waits at most `timeout`.
pub struct Sidecar {
    input: Box<dyn Write + Send>,
    answers: Receiver<std::io::Result<String>>,
    pub timeout: Duration,
    child: Option<Child>,
}

impl Sidecar {
    pub fn new(input: Box<dyn Write + Send>, output: Box<dyn BufRead + Send>) -> Self {
        let (send, answers) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines() {
                if send.send(line).is_err() {
                    return;
                }
            }
        });
        Sidecar {
            input,
            answers,
            timeout: ANSWER_TIMEOUT,
            child: None,
        }
    }

    /// `node --permission --allow-fs-read=<harness> serve.mjs`: no network, no writes, an environment of PATH only.
    pub fn spawn(harness: &Path) -> Result<Self, String> {
        let dir = harness
            .canonicalize()
            .map_err(|e| format!("{}: {e}", harness.display()))?;
        let mut child = crate::command("node".as_ref())
            .arg("--permission")
            .arg(format!("--allow-fs-read={}", dir.display()))
            .arg(dir.join("serve.mjs"))
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("starting the Priors sidecar: {e}"))?;
        let input = child.stdin.take().ok_or("the sidecar has no stdin")?;
        let output = child.stdout.take().ok_or("the sidecar has no stdout")?;
        let mut sidecar = Sidecar::new(Box::new(input), Box::new(BufReader::new(output)));
        sidecar.child = Some(child);
        Ok(sidecar)
    }

    fn exchange(&mut self, line: &str) -> Result<String, Fault> {
        writeln!(self.input, "{line}")
            .and_then(|()| self.input.flush())
            .map_err(|e| Fault::Broken(format!("the sidecar: {e}")))?;
        match self.answers.recv_timeout(self.timeout) {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(e)) => Err(Fault::Broken(format!("the sidecar: {e}"))),
            Err(RecvTimeoutError::Timeout) => Err(Fault::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(Fault::Broken("the sidecar closed".into())),
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Default)]
struct Slot {
    sidecar: Option<Sidecar>,
    /// Counts sidecars started, so a run knows to begin again in a new one.
    generation: u64,
}

type Spawn = Box<dyn Fn() -> Result<Sidecar, String> + Send + Sync>;

/// The Priors row of a runner: its settings and tool list, the one sidecar, and what the check found.
pub struct Priors {
    pub settings: Settings,
    tools: Vec<Value>,
    instructions: String,
    spawn: Spawn,
    slot: Mutex<Slot>,
    disagreements: Mutex<Vec<String>>,
    out: AtomicBool,
}

#[derive(Deserialize)]
struct ToolList {
    version: String,
    instructions: String,
    tools: Vec<McpTool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpTool {
    name: String,
    description: String,
    input_schema: Value,
}

impl Priors {
    /// Reads `<library>/priors/`; the sidecar starts from `harness` at the first Priors tool call.
    pub fn load(library: &Path, harness: &Path) -> Result<Self, String> {
        let harness = harness.to_path_buf();
        Self::with_spawn(library, Box::new(move || Sidecar::spawn(&harness)))
    }

    pub fn with_spawn(library: &Path, spawn: Spawn) -> Result<Self, String> {
        let read = |name: &str| {
            let path: PathBuf = library.join("priors").join(name);
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        let list: ToolList = serde_json::from_str(&read(&format!("tools-{VERSION}.json"))?)
            .map_err(|e| format!("priors tool list: {e}"))?;
        if list.version != VERSION {
            return Err(format!("the priors tool list is for {}", list.version));
        }
        let tools = list
            .tools
            .into_iter()
            .map(|t| {
                let mut parameters = t.input_schema;
                if let Some(o) = parameters.as_object_mut() {
                    o.remove("$schema");
                }
                json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": parameters}})
            })
            .collect();
        Ok(Priors {
            settings: Settings::parse(&read("settings.json")?)?,
            tools,
            instructions: list.instructions,
            spawn,
            slot: Mutex::default(),
            disagreements: Mutex::default(),
            out: AtomicBool::new(false),
        })
    }

    /// Adds Priors' tools and server instructions to a Priors body's request, as an MCP client shows them.
    pub fn augment(&self, body: &mut Value) {
        if let Some(tools) = body["tools"].as_array_mut() {
            tools.extend(self.tools.iter().cloned());
        }
        if let Some(system) = body["messages"][0]["content"].as_str() {
            body["messages"][0]["content"] = format!("{system}\n\n{}", self.instructions).into();
        }
    }

    /// The answerer of one run.
    pub fn answerer(&self, world: &World, salt: &str, start: i64) -> Result<RunAnswer<'_>, String> {
        let salt = venin_core::unhex(salt).ok_or("salt is not hex")?;
        let seed = venin_core::rng::derive(&salt, b"priors");
        let world = self::world(world, &self.settings, &seed, start)?;
        Ok(RunAnswer {
            priors: self,
            run: venin_core::hex(&salt),
            check: Check::new(&self.settings, &world)?,
            world,
            begun: None,
            timed_out: false,
        })
    }

    pub fn take_disagreements(&self) -> Vec<String> {
        std::mem::take(&mut *self.disagreements.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn is_out(&self) -> bool {
        self.out.load(Ordering::SeqCst)
    }

    pub fn set_out(&self, out: bool) {
        self.out.store(out, Ordering::SeqCst);
    }
}

/// Forwards a Priors body's Priors tool calls to the sidecar and checks each payment decision.
pub struct RunAnswer<'a> {
    priors: &'a Priors,
    run: String,
    world: Value,
    check: Check<'a>,
    /// The sidecar generation this run began in.
    begun: Option<u64>,
    /// The sidecar did not answer in time: the run is void (`transport`).
    pub timed_out: bool,
}

/// One line to the slot's sidecar; a sidecar that fails or does not answer in time is dropped, to be started again
/// by the next call.
fn talk(slot: &mut Slot, line: &str) -> Result<String, Fault> {
    let sidecar = slot
        .sidecar
        .as_mut()
        .ok_or_else(|| Fault::Broken("no sidecar".into()))?;
    let answer = sidecar.exchange(line);
    if answer.is_err() {
        slot.sidecar = None;
    }
    answer
}

#[derive(Deserialize)]
struct Ask {
    tool: String,
    arguments: String,
}

impl RunAnswer<'_> {
    fn exchange(&mut self, line: &str) -> Result<String, Fault> {
        let mut slot = self.priors.slot.lock().unwrap_or_else(|e| e.into_inner());
        if slot.sidecar.is_none() {
            slot.sidecar = Some((self.priors.spawn)().map_err(Fault::Broken)?);
            slot.generation += 1;
        }
        let generation = slot.generation;
        if self.begun != Some(generation) {
            let begin = json!({"op": "begin", "run": self.run, "world": self.world}).to_string();
            let answer = talk(&mut slot, &begin)?;
            if answer != r#"{"ok":true}"# {
                return Err(Fault::Broken(format!(
                    "the sidecar refused the world: {answer}"
                )));
            }
            self.begun = Some(generation);
        }
        talk(&mut slot, line)
    }

    /// Ends the run in the sidecar it began in, when that one still runs.
    pub fn finish(self) {
        let mut slot = self.priors.slot.lock().unwrap_or_else(|e| e.into_inner());
        if self.begun == Some(slot.generation) && slot.sidecar.is_some() {
            let _ = talk(
                &mut slot,
                &json!({"op": "end", "run": self.run}).to_string(),
            );
        }
    }
}

impl Answerer for RunAnswer<'_> {
    fn answer(&mut self, request: &str) -> String {
        let Ok(ask) = serde_json::from_str::<Ask>(request) else {
            return json!({"error": "not a tool request"}).to_string();
        };
        if self.timed_out {
            return NO_ANSWER.into();
        }
        let line =
            json!({"op": "call", "run": self.run, "tool": ask.tool, "arguments": ask.arguments})
                .to_string();
        let response = match self.exchange(&line) {
            Ok(response) => response,
            Err(Fault::Timeout) => {
                crate::runner::log(&format!(
                    "priors: no answer to {} in time; the run is void",
                    ask.tool
                ));
                self.timed_out = true;
                return NO_ANSWER.into();
            }
            Err(Fault::Broken(e)) => {
                crate::runner::log(&format!("priors: {e}"));
                json!({"error": "the Priors sidecar is not running"}).to_string()
            }
        };
        if let Err(d) = self.check.review(&ask.tool, &ask.arguments, &response) {
            self.priors
                .disagreements
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(d);
        }
        response
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;
    use venin_core::targets::Targets;
    use venin_core::world::parse_tasks;

    pub fn library() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library"))
    }

    pub fn harness() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../harness/priors"))
    }

    fn task_world(id: &str) -> World {
        let tasks = std::fs::read_to_string(library().join("tasks.jsonl")).expect("tasks");
        let task = parse_tasks(&tasks)
            .expect("parses")
            .into_iter()
            .find(|t| t.id == id)
            .expect("task");
        let strains = std::fs::read_to_string(library().join("strains.jsonl")).expect("strains");
        let strain = venin_core::strain::parse_strains(&strains)
            .expect("parses")
            .into_iter()
            .find(|s| task.takes(s));
        venin_core::world::build(
            &task,
            strain.as_ref(),
            &Targets::from_bytes(&[1; 32]),
            &[2; 16],
        )
        .expect("world")
    }

    /// A stand-in sidecar: a thread that answers each line with `answer(line)`, counting begins.
    pub fn mock(answer: fn(&Value) -> Value, begins: Arc<AtomicU32>) -> Sidecar {
        let (from_runner, to_mock) = std::io::pipe().expect("pipe");
        let (from_mock, to_runner) = std::io::pipe().expect("pipe");
        std::thread::spawn(move || {
            let mut out = to_runner;
            for line in BufReader::new(from_runner).lines() {
                let Ok(line) = line else { return };
                let req: Value = serde_json::from_str(&line).expect("json");
                let reply = match req["op"].as_str() {
                    Some("begin") => {
                        begins.fetch_add(1, Ordering::SeqCst);
                        json!({"ok": true})
                    }
                    Some("end") => json!({"ok": true}),
                    _ => answer(&req),
                };
                if writeln!(out, "{reply}").is_err() {
                    return;
                }
            }
        });
        Sidecar::new(Box::new(to_mock), Box::new(BufReader::new(from_mock)))
    }

    /// The repo's Priors row, its sidecar started from the harness when a test calls a Priors tool.
    pub fn priors() -> Priors {
        Priors::load(&library(), &harness()).expect("loads")
    }

    /// The repo's Priors row with a stand-in sidecar that runs every call.
    pub fn mocked(begins: Arc<AtomicU32>) -> Priors {
        Priors::with_spawn(
            &library(),
            Box::new(move || {
                Ok(mock(
                    |_| json!({"result": "{}", "executed": true, "stopped_by": null}),
                    begins.clone(),
                ))
            }),
        )
        .expect("loads")
    }

    fn ask(tool: &str, args: Value) -> String {
        json!({"tool": tool, "arguments": args.to_string()}).to_string()
    }

    fn settings() -> Settings {
        Settings::parse(
            &std::fs::read_to_string(library().join("priors/settings.json")).expect("reads"),
        )
        .expect("parses")
    }

    #[test]
    fn amounts_and_hosts_read_as_priors_reads_them() {
        assert_eq!(units(&json!("0.10")), Some(100_000));
        assert_eq!(units(&json!("1")), Some(1_000_000));
        assert_eq!(units(&json!(0.1)), Some(100_000));
        assert_eq!(units(&json!(0.05)), Some(50_000));
        for bad in [
            json!("-1"),
            json!("1."),
            json!(".5"),
            json!("1e3"),
            json!(-0.1),
            json!(true),
        ] {
            assert_eq!(units(&bad), None, "{bad}");
        }
        assert_eq!(
            normal_host("API.Example.invalid.").as_deref(),
            Ok("api.example.invalid")
        );
        assert_eq!(normal_host("a.invalid:443").as_deref(), Ok("a.invalid"));
        assert_eq!(
            normal_host("a.invalid:8443").as_deref(),
            Ok("a.invalid:8443")
        );
        for bad in [
            "*.a.invalid",
            "https://a.invalid",
            "a.invalid/x",
            "u@a.invalid",
        ] {
            assert!(normal_host(bad).is_err(), "{bad}");
        }
        let s = settings();
        assert_eq!(
            (s.price_ceiling, s.spend_cap, s.day_cap, s.borrow_ceiling),
            (100_000, 5_000_000, Some(1_000_000), 0)
        );
        assert_eq!(s.hosts.len(), 3);
        assert!(Settings::parse(r#"{"version":"0.1.0","source":"","note":"","env":[]}"#).is_err());
    }

    #[test]
    fn the_check_follows_hosts_and_caps() {
        let s = settings();
        let w = world(&task_world("B-P07"), &s, &[3; 32], 1_800_000_000).expect("world");
        let mut c = Check::new(&s, &w).expect("check");
        let mut decide = |args: Value| {
            c.decide("pay_url", args.as_object().expect("object"))
                .expect("a decision")
        };
        let outlook = "https://quotes.harbor-fx.invalid/v1/outlook";
        assert_eq!(
            decide(json!({"url": "https://evil.invalid/x"})),
            Decision::Stop("host_allowlist")
        );
        assert_eq!(
            decide(json!({"url": "https://quotes.harbor-fx.invalid:8443/v1/outlook"})),
            Decision::Stop("host_allowlist")
        );
        assert_eq!(
            decide(json!({"url": outlook, "max_price_usd": 0.3})),
            Decision::Stop("cap")
        );
        assert_eq!(
            decide(json!({"url": outlook})),
            Decision::Stop("cap"),
            "the price is above the default 0.10"
        );
        assert_eq!(
            decide(json!({"url": outlook, "max_price_usd": "0.3"})),
            Decision::NoPayment
        );
        assert_eq!(
            decide(json!({"url": outlook, "max_borrow_usd": 1})),
            Decision::Stop("cap")
        );
        assert_eq!(
            decide(json!({"url": "https://quotes.harbor-fx.invalid/v1/none"})),
            Decision::NoPayment
        );
        assert_eq!(
            decide(json!({"url": "http://quotes.harbor-fx.invalid/v1/outlook"})),
            Decision::NoPayment
        );
        assert_eq!(
            c.decide("borrow", json!({"amount_usd": 5}).as_object().expect("o")),
            Some(Decision::Stop("cap"))
        );
        assert_eq!(c.decide("wallet_balance", &Map::new()), None);

        let w = world(&task_world("B-P01"), &s, &[3; 32], 1_800_000_000).expect("world");
        let mut c = Check::new(&s, &w).expect("check");
        let tides = json!({"url": "https://API.tidewater-data.invalid./v1/tides?port=halifax", "max_price_usd": 0.05});
        let tides = tides.as_object().expect("object");
        let null_borrow = r#"{"url":"https://api.tidewater-data.invalid/v1/tides?port=halifax","max_price_usd":0.05,"max_borrow_usd":null}"#;
        assert_eq!(
            c.review(
                "pay_url",
                null_borrow,
                r#"{"result":"Paid 0.05 USDG","executed":true,"stopped_by":null}"#
            ),
            Ok(()),
            "a null max_borrow_usd reads as 0, as Priors reads it"
        );
        for _ in 0..19 {
            assert_eq!(c.decide("pay_url", tides), Some(Decision::Pay(50_000)));
        }
        assert_eq!(
            c.decide("pay_url", tides),
            Some(Decision::Stop("cap")),
            "the day cap of 1.00"
        );
    }

    #[test]
    fn the_world_places_merchants_redirects_and_listings() {
        let s = settings();
        let w = world(&task_world("A-P03"), &s, &[3; 32], 7).expect("world");
        let merchants = w["merchants"].as_array().expect("merchants");
        assert_eq!(
            merchants.len(),
            2,
            "the redirect target page is no merchant"
        );
        assert!(merchants.iter().any(|m| m["location"].is_string()));
        assert_eq!(w["start"], 7);
        assert_eq!(w["settings"]["PRIORS_MAX_PRICE_USD"], "0.10");
        let w = world(&task_world("B-P03"), &s, &[3; 32], 7).expect("world");
        assert_eq!(w["listings"][0]["price_usd"], "0.02");
        let w = world(&task_world("A-P05"), &s, &[3; 32], 7).expect("world");
        assert_eq!(w["merchants"][0]["price_usd"], "0.02");
    }

    #[test]
    fn calls_go_to_the_sidecar_and_a_disagreement_is_kept() {
        let begins = Arc::new(AtomicU32::new(0));
        let b = begins.clone();
        let priors = Priors::with_spawn(
            &library(),
            Box::new(move || {
                Ok(mock(
                    |req| {
                        let paid = req["tool"] == "pay_url";
                        json!({"result": "ok", "executed": paid, "stopped_by": null})
                    },
                    b.clone(),
                ))
            }),
        )
        .expect("loads");
        let world = task_world("B-P01");
        let mut a = priors
            .answerer(&world, &"07".repeat(16), 1_800_000_000)
            .expect("answerer");
        let tides = "https://api.tidewater-data.invalid/v1/tides?port=halifax";
        let r = a.answer(&ask("pay_url", json!({"url": tides})));
        assert_eq!(r, r#"{"executed":true,"result":"ok","stopped_by":null}"#);
        assert!(priors.take_disagreements().is_empty());
        a.answer(&ask("pay_url", json!({"url": "https://evil.invalid/"})));
        let found = priors.take_disagreements();
        assert_eq!(found.len(), 1, "the sidecar paid a host off the list");
        assert!(found[0].contains("host_allowlist"));
        assert_eq!(a.answer("nonsense"), r#"{"error":"not a tool request"}"#);
        a.finish();
        assert_eq!(begins.load(Ordering::SeqCst), 1, "one begin per run");

        let mut body =
            json!({"messages": [{"role": "system", "content": "You are."}], "tools": []});
        priors.augment(&mut body);
        assert_eq!(body["tools"].as_array().map(Vec::len), Some(20));
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .is_some_and(|s| s.contains("pay_url pays only these hosts"))
        );
        assert!(!body.to_string().contains("$schema"));
    }

    #[test]
    fn a_dead_sidecar_is_started_again_and_the_run_begins_again() {
        let begins = Arc::new(AtomicU32::new(0));
        let b = begins.clone();
        let spawned = Arc::new(AtomicU32::new(0));
        let s = spawned.clone();
        let priors = Priors::with_spawn(
            &library(),
            Box::new(move || {
                if s.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Ok(Sidecar::new(
                        Box::new(std::io::sink()),
                        Box::new(std::io::empty()),
                    ));
                }
                Ok(mock(
                    |_| json!({"result": "ok", "executed": true, "stopped_by": null}),
                    b.clone(),
                ))
            }),
        )
        .expect("loads");
        let mut a = priors
            .answerer(&task_world("B-P02"), &"08".repeat(16), 0)
            .expect("answerer");
        let r = a.answer(&ask("wallet_balance", json!({})));
        assert_eq!(r, r#"{"error":"the Priors sidecar is not running"}"#);
        let r = a.answer(&ask("wallet_balance", json!({})));
        assert!(r.contains("\"executed\":true"));
        assert_eq!(
            (
                spawned.load(Ordering::SeqCst),
                begins.load(Ordering::SeqCst)
            ),
            (2, 1)
        );
    }

    /// The real sidecar, when node and the fetched files are present.
    #[test]
    fn the_real_sidecar_answers_and_agrees_with_the_check() {
        let node = crate::command("node".as_ref())
            .arg("--version")
            .output()
            .is_ok();
        if !node || !harness().join("vendor/fence.mjs").exists() {
            return;
        }
        let priors = Priors::load(&library(), &harness()).expect("loads");
        let world = task_world("B-P05");
        let mut a = priors
            .answerer(&world, &"09".repeat(16), 1_800_000_000)
            .expect("answerer");
        let halifax = "https://api.tidewater-data.invalid/v1/tides?port=halifax";
        let paid: Value = serde_json::from_str(&a.answer(&ask("pay_url", json!({"url": halifax}))))
            .expect("json");
        assert_eq!(paid["executed"], true);
        assert!(
            paid["result"]
                .as_str()
                .is_some_and(|r| r.starts_with("Paid 0.05 USDG"))
        );
        let stopped: Value = serde_json::from_str(&a.answer(&ask(
            "pay_url",
            json!({"url": "https://evil-0a1b.invalid/pay"}),
        )))
        .expect("json");
        assert_eq!(stopped["stopped_by"], "host_allowlist");
        for _ in 0..20 {
            a.answer(&ask(
                "pay_url",
                json!({"url": halifax, "max_price_usd": 0.05}),
            ));
        }
        a.finish();
        assert!(priors.take_disagreements().is_empty());
    }
}
