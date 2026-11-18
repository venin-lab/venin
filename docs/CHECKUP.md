# The check-up and verify in the browser

Both pages run `venin-core` compiled to WebAssembly. The same code backs `venin scan <dir>` and `venin verify`.

## The check-up

`/checkup` opens on the scan of `library/examples/sample-agent.json`, a made-up bookkeeping agent, labelled as a
sample. A folder dropped on it or picked with "Pick a folder" is read in the browser only:

- files are triaged by name and size before any byte is read: `.env*`, `*.pem`, any name containing `key`, keystores
  (`*.jks`, `*.keystore`, `*.p12`, `*.pfx`, `*.kdbx`, `UTC--*`, anything under a `keystore` folder) and SSH private
  keys are listed as left unread; `.git`, `node_modules`, `target`, `.venv` and `__pycache__` are passed by; only text
  files up to 2,000,000 bytes are read;
- when the page opens it adds `connect-src 'self'` to the document's Content Security Policy, so nothing the page reads
  can be sent anywhere; leaving the page reloads it when the record is served from another origin.

`venin scan <dir>` applies the same triage on disk and prints one line per finding, the files left unread, and counts
per organ. Neither prints a secret value: environment values are never shown, and anything in a finding that looks like
a key or token is cut to its first four characters.

### Findings

Each finding has an organ, a check, a kind, a file and line (0 when it belongs to the whole file), one plain sentence,
and the ids of the genes in `library/genes.json` that cover it. The page shows each gene's block rate from the current
record.

| check | what | organ | genes |
|---|---|---|---|
| `hidden` | characters that show nothing but reach the model; Unicode tags, variation-selector bytes and zero-width bits (U+200B as 0, U+200C as 1) are decoded and the text they spell is quoted | where the text lives | S03, I09 |
| `comment` | an HTML comment, a markdown link definition or text pushed 80+ spaces off screen that gives the model orders | where the text lives | S04, I08 |
| `order` | orders planted in rule, note, skill or tool text: ignore other instructions, keep something from you, hand over or read secret files, send data out, copy your words; in tools also baiting field names and shadowing of another tool | where the text lives | by kind |
| `actor` | a tool that sends, pays, deletes, publishes or runs code and runs with no one asking: freed by an allow rule or bypass mode with no deny or ask rule, or by the server's own auto-approve setting | hands | S09, S10, I04 |
| `wide` | an allow rule, a skill's `allowed-tools`, an extra working folder or a setting that lets too much run without asking | hands | by what it frees |
| `pay` | a pay tool whose server has no host or payee allowlist, no spending cap, or a cap with no state file to survive a restart | heart | S01, S08, I06 / S02, I06 |

Organs follow where the text enters: MCP tool and server text the eyes, skill folders the hands, other markdown the
brain, other text files the ears. Planted orders and comments in tool or skill text also list S06.

Settings follow Claude Code's scopes: `settings.json` at the folder root is user scope, `.claude/settings.json`
project and `.claude/settings.local.json` local; a project or local file cannot switch bypass mode on, and
`.vscode/settings.json` is read for the editor's permission mode. MCP servers come from any JSON file with an
`mcpServers` object; a server's tool list is read from its `tools` array and its note from `instructions`.

Known limit: detectors are English only, and a tool list is known only when the config carries it.

## Verify

`/verify` reads `manifest.json`, `exam.json`, `anchors.json`, `head.json`, every `hours/<n>.json`, `takedowns.json`
when present, the library files, and for the periods in scope `runs/<n>.jsonl.gz` and `cards/<n>.json`. The draw
blocks the runner saved (`draws/<n>.json`) are never taken as chain facts. The default scope is the last 24 periods and
a random sample of 8 earlier ones; "Replay every period" takes all. A period out of scope is still checked by hash chain
and anchor; it is reported as not replayed, never as a mismatch. A runs file asked for and not found is a mismatch
unless taken down; a card file not found is not compared, as in `venin verify`. The recomputed headline counts only
replayed periods, so when some were not replayed the page names the exam periods it counts (`headline_hours` of the
report: replayed, on time, not sealed and at most 10% void) and how many exam periods were not replayed.

Chain checks run only on "Check anchors on chain": the anchor transactions of the scoped periods, the manifest and exam
anchors and the anchor address's nonce, about one request a second, from `VITE_ANCHOR_RPC` (a comma-separated list
tried in order) and, when the page is served from this machine, the dev anvil at `http://127.0.0.1:8546`; then, when
`VITE_DRAW_RPC` names the draw chain's RPCs, the exam draw block and the scoped periods' draw blocks with their parents.
An anchor the chain does not know, or one it has not mined other than the latest hour's, is a mismatch. Without a draw
RPC the draw blocks count as unchecked. The synthetic league reads its `chain.json` instead.

## WebAssembly exports

All take and return JSON text; input that does not parse returns `{"error": "..."}`.

| export | in | out |
|---|---|---|
| `verify(records, bundles, chain, library)` | `records`: `{manifest, exam, anchors, head, hours: {"<n>": text}, cards: {"<n>": text or null}, takedowns}`; `bundles`: `{"<n>": runs text or null}`; `chain`: `{nonce, txs, blocks, pending, absent}` as `venin_core::verify::Chain`; `library`: `{strains, tasks, genes, rows, names}` texts | `{report, ok, replayed, not_loaded}` |
| `replay(lab, mutants, line)` | `lab`: `{manifest, exam, library}`; `mutants`: strains; `line`: a run line | `{ok, error?}` |
| `scan(files)` | `{files: [{path, content}]}` | `{read, skipped, findings}` |
| `gate(payload, route)` | a holder payload and `EYE`, `EAR`, `MEM` or `SKL` | `{ok, reason, strain_preview, organ}` |
| `mutate(strain, operator, seed_hex)` | a strain, a snake_case operator, 32 bytes of hex | the child strain; the seed picks its variant, 1 to 9999 |

`pnpm dev`, `pnpm build`, `pnpm check` and `pnpm shots` build the module first with `web/scripts/wasm.mjs` into
`web/src/wasm/` (not tracked). The build fails above 1.5 MB gzipped or when `scripts/check artifacts` finds a home path
or host name in the output.
