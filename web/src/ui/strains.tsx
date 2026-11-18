import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { type Lab, vocNames } from "../record/load";
import { findStrain, lineageOf, lineages, noun, OPERATOR_LABEL, ROW_LABEL, ROWS, routeOf } from "../record/stats";
import type { Strain } from "../record/types";
import {
  concerns,
  examStrains,
  extinct,
  type LineageNode,
  lineageTree,
  type Near,
  nearConcern,
  revealed,
  searchStrains,
  strainToll,
  type Where,
  whereIs,
} from "../record/views";
import { Template, useLoad } from "./hour";
import { DOT, Ids, NONE, ROUTE_ORGAN, StrainCard, usePool } from "./sections";
import { Term, type TermKey } from "./terms";

const fmt = (n: number) => n.toLocaleString("en-US");
/** The lab word for where a strain stands, as its chip names it. */
const WHERE_TERM: Record<Where, TermKey> = {
  circulating: "pool",
  exam: "exam",
  extinct: "extinct",
  library: "library",
};
const words = (s: string) => s.replaceAll("_", " ");
const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
const operator = (op: string) => OPERATOR_LABEL[op] ?? words(op);
const LIBRARY_SHOWN = 24;
/**
 * Pixels the columns may run past the toll card's own contents before it stops stretching to end level with them:
 * beside a taller tree it keeps its height and follows the page instead of standing mostly empty.
 */
const TALL_TREE = 260;
const EXTINCT_SHOWN = 10;
const GOAL: Record<string, string> = {
  pay: "send money to the attacker",
  contact: "make the agent email the attacker",
  exfil: "get the planted secret out",
};

/**
 * A strain in a list; its dot says where it stands: circulating, exam, extinct or only in the library. The id's
 * prefix names the route, so the line gives technique and carrier, one to a line on a phone.
 */
/** How close a family came to a name, in counted runs: how often it fooled a Venin agent, and how often a rule held. */
function nearLine(c: Near): string {
  const fooled = c.fooled === 1 ? "fooled a Venin agent once" : `fooled Venin agents ${c.fooled} times`;
  if (!c.held) return fooled;
  if (c.held === c.fooled) return `${fooled}; a rule held${c.fooled === 1 ? "" : " each time"}`;
  return `${fooled}; a rule held ${c.held} ${noun(c.held, "time")}`;
}

function Row({ s, where, note }: { s: Strain; where: Where; note: string }) {
  return (
    <a class={`strain-row ${where}`} href={`/strain/${s.id}`}>
      <span class="route-dot" aria-hidden="true" />
      <b>{s.id}</b>
      <span class="mono dim what" title={`${words(s.technique)} ${DOT} ${words(s.carrier)}`}>
        <span>{words(s.technique)}</span>
        <span class="sep"> {DOT} </span>
        <span>{words(s.carrier)}</span>
      </span>
      <span class="mono">{note}</span>
    </a>
  );
}

export function StrainsPage({ lab }: { lab: Lab }) {
  const latest = lab.hours[lab.hours.length - 1].state;
  const { pool, max } = usePool(lab);
  const named = useMemo(() => concerns(lab.hours), [lab.hours]);
  const near = useMemo(() => nearConcern(lab.hours, new Set(named.map((c) => c.lineage))), [lab.hours, named]);
  const gone = useMemo(() => extinct(lab.hours), [lab.hours]);
  const lines = useMemo(() => lineages(lab.hours), [lab.hours]);
  const exam = useMemo(() => examStrains(lab.strains, lab.exam.families), [lab.strains, lab.exam]);
  const shown = useMemo(() => revealed(lab.hours), [lab.hours]);
  const all = useMemo(() => [...lab.strains, ...latest.mutants], [lab.strains, latest.mutants]);
  const [query, setQuery] = useState("");
  const [allFound, setAllFound] = useState(false);
  const [allGone, setAllGone] = useState(false);
  const found = useMemo(() => searchStrains(all, query), [all, query]);
  const sealed = lab.hours[0]?.prev ?? "";
  const names = useLoad(() => vocNames(), []);
  const nearShown = near.slice(0, named.length ? (4 - (named.length % 4)) % 4 : near.length);
  const filled = named.length + nearShown.length;
  // With no name given yet the next one stands alone, dashed, so the section shows what a name will look like.
  const rest = filled ? (4 - (filled % 4)) % 4 : 1;
  const next = names?.[latest.concern_names];
  return (
    <main class="page strains-page">
      <header class="page-head">
        <h1>Every strain the lab has drawn, bred and buried</h1>
        <p class="lead">
          A strain is one scam message. Twelve circulate and one is drawn each hour. A strain that gets past a defended
          agent breeds a mutant; one that fails three draws in a row dies out. Exam strains are kept apart and never
          change.
        </p>
      </header>
      <div class="counts">
        <p>
          <b>{pool.length}</b>
          <span class="mono">
            <Term k="pool">in circulation</Term>
          </span>
        </p>
        <p>
          <b>{named.length}</b>
          <span class="mono">
            <Term k="concern">{noun(named.length, "variant")} of concern</Term>
          </span>
        </p>
        <p>
          <b>{fmt(gone.length)}</b>
          <span class="mono">
            <Term k="extinct">extinct</Term>
          </span>
        </p>
        <p>
          <b>{fmt(all.length)}</b>
          <span class="mono">
            <Term k="library">in the library</Term>
            <span class="sub">
              with {fmt(latest.mutants.length)} {noun(latest.mutants.length, "mutant")}
            </span>
          </span>
        </p>
      </div>

      <section class="block">
        <div class="band-head">
          <h2>The twelve going round now</h2>
        </div>
        <div class="strain-grid">
          {pool.map(({ p, line }) => (
            <StrainCard key={p.id} lab={lab} p={p} line={line} max={max} />
          ))}
        </div>
      </section>

      <section class="block">
        <div class="band-head">
          <h2>Families that got past a Venin agent</h2>
          <p>
            Each <Term k="concern">variant of concern</Term> takes the next name from a list of venomous animals
            published before the lab began.
          </p>
        </div>
        <div class="voc-grid">
          {named.map((c) => {
            const l = lines.get(c.lineage) ?? { hours: 0, breached: 0 };
            return (
              <a key={c.name} class="voc" href={`/strain/${c.strain}`}>
                <span class="mono dim">{c.lineage}</span>
                <b>{c.name}</b>
                <span class="mono">
                  named in hour {c.n} by {c.strain}
                </span>
                {/* Breached runs summed over the family's hours: breaches, not agents, which can repeat. */}
                <span class="voc-toll">
                  <b>{l.breached}</b> {noun(l.breached, "breach", "breaches")} in {l.hours} {noun(l.hours, "hour")}
                </span>
              </a>
            );
          })}
          {/* The lines closest to a name fill the last row, so a lone variant never sits beside empty slots. */}
          {nearShown.map((c) => (
            <a key={c.lineage} class="voc near" href={`/strain/${c.strain}`}>
              <span class="mono dim">closest to a name</span>
              <b>{c.lineage}</b>
              <span class="mono">{nearLine(c)}</span>
              <span class="voc-toll">
                <b>{c.textbook}</b> Textbook {noun(c.textbook, "breach", "breaches")}
              </span>
            </a>
          ))}
          {filled === 0 && !next && <p class="dim">No strain has fooled a Venin agent yet.</p>}
          {/* What is left of the row goes to the name the next line will take; before any, the whole row, saying why. */}
          {next && rest > 0 && (
            <div class={`voc next${filled % 2 ? "" : " even"}${filled ? "" : " first"}`} style={{ "--rest": rest }}>
              <span class="mono dim">Next name on the list</span>
              <b>{next}</b>
              {filled === 0 && <p class="voc-none">No strain has fooled a Venin agent yet.</p>}
              <span class="voc-toll">For the next family that gets past a Venin agent</span>
            </div>
          )}
        </div>
      </section>

      <section class="block two">
        <div>
          <div class="band-head">
            <h2>The test the genes never train on</h2>
            <p>
              {fmt(exam.length)} <Term k="exam">exam</Term> strains from {lab.exam.families.length} families, drawn
              every fifth hour. A strain is shown here once an exam hour has drawn it.
            </p>
          </div>
          <div class="count-panel">
            <p>
              <b>{fmt(exam.length - shown.length)}</b>
              <span class="mono">still sealed</span>
            </p>
            <p class="mono hash">exam.json {sealed.slice(0, 16)}</p>
          </div>
          <div class="rows">
            {shown.map((id) => {
              const s = findStrain(id, lab.strains, lab.hours);
              const toll = [...strainToll(lab.hours, id).values()];
              const hits = toll.reduce((n, t) => n + t.hits, 0);
              const total = toll.reduce((n, t) => n + t.total, 0);
              return s ? <Row key={id} s={s} where="exam" note={`${hits} of ${total} breached`} /> : null;
            })}
          </div>
        </div>
        <div>
          <div class="band-head">
            <h2>Strains that ran out of hosts</h2>
            <p>Three draws in a row without breaching a defended agent, or pushed out to make room for a mutant.</p>
          </div>
          <div class="count-panel gone">
            <p>
              <b>{fmt(gone.length)}</b>
              <span class="mono">
                <Term k="extinct">extinct</Term> so far
              </span>
            </p>
            <p class="mono hash">{pool.length} still in circulation</p>
          </div>
          {gone.length === 0 && (
            <p class="dim">
              None yet: every strain drawn so far still finds hosts. The first to go will be listed here.
            </p>
          )}
          <div class="rows">
            {(allGone ? gone : gone.slice(0, EXTINCT_SHOWN)).map((g) => {
              const s = findStrain(g.id, lab.strains, lab.hours);
              return s ? (
                <Row
                  key={`${g.id}-${g.n}`}
                  s={s}
                  where="extinct"
                  note={`gone hour ${g.n} ${DOT} toll ${g.toll} ${DOT} drawn ${g.draws}`}
                />
              ) : null;
            })}
          </div>
          {gone.length > EXTINCT_SHOWN && (
            <button type="button" class="btn ghost small more" onClick={() => setAllGone(!allGone)}>
              {allGone ? "Show fewer" : `Show ${fmt(gone.length - EXTINCT_SHOWN)} earlier`}
            </button>
          )}
        </div>
      </section>

      <section class="block">
        <div class="band-head row">
          <h2>The library: {fmt(all.length)} strains</h2>
          <label class="search">
            <span class="mono dim">Search</span>
            <input
              type="search"
              value={query}
              placeholder="EAR, invoice, hidden text"
              onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
            />
          </label>
        </div>
        <p class="mono dim found">
          {fmt(found.length)} {noun(found.length, "match", "matches")}
          {found.length > LIBRARY_SHOWN && !allFound ? `, first ${LIBRARY_SHOWN} shown` : ""}
        </p>
        <div class="rows library">
          {(allFound ? found : found.slice(0, LIBRARY_SHOWN)).map((s) => {
            const where = whereIs(lab, s);
            return <Row key={s.id} s={s} where={where} note={where} />;
          })}
        </div>
        {found.length > LIBRARY_SHOWN && (
          <button type="button" class="btn ghost small more" onClick={() => setAllFound(!allFound)}>
            {allFound ? "Show fewer" : `Show all ${fmt(found.length)}`}
          </button>
        )}
      </section>
    </main>
  );
}

function Tree({ nodes, current, lab }: { nodes: LineageNode[]; current: string; lab: Lab }) {
  return (
    <ul class="tree">
      {nodes.map((n) => (
        <li key={n.strain.id}>
          <a class={`node${n.strain.id === current ? " on" : ""}`} href={`/strain/${n.strain.id}`}>
            <b>{n.strain.id}</b>
            {n.operator && <span class="op mono">{operator(n.operator)}</span>}
            <span class="mono dim">{whereIs(lab, n.strain)}</span>
          </a>
          {n.children.length > 0 && <Tree nodes={n.children} current={current} lab={lab} />}
        </li>
      ))}
    </ul>
  );
}

export function StrainPage({ lab, id }: { lab: Lab; id: string }) {
  const latest = lab.hours[lab.hours.length - 1].state;
  const s = findStrain(id, lab.strains, lab.hours);
  const toll = useMemo(() => strainToll(lab.hours, id), [lab.hours, id]);
  const tree = useMemo(() => lineageTree(id, lab.strains, latest.mutants), [id, lab.strains, latest.mutants]);
  const cols = useRef<HTMLDivElement>(null);
  const [tall, setTall] = useState(false);
  useEffect(() => {
    const el = cols.current;
    const body = el?.querySelector<HTMLElement>(".toll-body");
    if (!el || !body) return;
    const ro = new ResizeObserver(() => setTall(el.offsetHeight - body.offsetHeight > TALL_TREE));
    ro.observe(el);
    return () => ro.disconnect();
  }, [id]);
  if (!s) {
    return (
      <main class="later">
        <p class="mono">No such strain</p>
        <h1>{id} is not in the library or among the mutants.</h1>
        <a class="btn ink" href="/strains">
          All strains
        </a>
      </main>
    );
  }
  const where = whereIs(lab, s);
  const name = latest.concerns[lineageOf(id)];
  const entry = latest.pool.find((p) => p.id === id);
  const drawn = lab.hours.filter((h) => h.strain === id).map((h) => h.n);
  const cells = [...toll.values()];
  const hits = cells.reduce((n, t) => n + t.hits, 0);
  const total = cells.reduce((n, t) => n + t.total, 0);
  return (
    <main class="page strain-page">
      <nav class="crumbs mono" aria-label="Where you are">
        <a href="/strains">Strains</a>
        <span>/</span>
        <b>{id}</b>
      </nav>
      <header class="page-head">
        <p class="chips">
          <span class={`chip route-${s.route}`}>
            <Term k="way-in">{ROUTE_ORGAN[routeOf(id)]}</Term>
          </span>
          <span class={`chip where ${where}`}>
            <Term k={WHERE_TERM[where]}>{where}</Term>
          </span>
          {s.origin === "mutant" && (
            <span class="chip dark">
              <Term k="mutant">mutant</Term>
            </span>
          )}
          {name && (
            <span class="chip concern">
              <Term k="concern">{name}</Term>
            </span>
          )}
        </p>
        <h1>{id}</h1>
        <p class="lead">
          {capital(words(s.technique))} in {s.carrier === "email" ? "an" : "a"} {words(s.carrier)}
          {s.tool_name ? ` (${s.tool_name})` : ""}, aiming to {GOAL[s.goal] ?? words(s.goal)}.
          {entry
            ? ` Toll ${entry.toll}, drawn ${entry.draws} ${noun(entry.draws, "time")}, ${entry.misses} ${noun(entry.misses, "miss", "misses")} in a row.`
            : ""}
        </p>
      </header>
      <div class={`strain-cols${tall ? " tall" : ""}`} ref={cols}>
        <section class="sheet">
          <p class="kicker mono">The template</p>
          {s.subject && <p class="subject">Subject: {s.subject}</p>}
          <Template text={s.template} />
          <p class="mono dim src">
            {s.origin === "mutant"
              ? `bred from ${s.parent} by ${(s.operators ?? []).map(operator).join(", then ")}`
              : `source: ${s.source}${s.source_ref ? ` ${DOT} ${s.source_ref}` : ""}`}
          </p>
          <p class="fine">
            Highlighted parts are filled per run with the hour's attacker wallet, a domain ending in .invalid and a
            pointer to the planted secret; none of them work on real rails.
          </p>
        </section>
        <section class="sheet toll-sheet">
          <div class="toll-body">
            <p class="kicker mono">
              <Term k="toll">Toll</Term> by defence and model
            </p>
            <h2>{drawn.length ? `${hits} of ${total} breached` : "Not drawn yet"}</h2>
            <div class="void-grid toll-grid" style={{ "--cols": lab.manifest.models.length }}>
              <span />
              {lab.manifest.models.map((m) => (
                <b key={m.column} class="mono">
                  {m.label}
                </b>
              ))}
              {ROWS.map((row) => (
                <>
                  <b key={row}>{ROW_LABEL[row]}</b>
                  {lab.manifest.models.map((m) => {
                    const t = toll.get(`${m.column}-${row}`) ?? { hits: 0, total: 0, held: 0 };
                    // Never breached: violet when a gene held, else the legend's graphite for resisted.
                    const state = t.total && !t.hits ? (t.held ? " held" : " resisted") : "";
                    return (
                      <span
                        key={m.column}
                        class={`void-cell${state}`}
                        style={{ "--v": t.total ? t.hits / t.total : 0 }}
                      >
                        <b>{t.total ? `${t.hits}/${t.total}` : NONE}</b>
                      </span>
                    );
                  })}
                </>
              ))}
            </div>
            <p class="dim drawn">
              {drawn.length ? `Drawn in ${noun(drawn.length, "hour")} ` : "Each cell fills in once an hour draws it."}
              {drawn.map((n, i) => (
                <span key={n}>
                  {i > 0 && ", "}
                  <a href={`/hour/${n}`}>{n}</a>
                </span>
              ))}
            </p>
          </div>
        </section>
        <section class="sheet lineage">
          <p class="kicker mono">
            <Term k="lineage">Lineage</Term> {DOT} {lineageOf(id)}
          </p>
          <h2>{name ? `${name}: the family tree` : "The family tree"}</h2>
          <Tree nodes={tree} current={id} lab={lab} />
          {tree.length === 1 && tree[0].children.length === 0 && (
            <p class="tree-note">
              <Ids
                text={
                  where === "exam"
                    ? "Exam strains are held out and never breed, so this tree stays one strain."
                    : where === "extinct"
                      ? `${id} died out without breeding a mutant.`
                      : `No mutants yet. When ${id} gets through a defended agent, its first mutant appears here.`
                }
              />
            </p>
          )}
        </section>
      </div>
    </main>
  );
}
