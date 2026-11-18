//! `venin library`: build the strain library from pinned public sources, and check every library file.

mod build;
mod calibrate;
mod check;
mod derive;
mod sources;
mod text;

use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Command {
    /// Fetch the pinned sources, verify their hashes and rewrite strains.jsonl with the derived strains.
    Build {
        /// Where downloaded sources are kept; it must be outside the repository.
        #[arg(long, value_name = "DIR")]
        cache: Option<PathBuf>,
        #[arg(long, value_name = "DIR", default_value = "library")]
        library: PathBuf,
    },
    /// Run every strain once on each no-defence body of a league and record what it did. Runs already in the runs file
    /// are not repeated. `--apply` writes calibration.json and drops the strains that fooled no model.
    Calibrate {
        #[arg(long, value_name = "FILE", default_value = "deploy/prod/venin.toml")]
        config: PathBuf,
        /// One JSON line per run, appended; keep it outside the repository.
        #[arg(long, value_name = "FILE")]
        runs: PathBuf,
        /// Runs at once, across the league's models.
        #[arg(long, default_value_t = 16)]
        jobs: usize,
        /// Runs of each strain on each model; a strain stays when any of them fooled any model.
        #[arg(long, default_value_t = 2)]
        trials: u32,
        /// Only the strains whose id starts with this, for a trial.
        #[arg(long)]
        only: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Parse and validate every library file and print counts.
    Check {
        #[arg(long, value_name = "DIR", default_value = "library")]
        library: PathBuf,
    },
}

pub fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Build { cache, library } => build::run(&library, cache),
        Command::Calibrate {
            config,
            runs,
            jobs,
            trials,
            only,
            apply,
        } => calibrate::run(&config, &runs, jobs, trials, only.as_deref(), apply),
        Command::Check { library } => check::run(&library),
    }
}
