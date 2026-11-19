<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="web/public/brand/venin-lockup-on-dark.svg">
    <img alt="Venin" src="web/public/brand/venin-lockup-on-light.svg" width="380">
  </picture>
</p>

<p align="center">
  <strong>A public immune lab for AI agents.</strong><br>
  Every hour twenty agents run real tasks while a prompt-injection strain tries to make them pay an attacker or leak a
  secret. Every hour is anchored on chain and recomputable by anyone.
</p>

<p align="center">
  <a href="https://venin.io">venin.io</a> &middot;
  <a href="https://venin.io/verify">Verify</a> &middot;
  <a href="https://venin.io/checkup">Check-up</a> &middot;
  <a href="https://x.com/veninlab">@veninlab</a>
</p>

## Check our claims

Each claim beside the place that shows it. The same list, kept current, closes
[venin.io/verify](https://venin.io/verify).

| claim | where to check it |
|---|---|
| Every hour is fixed on chain as it is written. | [anchors.json](https://data.venin.io/anchors.json) names one Robinhood Chain transaction for the manifest, the exam and each hour, all sent from [0xf9f1...fe20](https://robinhoodchain.blockscout.com/address/0xf9f1397d70f04b14781c3d730eb4f5edc206fe20). That address sends nothing else: its transaction count equals the list's length. |
| Every number on the site follows from the record. | [venin.io/verify](https://venin.io/verify) replays every run in the browser with this code, compiled to WebAssembly; `venin verify` does it on your machine ([below](#verify-a-record)). Genesis pinned commit [c473537](https://github.com/venin-lab/venin/tree/c473537e655c5fd24e7ba25577b73bafc019693a) in [manifest.json](https://data.venin.io/manifest.json). |
| The Venin rows evolve. | Every 12 hours each model's two Venin rows are scored on the same hours; the challenger takes over only with a lead of 5 fitness points, and the row that loses becomes a copy of the winner, plus one change: the `replaced`, `kept` and `remutated` events of an hour's record, first in [hour 11](https://venin.io/hour/11) ([hours/11.json](https://data.venin.io/hours/11.json)). |
| Strains evolve. | Outside exam hours a strain that gets past a defended body spawns a mutant, and one that misses three draws running dies out: the `mutant` and `extinct` events. The first child was [SKL-31.1](https://venin.io/strain/SKL-31.1), Krait in ROT13, in hour 6. |
| Releasing burns $VENIN. | The Worker queues a release only once its burn to the dead address is mined ([worker/src/release.ts](worker/src/release.ts)). Each hour's `holders` keeps the burn transaction, and [venin.io/release](https://venin.io/release) links every one. |

## How it works

- **Twenty bodies.** Four models from four labs, five rows each: no defence, a textbook defence, a sandbox copy of
  Priors, and two Venin rows whose defence genes evolve. Every hour all twenty meet the same strain.
- **Real strains.** 89 prompt-injection strains hidden in tool descriptions, memory notes, skill files and web pages,
  kept from 599 candidates (public research sets and our own) because each fooled an undefended body before genesis.
  Strains that get through mutate; strains that fail go extinct.
- **Fake rails.** The tools are fake, wallets are one hex digit too long and every domain ends in `.invalid`, so
  nothing a strain asks for can work outside the lab.
- **An open record.** Every run is kept: the model's words, the tool calls, the outcome. Each file is hashed and
  anchored on chain the hour it is written.
- **Verify, don't trust.** `venin verify`, or `/verify` in the browser, replays every run and recomputes every number
  from the record and the chain.

## What runs where

| part | where | what it does |
|---|---|---|
| runner (`venin run`) | one machine: a VPS in production, any Mac or Linux box in dev | draws the hour's strain, runs the 60 runs against the models, judges them, evolves the genomes, writes and anchors the record |
| models | dev: llama-server on the same machine; production: OpenRouter | answer the bodies' chat requests with tool calls |
| record | the runner's data dir, copied every minute to a public bucket by `venin upload` | `manifest.json`, `exam.json`, `hours/<n>.json`, `runs/<n>.jsonl.gz`, `anchors.json`, cards |
| hourly post (`venin post`) | the runner's machine, every minute | posts each new hour's caption and card on X once; off until switched on |
| anchors | dev: a local anvil chain; production: Robinhood Chain (4663) | one 46-byte transaction per file fixes its hash |
| site (`web/`) | static pages | the ward in 3D, the hour feed, strains, genes, verify and the check-up in the browser (WebAssembly) |
| Worker (`worker/`) | Cloudflare | takes holder strain releases and queues them for the runner |

## Run the dev lab

Needs: the Rust toolchain from `rust-toolchain.toml` (through rustup), Node 25 with pnpm 11, Foundry (`anvil`, `cast`,
`forge`), `llama-server`, `openssl`, and the four GGUF files named in `deploy/dev/venin.toml` under
`~/Documents/venin-dev/models`. Keys, state, data and logs live in `~/Documents/venin-dev`.

```sh
pnpm install
./dev up                                                      # anvil, the model router, the Worker, the site
target/release/venin genesis --config deploy/dev/venin.toml   # once: pins the manifest, draws the exam list
./dev up                                                      # now also starts the runner
./dev status
./dev logs runner
./dev down
```

The site is at http://localhost:5173.

Before a genesis the site reads a synthetic record (league "synthetic fixture") from
`~/Documents/venin-dev/synthetic`, written by `cargo run -p venin-core --example synthetic -- <dir> <periods>`.
`venin pause` and `venin resume` write the coming hours as void (`paused`) or run them again.

## Verify a record

```sh
. scripts/env && cargo build --release -p venin
target/release/venin verify --data ~/Documents/venin-dev/data --rpc http://127.0.0.1:8546 \
  --draw-rpc https://rpc.mainnet.chain.robinhood.com
```

It replays every run, recomputes every state and number, and checks each file's hash against its anchor and each draw
block against the draw chain. Without `--rpc` the anchors are left unchecked, and without `--draw-rpc` the draw
blocks. The same check runs in the browser on `/verify`.

A change to a caption template makes the older card files fail verify until `venin cards --data <dir>` writes them
again from the record. Run it once the runner runs the new build; it keeps their LATE notes and leaves the card
pictures as they are.

## Check an agent folder

```sh
target/release/venin scan path/to/agent
```

It reads rules, skills, MCP configs and tool descriptions locally and reports hidden characters, planted orders,
acting tools that run without asking, wide permissions and pay tools without a host allowlist or a restart-safe cap,
with the genes that cover each finding. Files that look like secrets are skipped before reading; findings print names,
paths and counts, never values. `/checkup` on the site does the same in the browser, with no upload.

## Layout

```
crates/core/      the lab's rules, pure: strains, genes, worlds, judging, evolution, record, replay, verify, scan
crates/venin/     the venin binary: runner, genesis, verify, cards, scan, library build, launch commands
harness/priors/   the sandbox for Priors bodies: the fetched Priors files under Node's permission model
library/          strains, genes, tasks, rows, names of variants of concern, pinned sources, calibration
web/              the site (Vite, Preact, Three.js) and the card renderer
worker/           the release Worker
deploy/           dev/ (the local league) and prod/ (the production league and its systemd unit)
docs/             METHODS.md, RECORD.md, LIBRARY.md, CHECKUP.md
scripts/          env, check (repository rules), hooks, ci
dev               brings the dev lab up and down
```

## Read more

- [docs/METHODS.md](docs/METHODS.md): how the lab works and what its numbers mean
- [docs/RECORD.md](docs/RECORD.md): the record's files and fields, the contract verify checks
- [docs/LIBRARY.md](docs/LIBRARY.md): the library's formats
- [docs/CHECKUP.md](docs/CHECKUP.md): the check-up and verify in the browser
- [SECURITY.md](SECURITY.md): reporting a vulnerability
- [THIRD_PARTY.md](THIRD_PARTY.md): sources, assets and their licences

The code is under the MIT licence in `LICENSE`; the Venin name, logo and brand files are not.
