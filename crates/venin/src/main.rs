mod anchor;
mod budget;
mod cards;
mod config;
mod control;
mod draw;
mod genesis;
mod intake;
mod library;
mod model;
mod net;
mod post;
mod priors;
mod runner;
mod scan;
mod store;
mod upload;
mod verify;

use clap::{Parser, Subcommand};
use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

pub const NTFY: &str = "https://ntfy.sh/";

/// The Venin lab: the runner, its record and the strain library.
#[derive(Parser)]
#[command(name = "venin", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
struct ConfigArg {
    #[arg(long, value_name = "FILE", default_value = "deploy/dev/venin.toml")]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    /// Build or check the strain library.
    #[command(subcommand)]
    Library(library::Command),
    /// Pin and anchor the manifest, then draw and anchor the exam list. Never overwrites a genesis.
    Genesis {
        #[command(flatten)]
        config: ConfigArg,
        /// Pin the current commit even though the working tree has changes.
        #[arg(long)]
        allow_dirty: bool,
    },
    /// Run the lab, one period after another.
    Run {
        #[command(flatten)]
        config: ConfigArg,
        /// Stop after writing one period.
        #[arg(long)]
        once: bool,
    },
    /// Recompute a published record from its files and the chain.
    Verify {
        #[arg(long, value_name = "DIR")]
        data: PathBuf,
        /// The anchor chain's JSON-RPC URL; without it anchors are left unchecked.
        #[arg(long, value_name = "URL")]
        rpc: Option<String>,
        /// The draw chain's JSON-RPC URL; without it draw blocks are left unchecked.
        #[arg(long, value_name = "URL")]
        draw_rpc: Option<String>,
    },
    /// Write the card files again from the record, after a caption change. LATE notes and pictures stay as they are.
    Cards {
        #[arg(long, value_name = "DIR")]
        data: PathBuf,
    },
    /// Check an agent folder: hidden text, planted orders, wide permissions, unguarded acting and pay tools.
    Scan {
        #[arg(value_name = "DIR")]
        dir: PathBuf,
    },
    /// Show the last hour, the status file and the anchor queue.
    Status(ConfigArg),
    /// Write every period as void (paused) from the next start.
    Pause(ConfigArg),
    /// Run periods again.
    Resume(ConfigArg),
    /// The Priors row.
    #[command(subcommand)]
    Priors(PriorsCommand),
    /// Launch settings.
    #[command(subcommand)]
    Launch(Launch),
    /// Send the record's new files to its public bucket, head.json last; a timer runs it every minute.
    Upload(JobArgs),
    /// Post the latest hour's caption and card on X once, when posting is on; off, show what it would post.
    Post(JobArgs),
}

/// A timer job beside the runner: its configuration, and the folder it keeps its own state in.
#[derive(clap::Args)]
struct JobArgs {
    #[command(flatten)]
    config: ConfigArg,
    /// The job's own state folder; the configuration's state_dir when not given.
    #[arg(long, value_name = "DIR")]
    state_dir: Option<PathBuf>,
}

impl JobArgs {
    /// Runs the job, and raises an alert an hour while it fails.
    fn run(
        self,
        name: &str,
        job: fn(&config::Config, &store::Store) -> Result<(), String>,
    ) -> Result<(), String> {
        let config = config::Config::load(&self.config.config)?;
        let state = self.state_dir.unwrap_or_else(|| config.state_dir.clone());
        let store = store::Store::new(config.data_dir.clone(), state);
        job(&config, &store)
            .inspect_err(|e| alert(&config, &store, name, &format!("venin {name}: {e}")))
    }
}

#[derive(Subcommand)]
enum PriorsCommand {
    /// Bring Priors back into the ward after a sealed period took it out.
    Restore {
        /// Why it is safe to bring it back; written to the log.
        #[arg(long)]
        reason: String,
        #[command(flatten)]
        config: ConfigArg,
    },
}

#[derive(Subcommand)]
enum Launch {
    /// Record the token contract (EIP-55, the configured token, with code) and write the site's release settings.
    SetCa {
        address: String,
        #[command(flatten)]
        config: ConfigArg,
    },
}

/// Unix seconds now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

pub fn sleep_until(t: i64) {
    while now() < t {
        let left = u64::try_from(t - now()).unwrap_or(0).min(60);
        std::thread::sleep(std::time::Duration::from_secs(left.max(1)));
    }
}

/// `N` bytes from the operating system.
pub fn random<const N: usize>() -> [u8; N] {
    let mut out = [0; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut out))
        .expect("the OS random source is readable");
    out
}

/// A child process with an empty environment but `PATH`, so it inherits no keys or tokens.
pub fn command(program: &OsStr) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    c.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    c
}

/// One alert to the ntfy topic, when one is configured.
pub fn notify(net: &net::Net, topic: &str, message: &str) {
    if topic.is_empty() {
        return;
    }
    let url = format!("{NTFY}{topic}");
    if let Err(e) = net.post(
        &url,
        message.as_bytes().to_vec(),
        std::time::Duration::from_secs(10),
    ) {
        runner::log(&format!("ntfy: {e}"));
    }
}

/// Sends `message` to the configured ntfy topic at most once an hour per job, so a timer that fails every minute raises
/// one alert an hour. A marker that cannot be written (a full disk, say) leaves the alert to the run at the top of the
/// hour. Best effort: an alert that cannot be sent changes nothing.
pub fn alert(config: &config::Config, store: &store::Store, job: &str, message: &str) {
    if config.ntfy_topic.is_empty() {
        return;
    }
    let name = format!("{job}-alert.json");
    let at = now();
    if store
        .state_json::<i64>(&name)
        .ok()
        .flatten()
        .is_some_and(|last| (0..3600).contains(&(at - last)))
    {
        return;
    }
    if store.write_state_json(&name, &at).is_err() && at % 3600 >= 60 {
        return;
    }
    if let Ok(net) = net::Net::new(&[NTFY.to_string()]) {
        notify(&net, &config.ntfy_topic, message);
    }
}

fn run(command: Command) -> Result<(), String> {
    let load = |c: &ConfigArg| config::Config::load(&c.config);
    match command {
        Command::Library(command) => library::run(command),
        Command::Genesis {
            config,
            allow_dirty,
        } => genesis::run(&load(&config)?, allow_dirty),
        Command::Run { config, once } => {
            let mut config = load(&config)?;
            config.read_reader()?;
            config.openrouter_key = std::env::var(config::OPENROUTER_KEY_ENV)
                .ok()
                .filter(|k| !k.is_empty());
            runner::run(&config, once)
        }
        Command::Verify {
            data,
            rpc,
            draw_rpc,
        } => verify::run(&data, rpc.as_deref(), draw_rpc.as_deref()),
        Command::Cards { data } => cards::run(&data),
        Command::Scan { dir } => scan::run(&dir),
        Command::Status(c) => control::status(&load(&c)?),
        Command::Pause(c) => control::pause(&load(&c)?),
        Command::Resume(c) => control::resume(&load(&c)?),
        Command::Priors(PriorsCommand::Restore { reason, config }) => {
            control::restore_priors(&load(&config)?, &reason)
        }
        Command::Launch(Launch::SetCa { address, config }) => {
            control::set_ca(&load(&config)?, &address)
        }
        Command::Upload(job) => job.run("upload", upload::run),
        Command::Post(job) => job.run("post", post::run),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("venin: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
        for args in [
            &["venin", "library", "build", "--cache", "/tmp/c"][..],
            &["venin", "library", "check"],
            &["venin", "genesis", "--allow-dirty"],
            &["venin", "run", "--once", "--config", "x.toml"],
            &[
                "venin",
                "verify",
                "--data",
                "d",
                "--rpc",
                "http://127.0.0.1:1",
                "--draw-rpc",
                "http://127.0.0.1:2",
            ],
            &["venin", "cards", "--data", "d"],
            &["venin", "scan", "library/examples"],
            &["venin", "status"],
            &["venin", "pause"],
            &["venin", "resume"],
            &["venin", "priors", "restore", "--reason", "fixed"],
            &["venin", "launch", "set-ca", "0x00"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
        assert!(Cli::try_parse_from(["venin", "library", "nope"]).is_err());
        assert!(Cli::try_parse_from(["venin", "verify"]).is_err());
        assert!(Cli::try_parse_from(["venin", "cards"]).is_err());
        assert!(Cli::try_parse_from(["venin", "scan"]).is_err());
        assert!(Cli::try_parse_from(["venin", "priors", "restore"]).is_err());
    }

    #[test]
    fn children_get_no_inherited_environment() {
        let out = command(OsStr::new("env")).output().expect("runs");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.lines().all(|l| l.starts_with("PATH=")), "{text}");
        assert_ne!(random::<16>(), random::<16>());
        let t = now();
        sleep_until(t - 5);
        assert!(now() - t < 2);
    }
}
