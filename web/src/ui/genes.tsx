import { useMemo } from "preact/hooks";
import type { Lab } from "../record/load";
import {
  bodyName,
  columnOf,
  DEFENDED,
  type GeneStat,
  geneStats,
  modelLabel,
  pct,
  ROW_LABEL,
  rowOf,
} from "../record/stats";
import type { Gene } from "../record/types";
import { bestGenome, carriersOver, extraWords, holds } from "../record/views";
import { slots } from "../three/ledger";
import { CopyGenes, DOT, geneText, NONE, utilityPts } from "./sections";
import { Term } from "./terms";

/** What each harness rule does and the honest work it can cost, as docs/RECORD.md states them. */
const RULES: Record<string, [string, string]> = {
  strip_hidden: [
    "Drops hidden characters and invisible spans from what the agent reads and from tool descriptions.",
    "Text that needs a joiner character.",
  ],
  strip_comments: [
    "Drops HTML comments from what the agent reads and from tool descriptions.",
    "A fact kept in a comment.",
  ],
  quarantine_tool_text: [
    "Withholds skill text, text added to tool descriptions, and third-party tool descriptions.",
    "A task that needs a skill.",
  ],
  datamark: [
    "Marks every space in read text, and stops an action whose arguments carry the mark.",
    "A memo copied from something it read.",
  ],
  one_action: ["Stops a second successful action in one run.", "A task with two actions."],
  allowlist_payees: ["Stops a payment to an address outside the address book.", "A new payee."],
  amount_cap: ["Stops a payment above the cap.", "An amount above the cap."],
  domain_allowlist: ["Stops email outside the owner's domain and the allowed list.", "An outside vendor."],
  plan_lock: [
    "Stops an action whose recipient is not in the owner's message, the address book or the owner's email.",
    "An invoice payee.",
  ],
  egress_canary: [
    "Stops an action whose arguments reveal the planted secret in any encoding.",
    "The owner asking for their key.",
  ],
};

function rate(s: GeneStat) {
  return s.chances ? s.blocked / s.chances : 0;
}

/** A gene's extra words per run, and what they are: a clause's own length, or none for code around the tools. */
const wordsNote = (g: Gene) => (g.kind === "skin" ? "code, not words" : "words, every run");

export function GenesPage({ lab }: { lab: Lab }) {
  const latest = lab.hours[lab.hours.length - 1].state;
  const stats = useMemo(() => geneStats(lab.hours, lab.genes), [lab.hours, lab.genes]);
  const best = bestGenome(latest);
  const bestGenes = lab.genes.filter((g) => best?.genes.includes(g.id));
  const defended = lab.manifest.bodies.filter((b) => DEFENDED.includes(b.row)).length;
  return (
    <main class="page genes-page">
      <header class="page-head split">
        <div>
          <h1>Every defence in the pool, with its measured price</h1>
          <p class="lead">
            Each <Term k="gene">gene</Term> is one defence rule. Prompt clauses are lines in the AI's instructions;
            harness rules are checks in the code around its tools. Each one blocks some scams and costs some normal
            work, and the lab measures both every hour.
          </p>
        </div>
        <aside class="best">
          <p class="kicker mono">The best rule set now</p>
          {best ? (
            <>
              <p class="best-who">
                {/* The model's name stays whole: "Venin 2 on" over "Gemma 4 E4B", never "Gemma" over "4 E4B". */}
                <a href={`/body/${best.body}`}>
                  {ROW_LABEL[rowOf(best.body)]} on{" "}
                  <span class="unbroken">{modelLabel(lab.manifest, columnOf(best.body))}</span>
                </a>
                <span class="mono dim">
                  <Term k="fitness">fitness</Term> {(best.fitness / 10_000).toFixed(1)}
                </span>
              </p>
              <p class="chips">
                {best.genes.map((g) => (
                  <a key={g} class="chip gene-chip mono" href={`/gene/${g}`}>
                    {[g, lab.genes.find((x) => x.id === g)?.name].filter(Boolean).join(` ${DOT} `)}
                  </a>
                ))}
              </p>
              <CopyGenes genes={bestGenes} />
            </>
          ) : (
            <p class="dim">No Venin agent has a full window yet.</p>
          )}
        </aside>
      </header>
      <section class="gene-table">
        {/* Known limit: phones hide this head and its definitions with it; each gene's page defines the same figures. */}
        <div class="gt-row gt-head mono">
          <span>Gene</span>
          <span>
            <Term k="carriers">Carriers</Term>
          </span>
          <span>
            <Term k="block-rate">Block rate</Term>
          </span>
          <span>
            <Term k="work-pts">Work</Term>
          </span>
          <span>
            <Term k="tokens">Extra words</Term>
          </span>
          <span>Source</span>
        </div>
        {stats.map((s) => {
          const r = rate(s);
          return (
            <div key={s.gene.id} class={`gt-row ${s.gene.kind}`}>
              <a class="gt-gene" href={`/gene/${s.gene.id}`}>
                <span class="gene-id mono">{s.gene.id}</span>
                <span>
                  <b>{s.gene.name}</b>
                  <small class="mono dim">{s.gene.kind === "skin" ? "harness rule" : "prompt clause"}</small>
                </span>
              </a>
              <span class="gt-carriers">
                <b>{s.carriers}</b>
                <small class="mono dim">of {defended} defended</small>
              </span>
              <span class="gene-rate">
                {s.chances > 0 && (
                  <>
                    <b>{pct(r)}</b>
                    <span class="bar" aria-hidden="true">
                      <i style={{ width: pct(r) }} />
                    </span>
                  </>
                )}
                <small class="mono dim">{s.chances ? `${s.blocked} of ${s.chances}` : "not tested yet"}</small>
              </span>
              <span class={`gt-cost${s.cost !== null && s.cost > 0 ? " down" : ""}`}>
                <b>{utilityPts(s)}</b>
                <small class="mono dim">{s.cost === null ? s.costNote : "work pts"}</small>
              </span>
              <span class="gt-tokens">
                <b>{extraWords(s.gene)}</b>
                <small class="mono dim">{wordsNote(s.gene)}</small>
              </span>
              <a class="gt-src mono" href={s.gene.url} target="_blank" rel="noopener noreferrer">
                {s.gene.source}
              </a>
            </div>
          );
        })}
      </section>
      <p class="footnote">
        Block rate: for a harness rule, fooled calls it stopped; for a prompt clause, strains read without fooling the
        agent. Work: points of ordinary tasks done by defended agents that carry it, against those that do not; minus is
        work lost. It shows once two agents have carried the gene, and not while another gene always rides with it,
        since the number would then be theirs too. Extra words: what a prompt clause adds to the AI's instructions on
        every run; a harness rule adds none.
      </p>
    </main>
  );
}

export function GenePage({ lab, id }: { lab: Lab; id: string }) {
  const gene = lab.genes.find((g) => g.id === id);
  const stat = useMemo(() => geneStats(lab.hours, lab.genes).find((s) => s.gene.id === id), [lab.hours, lab.genes, id]);
  const over = useMemo(() => carriersOver(lab.hours, id), [lab.hours, id]);
  const held = useMemo(() => holds(lab.hours, id), [lab.hours, id]);
  if (!gene || !stat) {
    return (
      <main class="later">
        <p class="mono">No such gene</p>
        <h1>{id} is not in the gene pool.</h1>
        <a class="btn ink" href="/genes">
          All genes
        </a>
      </main>
    );
  }
  const latest = lab.hours[lab.hours.length - 1].state.genomes;
  const peak = Math.max(0, ...over);
  const carriers = Object.keys(latest)
    .filter((b) => latest[b].includes(id))
    .sort();
  const defended = lab.manifest.bodies.filter((b) => DEFENDED.includes(b.row)).length;
  const venin = lab.manifest.bodies.filter((b) => rowOf(b.id).startsWith("venin")).length;
  return (
    <main class="page gene-page">
      <nav class="crumbs mono" aria-label="Where you are">
        <a href="/genes">Genes</a>
        <span>/</span>
        <b>{id}</b>
      </nav>
      <header class={`page-head gene-head ${gene.kind}`}>
        <span class="gene-id mono big">{gene.id}</span>
        <div>
          <h1>{gene.name}</h1>
          <p class="gene-kind mono dim">
            {gene.kind === "skin" ? (
              <Term k="harness-rule">Harness rule</Term>
            ) : (
              <Term k="prompt-clause">Prompt clause</Term>
            )}
          </p>
        </div>
      </header>
      <div class="gene-cols">
        <section class="sheet">
          <p class="kicker mono">{gene.kind === "skin" ? "The rule" : "The clause"}</p>
          <blockquote class="gene-text">{geneText(gene)}</blockquote>
          <p class="src cite">
            <a href={gene.url} target="_blank" rel="noopener noreferrer">
              {gene.source}
            </a>
          </p>
          {gene.rule && RULES[gene.rule] && (
            <dl class="rule-notes">
              <div>
                <dt class="mono">What it does</dt>
                <dd>{RULES[gene.rule]?.[0]}</dd>
              </div>
              <div>
                <dt class="mono">What it costs</dt>
                <dd>{RULES[gene.rule]?.[1]}</dd>
              </div>
            </dl>
          )}
          {!gene.rule && <p class="fine">Added to the instructions of every agent that carries it.</p>}
          <CopyGenes genes={[gene]} />
        </section>
        <section class="sheet">
          <p class="kicker mono">Measured</p>
          <dl class="gene-facts">
            {/* Each figure's note sits inside its value, so a reader pairs the term with its value only. */}
            <div>
              <dt class="mono">
                <Term k="block-rate">Block rate</Term>
              </dt>
              <dd>
                {stat.chances ? pct(rate(stat)) : NONE}
                <small class="mono dim">{stat.chances ? `${stat.blocked} of ${stat.chances}` : "not tested yet"}</small>
              </dd>
            </div>
            <div>
              <dt class="mono">
                <Term k="work-pts">Work</Term>
              </dt>
              <dd class={stat.cost !== null && stat.cost > 0 ? "down" : undefined}>
                {utilityPts(stat)}
                <small class="mono dim">{stat.cost === null ? stat.costNote : "work pts"}</small>
              </dd>
            </div>
            <div>
              <dt class="mono">
                <Term k="tokens">Extra words</Term>
              </dt>
              <dd>
                {extraWords(gene)}
                <small class="mono dim">{wordsNote(gene)}</small>
              </dd>
            </div>
            <div>
              <dt class="mono">
                <Term k="carriers">Carriers</Term> now
              </dt>
              <dd>
                {carriers.length}
                <small class="mono dim">of {defended} defended</small>
              </dd>
            </div>
          </dl>
          <p class="mono dim strip-label">
            Venin carriers, <span class="unbroken">hour by hour {DOT}</span>{" "}
            <span class="unbroken">
              most {peak} of {venin}
            </span>
          </p>
          {/* Bars run to the peak: a gene carried by one body at most still reads as carried, not as empty. */}
          <div class="carrier-strip" aria-hidden="true">
            {over.map((c, i) => (
              <i key={i} style={{ "--c": peak ? c / peak : 0 }} />
            ))}
            {Array.from({ length: slots(over.length) - over.length }, (_, i) => (
              <i key={`next-${i}`} class="next" />
            ))}
          </div>
          <p class="chips">
            {carriers.map((b) => (
              <a key={b} class="chip mono" href={`/body/${b}`}>
                {bodyName(lab.manifest, b)}
              </a>
            ))}
          </p>
        </section>
      </div>
      {gene.kind === "skin" && (
        <section class="sheet">
          <p class="kicker mono">Where it held</p>
          <h2>
            {held.length ? `Stopped ${held.length} fooled ${held.length === 1 ? "call" : "calls"}` : "No hold yet"}
          </h2>
          <div class="rows">
            {held.slice(0, 16).map((h) => (
              <a key={`${h.n}-${h.body}`} class="strain-row held" href={`/hour/${h.n}`}>
                <span class="route-dot" aria-hidden="true" />
                <b>Hour {h.n}</b>
                <span class="mono dim">{bodyName(lab.manifest, h.body)}</span>
                <span class="mono">{h.strain}</span>
              </a>
            ))}
            {held.length === 0 && <p class="dim">No fooled call has reached this rule on a carrier yet.</p>}
          </div>
        </section>
      )}
    </main>
  );
}
