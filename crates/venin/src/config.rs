//! The runner's configuration file (TOML). Paths may start with `~/`; relative paths are taken from the working
//! directory the runner starts in.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use venin_core::lab::{COLUMNS, Sampling};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub league: String,
    pub data_dir: PathBuf,
    /// Private runner state: the pause flag, the anchor in flight, the anchor spend log. Never served.
    pub state_dir: PathBuf,
    pub library_dir: PathBuf,
    pub period_s: u32,
    pub deadline_s: u32,
    pub late_s: u32,
    pub k: u32,
    pub exam_draw_delay_s: u32,
    pub call_timeout_s: u32,
    /// The llama-server binary of a self-hosted league; genesis pins its version.
    #[serde(default)]
    pub llama_server: Option<PathBuf>,
    pub card_script: PathBuf,
    /// The built site the card script may read.
    #[serde(default = "web_dist")]
    pub web_dist: PathBuf,
    /// The Priors sidecar's folder (`serve.mjs` and the fetched `vendor/`).
    #[serde(default = "priors_harness")]
    pub priors_harness: PathBuf,
    /// Alerts go to `https://ntfy.sh/<topic>`; empty sends none.
    pub ntfy_topic: String,
    pub host_check: HostCheck,
    pub draw: DrawChain,
    pub anchor: AnchorChain,
    /// The holder intake; without it no holder strain runs.
    #[serde(default)]
    pub release: Option<Release>,
    /// The spend governor; without it model spend is not limited.
    #[serde(default)]
    pub budget: Option<Budget>,
    /// The web build's env file, where `venin launch set-ca` writes the release settings.
    #[serde(default = "web_env")]
    pub web_env: PathBuf,
    pub models: Vec<ModelConfig>,
    /// The OpenRouter key, set from the environment at start, never from the file.
    #[serde(skip)]
    pub openrouter_key: Option<String>,
    /// The record's public bucket, for `venin upload`.
    #[serde(default)]
    pub upload: Option<Upload>,
    /// The hourly post, for `venin post`.
    #[serde(default)]
    pub post: Option<Post>,
}

/// The record's public bucket. Its endpoint and keys come from the environment (`upload::ENDPOINT_ENV` and the
/// others), so no account id sits in the repository.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upload {
    pub bucket: String,
    /// The signing region; R2 signs with "auto".
    #[serde(default = "auto_region")]
    pub region: String,
}

/// The hourly post on X. Its switch and keys come from the environment (`post::SWITCH_ENV` and the others), so turning
/// it on changes nothing in the repository.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Post {
    #[serde(default = "x_api")]
    pub api: String,
}

fn auto_region() -> String {
    "auto".into()
}

fn x_api() -> String {
    "https://api.x.com".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Micro-USD a day; each period adds a 24th of it to the allowance.
    pub daily_cap_micro: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    /// The Worker's `GET /api/queue` URL. The bearer token comes from the environment, never from this file.
    pub queue_url: String,
    /// Holder strains a period runs at most, up to 6.
    pub slots: u32,
    /// The token's chain; burns and signatures are read for it.
    pub chain_id: u64,
    /// The token contract.
    pub token: String,
    /// The burn amount in the token's smallest unit, as a decimal string.
    pub amount: String,
    pub rpcs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCheck {
    pub enabled: bool,
    pub min_free_percent: u32,
    /// A file naming a service this machine also runs, kept with the machine's other local files rather than in the
    /// repository. Without the file the check reads memory only.
    #[serde(default)]
    pub reader_file: Option<PathBuf>,
    /// Read from `reader_file` by [`Config::read_reader`], for the runner only.
    #[serde(skip)]
    pub reader: Option<Reader>,
}

/// A local service model work gives way to: `url` must answer 200 within 2 s with a JSON object whose `time_key` holds
/// a time in seconds since the epoch at most `fresh_s` old.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reader {
    pub url: String,
    pub time_key: String,
    pub fresh_s: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawChain {
    pub chain_id: u64,
    pub rpcs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorChain {
    pub chain_id: u64,
    pub rpcs: Vec<String>,
    pub key_file: PathBuf,
    pub gas_limit_max: u64,
    /// Wei, as a decimal string.
    pub max_fee_per_gas_max: String,
    /// Wei per rolling 24 hours, as a decimal string.
    pub daily_cap_wei: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub prompt: u64,
    pub completion: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub column: String,
    pub id: String,
    pub label: String,
    pub served_by: String,
    /// The OpenAI-compatible base URL; requests go to `<endpoint>/chat/completions`.
    pub endpoint: String,
    pub concurrency: u32,
    /// The weights of a self-hosted model; genesis pins their SHA-256.
    #[serde(default)]
    pub gguf: Option<PathBuf>,
    /// Set for a model served through OpenRouter.
    #[serde(default)]
    pub openrouter: Option<OpenRouter>,
    /// Micro-USD per million tokens.
    pub price_micro_per_mtok: Price,
    pub sampling: Sampling,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRouter {
    /// The one provider allowed to serve the model, as OpenRouter names it.
    pub provider: String,
}

/// Where the OpenRouter key comes from.
pub const OPENROUTER_KEY_ENV: &str = "VENIN_OPENROUTER_KEY";
/// An alert topic that takes the place of the configuration's `ntfy_topic`.
pub const NTFY_TOPIC_ENV: &str = "VENIN_NTFY_TOPIC";

fn web_env() -> PathBuf {
    PathBuf::from("web/.env.production.local")
}

fn web_dist() -> PathBuf {
    PathBuf::from("web/dist")
}

fn priors_harness() -> PathBuf {
    PathBuf::from("harness/priors")
}

fn expand(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => path.to_path_buf(),
    }
}

pub fn wei(s: &str) -> Result<u128, String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{s:?} is not a decimal number of wei"));
    }
    s.parse().map_err(|e| format!("{s}: {e}"))
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        for p in [
            &mut c.data_dir,
            &mut c.state_dir,
            &mut c.library_dir,
            &mut c.card_script,
            &mut c.web_dist,
            &mut c.web_env,
            &mut c.priors_harness,
            &mut c.anchor.key_file,
        ] {
            *p = expand(p);
        }
        let optional = c.models.iter_mut().map(|m| &mut m.gguf);
        for p in optional
            .chain([&mut c.llama_server, &mut c.host_check.reader_file])
            .flatten()
        {
            *p = expand(p);
        }
        c.check()?;
        Ok(c)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut config = Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        // The alert topic works like a password, so a published configuration leaves it to the environment.
        if let Some(topic) = std::env::var(NTFY_TOPIC_ENV).ok().filter(|t| !t.is_empty()) {
            config.ntfy_topic = topic;
        }
        Ok(config)
    }

    /// Reads the machine's reader file into the host check, when the check is on and the file exists. Only the runner
    /// calls this, so status, pause and the other commands never depend on that file. A file that does not parse, or
    /// names a URL the client cannot use, is refused.
    pub fn read_reader(&mut self) -> Result<(), String> {
        let hc = &mut self.host_check;
        let Some(file) = hc.reader_file.as_deref().filter(|_| hc.enabled) else {
            return Ok(());
        };
        let at = |e: &dyn std::fmt::Display| format!("{}: {e}", file.display());
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(at(&e)),
        };
        let reader: Reader = toml::from_str(&text).map_err(|e| at(&e))?;
        crate::net::origin(&reader.url).map_err(|e| at(&e))?;
        hc.reader = Some(reader);
        Ok(())
    }

    fn check(&self) -> Result<(), String> {
        if self.period_s == 0 || self.deadline_s == 0 || self.deadline_s > self.period_s {
            return Err("deadline_s must lie inside the period".into());
        }
        let columns: Vec<&str> = self.models.iter().map(|m| m.column.as_str()).collect();
        if columns != COLUMNS {
            return Err("models must be one per column, m1 to m4 in order".into());
        }
        if let Some(m) = self.models.iter().find(|m| m.concurrency == 0) {
            return Err(format!("model {}: concurrency must be at least 1", m.id));
        }
        if let Some(m) = self
            .models
            .iter()
            .find(|m| m.gguf.is_some() == m.openrouter.is_some())
        {
            return Err(format!("model {}: set either gguf or openrouter", m.id));
        }
        if let Some(m) = self.models.iter().find(|m| {
            m.openrouter.as_ref().is_some_and(|o| {
                venin_core::replay::pinned_provider(&m.served_by) != Some(o.provider.as_str())
            })
        }) {
            return Err(format!(
                "model {}: served_by must end in \"provider <openrouter.provider>\"",
                m.id
            ));
        }
        let hosted = self.hosted();
        if self.models.iter().any(|m| m.openrouter.is_some() != hosted) {
            return Err("a league is either self-hosted or served through OpenRouter".into());
        }
        if !hosted && self.llama_server.is_none() {
            return Err("a self-hosted league needs llama_server".into());
        }
        if self.budget.as_ref().is_some_and(|b| b.daily_cap_micro == 0) {
            return Err("budget: daily_cap_micro must be above 0".into());
        }
        if self.draw.rpcs.is_empty() || self.anchor.rpcs.is_empty() {
            return Err("the draw and anchor chains need at least one RPC each".into());
        }
        wei(&self.anchor.max_fee_per_gas_max)?;
        wei(&self.anchor.daily_cap_wei)?;
        if let Some(r) = &self.release {
            if r.slots > 6 || r.rpcs.is_empty() {
                return Err("release: slots must be 0 to 6 and rpcs at least one".into());
            }
            wei(&r.amount)?;
        }
        for url in self.urls() {
            crate::net::origin(&url)?;
        }
        if let Some(post) = &self.post {
            crate::net::origin(&post.api)?;
        }
        Ok(())
    }

    /// Every URL the runner may reach. The HTTP client refuses any other origin.
    pub fn urls(&self) -> Vec<String> {
        let mut urls: Vec<String> = self.draw.rpcs.clone();
        urls.extend(self.anchor.rpcs.iter().cloned());
        urls.extend(self.models.iter().map(|m| m.endpoint.clone()));
        if self.host_check.enabled {
            urls.extend(self.host_check.reader.iter().map(|r| r.url.clone()));
        }
        if !self.ntfy_topic.is_empty() {
            urls.push(crate::NTFY.into());
        }
        if let Some(r) = &self.release {
            urls.push(r.queue_url.clone());
            urls.extend(r.rpcs.iter().cloned());
        }
        urls
    }

    /// True when the league's models are served through OpenRouter.
    pub fn hosted(&self) -> bool {
        self.models.iter().any(|m| m.openrouter.is_some())
    }

    pub fn model(&self, id: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.id == id)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn text(data: &Path, rpc: &str, model: &str) -> String {
        let models: String = COLUMNS
            .iter()
            .map(|c| {
                format!(
                    "[[models]]\ncolumn = \"{c}\"\nid = \"model-{c}\"\nlabel = \"Model {c}\"\n\
                     served_by = \"test\"\nendpoint = \"{model}\"\nconcurrency = 2\ngguf = \"/nonexistent\"\n\
                     price_micro_per_mtok = {{ prompt = 1000000, completion = 2000000 }}\n\
                     sampling = {{ temperature_milli = 700, top_p_milli = 950, top_k = 40, max_tokens = 512 }}\n"
                )
            })
            .collect();
        format!(
            "league = \"test league\"\ndata_dir = \"{d}/data\"\nstate_dir = \"{d}/state\"\n\
             library_dir = \"{lib}\"\nperiod_s = 3600\ndeadline_s = 3000\nlate_s = 300\nk = 8\n\
             exam_draw_delay_s = 0\ncall_timeout_s = 10\nllama_server = \"/nonexistent\"\n\
             card_script = \"{d}/card.mjs\"\nweb_dist = \"{d}/dist\"\nntfy_topic = \"\"\n\
             [host_check]\nenabled = false\nmin_free_percent = 20\n\
             [draw]\nchain_id = 31337\nrpcs = [\"{rpc}\", \"{rpc}/second\"]\n\
             [anchor]\nchain_id = 31337\nrpcs = [\"{rpc}\"]\nkey_file = \"{d}/anchor.key\"\n\
             gas_limit_max = 40000\nmax_fee_per_gas_max = \"100000000000\"\n\
             daily_cap_wei = \"1000000000000000000\"\n{models}",
            d = data.display(),
            lib = concat!(env!("CARGO_MANIFEST_DIR"), "/../../library"),
        )
    }

    #[test]
    fn the_dev_config_parses() {
        let text = include_str!("../../../deploy/dev/venin.toml");
        let c = Config::parse(text).expect("parses");
        assert_eq!(c.anchor.chain_id, 31337);
        assert_eq!(c.draw.chain_id, 4663);
        assert!(c.data_dir.is_absolute());
        assert_eq!(c.models.len(), 4);
        assert_eq!(c.web_dist, Path::new("web/dist"));
        assert_eq!(c.priors_harness, Path::new("harness/priors"));
        let release = c.release.as_ref().expect("a release section");
        assert_eq!((release.slots, release.chain_id), (0, 31337));
        assert!(c.urls().contains(&release.queue_url));
        let reader = c.host_check.reader_file.as_ref().expect("a reader file");
        assert!(reader.is_absolute() && c.host_check.reader.is_none());
    }

    #[test]
    fn the_prod_config_parses_as_a_hosted_league_with_a_budget() {
        let c = Config::parse(include_str!("../../../deploy/prod/venin.toml")).expect("parses");
        assert!(c.hosted());
        assert_eq!((c.anchor.chain_id, c.draw.chain_id), (4663, 4663));
        assert_eq!(
            c.budget.as_ref().map(|b| b.daily_cap_micro),
            Some(5_000_000)
        );
        assert!(c.release.is_none() && !c.host_check.enabled && c.llama_server.is_none());
        assert_eq!(
            c.upload
                .as_ref()
                .map(|u| (u.bucket.as_str(), u.region.as_str())),
            Some(("venin-record", "auto"))
        );
        assert!(
            c.post
                .as_ref()
                .is_some_and(|p| p.api == "https://api.x.com")
        );
        assert!(c.models.iter().all(|m| {
            m.gguf.is_none()
                && m.endpoint == "https://openrouter.ai/api/v1"
                && m.served_by
                    .ends_with(&m.openrouter.as_ref().expect("pinned").provider)
        }));
        assert!(c.urls().iter().all(|u| u.starts_with("https://")));
        assert_eq!(
            crate::genesis::model_pins(&c).expect("pins"),
            ("openrouter".to_string(), vec![String::new(); 4])
        );
        assert!(
            !Config::parse(include_str!("../../../deploy/dev/venin.toml"))
                .expect("parses")
                .hosted()
        );
    }

    #[test]
    fn an_openrouter_model_names_its_pinned_provider_in_served_by() {
        let prod = include_str!("../../../deploy/prod/venin.toml");
        let wrong = prod.replacen(
            "served_by = \"OpenRouter, provider DeepInfra\"",
            "served_by = \"OpenRouter, provider Together\"",
            1,
        );
        assert_ne!(wrong, prod);
        assert!(Config::parse(&wrong).is_err_and(|e| e.contains("served_by")));
    }

    #[test]
    fn bad_configs_are_refused() {
        let good = text(
            Path::new("/tmp/x"),
            "http://127.0.0.1:1",
            "http://127.0.0.1:2/v1",
        );
        assert!(Config::parse(&good).is_ok());
        for (from, to) in [
            ("deadline_s = 3000", "deadline_s = 4000"),
            ("column = \"m4\"", "column = \"m5\""),
            ("concurrency = 2", "concurrency = 0"),
            ("\"100000000000\"", "\"1e11\""),
            ("http://127.0.0.1:2/v1", "ftp://127.0.0.1:2/v1"),
            ("k = 8", "k = 8\nextra = 1"),
            (
                "gguf = \"/nonexistent\"",
                "openrouter = { provider = \"x\" }",
            ),
            (
                "gguf = \"/nonexistent\"",
                "gguf = \"/a\"\nopenrouter = { provider = \"x\" }",
            ),
            ("llama_server = \"/nonexistent\"\n", ""),
            ("[draw]", "[budget]\ndaily_cap_micro = 0\n[draw]"),
            (
                "[draw]",
                "[release]\nqueue_url = \"http://127.0.0.1:3/q\"\nslots = 7\nchain_id = 1\ntoken = \"0x00\"\namount = \"1\"\nrpcs = [\"http://127.0.0.1:3\"]\n[draw]",
            ),
            (
                "[draw]",
                "[release]\nqueue_url = \"http://127.0.0.1:3/q\"\nslots = 1\nchain_id = 1\ntoken = \"0x00\"\namount = \"1e3\"\nrpcs = [\"http://127.0.0.1:3\"]\n[draw]",
            ),
        ] {
            assert!(Config::parse(&good.replacen(from, to, 1)).is_err(), "{to}");
        }
    }

    #[test]
    fn home_is_expanded() {
        let home = std::env::var("HOME").expect("HOME");
        assert_eq!(expand(Path::new("~/a")), Path::new(&home).join("a"));
        assert_eq!(expand(Path::new("/a")), Path::new("/a"));
        assert_eq!(wei("12").expect("wei"), 12);
        assert!(wei("-1").is_err());
    }

    #[test]
    fn the_reader_comes_from_its_own_file_and_only_when_present() {
        let dir = crate::anchor::tests::temp("reader");
        let file = dir.join("reader.toml");
        let config = dir.join("venin.toml");
        let on = format!("enabled = true\nreader_file = \"{}\"\n", file.display());
        let good = text(&dir, "http://127.0.0.1:1", "http://127.0.0.1:2/v1");
        std::fs::write(&config, good.replacen("enabled = false\n", &on, 1)).expect("write");
        let read = || {
            let mut c = Config::load(&config)?;
            c.read_reader().map(|()| c)
        };
        let c = read().expect("no reader file: memory only");
        assert!(c.host_check.enabled && c.host_check.reader.is_none());
        let reader = "url = \"http://127.0.0.1:3/now\"\ntime_key = \"updated\"\nfresh_s = 300\n";
        std::fs::write(&file, reader).expect("write");
        let c = read().expect("loads");
        assert_eq!(
            c.host_check
                .reader
                .as_ref()
                .map(|r| (r.time_key.as_str(), r.fresh_s)),
            Some(("updated", 300))
        );
        assert!(c.urls().contains(&"http://127.0.0.1:3/now".to_string()));
        std::fs::write(&file, reader.replace("http:", "ftp:")).expect("write");
        let e = read().expect_err("a URL the client cannot use");
        assert!(e.starts_with(&file.display().to_string()), "{e}");
        assert!(
            Config::load(&config).is_ok(),
            "status and pause never read the file"
        );
        std::fs::write(&file, format!("{reader}extra = 1\n")).expect("write");
        assert!(read().is_err(), "an unknown field");
        let off = format!("enabled = false\nreader_file = \"{}\"\n", file.display());
        std::fs::write(&config, good.replacen("enabled = false\n", &off, 1)).expect("write");
        let c = read().expect("check off: the broken file is never read");
        assert!(c.host_check.reader.is_none());
    }
}
