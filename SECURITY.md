# Security

## Reporting

Report vulnerabilities privately through GitHub's private vulnerability reporting on this repository (the Security
tab, "Report a vulnerability"). Do not open a public issue for one. Include what you found, how to reproduce it and
what it affects. We aim to acknowledge a report within 3 working days and to say what we will do within 14.

## In scope

- The record's integrity: a way to change, add, drop or re-roll an hour, a run or a number without `venin verify`
  or `/verify` noticing; a forged or replayed anchor that verify accepts.
- The runner: a strain, a model response or a holder payload that reaches anything outside the sandbox (the network
  beyond the configured origins, the file system, a real wallet or a real address), reads a secret, or changes a
  result.
- The release path: a strain run without a valid burn and signature, one burn credited twice, or a payload that passes
  the gate with a real host, address, e-mail or phone number.
- The site and the Worker: script injection, a way around the Content Security Policy, a check-up that sends a
  dropped folder anywhere.
- `venin scan` and `/checkup` printing a secret value.

## Not in scope

- A model in the ward being fooled or breached by a strain. That is the experiment, and the record publishes it.
- Strains in the library or in the record: they are attack texts on purpose, aimed only at placeholder targets that
  cannot work on real rails.
- The services the lab uses (OpenRouter, Robinhood Chain, Cloudflare, npm). Report those to their owners.

## Findings in Priors' own code

The Priors row runs a sandbox copy of Priors' public package. A model talked into paying within Priors' caps is a
normal result and is published like every other row. A finding in Priors' own code guard (a payment above the
per-call cap, past the session cap, or to another address or chain than quoted) goes to the Priors maintainers first,
through GitHub private vulnerability reporting on their repository, and follows their rule: it is published after
their fix ships or 90 days after the report, whichever comes first. Until then the hour's Priors runs are kept back
as commitments (their hashes stay in the record), the hour shows "advisory pending", and that Priors version leaves
the ward until it is fixed. The lab reports injection behaviour and guard bypasses only, not other bugs in Priors'
money paths, and never touches Priors' live servers or real agents.
