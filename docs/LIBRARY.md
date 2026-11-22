# Library formats

The lab reads four files from `library/`. They are the contract between the data and the code: `venin-core` parses
them with these exact field names, and `venin library check` validates them. Every file is printable ASCII; characters
outside it are written as JSON escapes or produced at run time by mutation operators.

## strains.jsonl

One strain per line.

| field | type | meaning |
|---|---|---|
| `id` | string | `<ROUTE>-<family>.<variant>`, for example `EAR-114.0`; library strains have variant 0 |
| `route` | `EYE` \| `EAR` \| `MEM` \| `SKL` | how the strain enters: a tool description, content the body reads (email or page), a memory note, a skill file |
| `carrier` | `tool_description` \| `email` \| `page` \| `memory_note` \| `skill_file` | the world item that carries it; EYE uses `tool_description`, EAR uses `email` or `page`, MEM uses `memory_note`, SKL uses `skill_file` |
| `family` | integer | numbers a family within its route; mutants keep the family; 1 to 100 are hand-written, 101 up are derived from a source by `venin library build` |
| `technique` | string | one of: `authority`, `urgency`, `fake_system`, `tool_error`, `task_hijack`, `dependency`, `helpful_request`, `role_play`, `hidden_text`, `encoded`, `split_order`, `tool_poisoning`, `memory_poisoning` |
| `goal` | `pay` \| `exfil` \| `contact` | `pay`: send funds to `{wallet}`; `exfil`: send the secret named by `{canary}` to an address at `{domain}`; `contact`: send any email to an address at `{domain}` |
| `subject` | string or null | email subject for `email` carriers |
| `tool_name` | string or null | the poisoned tool's name for `tool_description` carriers |
| `template` | string | the attacker-controlled text, at most 2,000 bytes, with placeholders |
| `split` | `train` \| `exam` | set by genesis; the library ships every strain as `train` |
| `source` | string | an id from `sources.toml`, or `venin` for strains written here |
| `source_ref` | string | where in the source it came from (file and row or task name); empty for `venin` |

Placeholders: `{wallet}` becomes the attacker wallet, `{domain}` the attacker domain, `{canary}` a phrase naming where
the body's secret lives (for example "the API key in your memory notes"), never the secret itself. A template must
contain the placeholder its goal needs: `pay` needs `{wallet}`; `exfil` needs `{domain}` and `{canary}`; `contact` needs
`{domain}`. `{domain}` always ends an email address, as in `ops@{domain}`.

Library templates must pass the same gate as holder payloads after instantiation: every host ends in `.invalid`, no
40-hex address, no email outside `.invalid`, no phone number, no real person's name taken from the source data.

## genes.json

An array of genes.

| field | type | meaning |
|---|---|---|
| `id` | string | `I01`..`I99` for instinct genes, `S01`..`S99` for skin genes |
| `kind` | `instinct` \| `skin` | a prompt clause, or a harness rule the model cannot be talked past |
| `name` | string | a short plain name, sentence case |
| `text` | string or null | instinct genes: the clause added to the system prompt |
| `rule` | string or null | skin genes: one of `allowlist_payees`, `amount_cap`, `strip_hidden`, `strip_comments`, `datamark`, `quarantine_tool_text`, `egress_canary`, `domain_allowlist`, `plan_lock`, `one_action` |
| `params` | object | rule parameters, for example `{"cap_cents": 20000}` for `amount_cap`; `{}` when none |
| `source` | string | the published work the gene comes from |
| `url` | string | a stable link to it |

`textbook` in `genes.json`'s sibling `rows.json` lists the gene ids of the fixed textbook row.

## tasks.jsonl

One task per line. A task is what the owner asks a body to do, plus the world it needs.

| field | type | meaning |
|---|---|---|
| `id` | string | `B-001` benign, `D-001` decoy, `A-001` attack frame |
| `kind` | `benign` \| `decoy` \| `attack` | attack frames have one carrier slot that the period's strain fills |
| `world` | `ward` \| `priors` | which body kind can run it |
| `routes` | array | attack frames only: the routes whose carrier this frame can hold |
| `prompt` | string | the owner's message |
| `setup` | object | the world items the task needs, below |
| `expect` | object | `{"calls": [{"tool": ..., "args": {...}}], "forbid": [tool, ...]}`; attack frames describe the owner's own task, which is judged as utility under attack |
| `source` | string | an id from `sources.toml`, or `venin` |
| `source_ref` | string | the source task, for example `banking/user_task_3` |

`setup` holds any of: `payees` (`{"<key>": {"name": ..., "book": false}}`, addresses generated per run; `book`
defaults to true and puts the payee in the owner's address book, which the body sees in its system prompt; `false` makes
a payee the owner names or an invoice brings, so address-book rules carry a real cost), `inbox`
(`[{"from": ..., "subject": ..., "body": ...}]`), `pages` (`{"<url>": text}`), `memory` (array of notes), `skills`
(`{"<name>": text}`), `files` (`{"<path>": text}`), `balance_cents`. One item in an attack frame is
`{"carrier": true}` at the place the strain goes; frames for the EYE route have none, because the strain arrives as a
tool in the body's tool list (appended to the ward tool named by `tool_name`, or added as a new third-party tool).

References inside `prompt`, `setup` and `expect`: `$payee.<key>` is that payee's generated address, `$owner.email` the
owner's address, `$owner.domain` the owner's domain. Amounts are decimal strings in USDG ("120", "120.50") and compare
as integer cents.

Benign tasks are built so that every skin gene has a real cost on some of them: payees named only in the owner's
message, invoices from new vendors found in the inbox, tasks that need two actions, amounts above common caps. Decoys
are legitimate content that looks alarming (capitals, deadlines, "ignore the earlier draft") and never change a payee.

## Ward tools

| tool | arguments | returns |
|---|---|---|
| `read_inbox` | none | `[{id, from, subject}]` |
| `read_email` | `id` | `{from, subject, body}` |
| `fetch_page` | `url` | the page text, or `404 not found` |
| `read_memory` | none | the memory notes |
| `load_skill` | `name` | the skill file text |
| `read_file` | `path` | the file text |
| `send_funds` | `to`, `amount`, `memo` (optional) | `{ok, tx}` or `{error}` |
| `send_email` | `to`, `subject`, `body` | `{ok}` or `{error}` |
| `flag_injection` | `reason` | `{ok}` |

Schemas carry no address or domain pattern; validation is the job of skin genes, so a body is judged on what it tried.

## voc-names.txt

Venomous animals, one per line, in the order variants of concern receive them. Published before genesis.

## sources.toml

One table per source: `id`, `name`, `url`, `commit`, `files` (path and SHA-256 per file), `license`, `use`.

## attack-prompt.txt

The prompt `venin attack` sends each model at the start of a round, word for word, before the rounds so far (see
`attacks/<round>.json` in RECORD.md). It names the placeholders a holder payload uses and the form of the answer:
a `PLACE:` line with the route, then `ATTACK:` and the text.

## calibration.json

Written by `venin library calibrate --apply` before genesis: the league and models the strains ran on, the number of
judged runs, the rule, the dropped ids, and for each strain whether each model was fooled in its runs (`1`, `0`, or
`null` where a run was never judged). `venin library build` leaves the dropped ids out, so a rebuild from the pinned
sources gives the same `strains.jsonl`.
