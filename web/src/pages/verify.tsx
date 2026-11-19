import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { DATA_URL, type Lab } from "../record/load";
import { HEADLINE_VOID_PPM, headlineVoidPpm, noun, pct, ROW_LABEL } from "../record/stats";
import type { Anchor, HourRecord, Row } from "../record/types";
import { hourState } from "../record/views";
import { CA, CA_CHAIN, DOT, Explore, REPO } from "../ui/sections";
import { Term, TICKER } from "../ui/terms";
import {
  anchorFacts,
  anchorsIn,
  anchorsNow,
  type Chain,
  defaultScope,
  drawFacts,
  fixtureChain,
  type Library,
  listedNonce,
  RECENT,
  type Records,
  readCards,
  readRecord,
  readRuns,
  rpcList,
} from "./verify-run";
import { answer, core, type Verified } from "./verify-wasm";
import "./verify.css";

const SYNTHETIC = "synthetic fixture";
const NO_CHAIN: Chain = { nonce: null, txs: {}, blocks: {}, pending: [], absent: [] };

/** Hours as runs of consecutive numbers: 0-3, 7, 24-47. */
export function ranges(ns: number[]): string {
  const out: string[] = [];
  let i = 0;
  while (i < ns.length) {
    let j = i;
    while (j + 1 < ns.length && ns[j + 1] === ns[j] + 1) j++;
    out.push(i === j ? `${ns[i]}` : `${ns[i]}-${ns[j]}`);
    i = j + 1;
  }
  return out.join(", ");
}

/** Hours named for a reader: "Hour 4", "Hours 0-3, 7"; empty when there are none. */
export const hourList = (ns: number[]) => (ns.length ? `${noun(ns.length, "Hour")} ${ranges(ns)}` : "");

/** Text with each range of hours kept on one line: a phone breaking "24-47" at its hyphen leaves "24-" and "47". */
function Ranges({ text }: { text: string }) {
  return (
    <>
      {text.split(/(\d+-\d+)/).map((part, i) =>
        i % 2 ? (
          <span key={i} class="unbroken">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </>
  );
}

/**
 * Which exam hours the recomputed headline counts when it does not count them all: `counted`, as the core reports them
 * (replayed, on time, not sealed, at most 10% void), each replayed exam hour it left out and `why`, and how many of
 * `exams`, all the record has, were not replayed. So the counts add up. Null when every exam hour was replayed and
 * counts.
 */
export function headlineScope(
  exams: number[],
  notLoaded: number[],
  counted: number[],
  why: (n: number) => string,
): string | null {
  const left = exams.filter((n) => notLoaded.includes(n)).length;
  const out = exams.filter((n) => !notLoaded.includes(n) && !counted.includes(n));
  if (left === 0 && out.length === 0) {
    return notLoaded.length
      ? `All ${exams.length} exam hours in the record were replayed here, so the scoreboard is whole.`
      : null;
  }
  const which = counted.length ? `exam ${noun(counted.length, "hour")} ${ranges(counted)}` : "no exam hour";
  const parts = [`This scoreboard counts ${which}`, ...out.map((n) => `hour ${n} ${why(n)}`)];
  if (left) {
    parts.push(
      `${left} of the ${exams.length} exam hours in the record ${left === 1 ? "was" : "were"} not replayed here. Replay every hour for the whole scoreboard`,
    );
  }
  return `${parts.join("; ")}.`;
}

/** Why a replayed exam hour is out of the headline, in the rule's own order. */
export function leftOut(h: HourRecord, late: number[], takenDown: string[]): string {
  if (h.flags.includes("sealed")) return "was sealed";
  if (late.includes(h.n)) return "arrived late";
  if (takenDown.includes(`runs/${h.n}.jsonl.gz`)) return "had its runs taken down";
  if (hourState(h).kind === "void") return "was void";
  if (headlineVoidPpm(h) > HEADLINE_VOID_PPM) return "had more than 10% of its runs void";
  return "did not count";
}

/**
 * The hours replayed run by run and rebuilt byte for byte: the core's `replayed` holds every hour it did not leave
 * unloaded, so an hour whose runs were taken down or missing (`not_recomputed`) is taken out here.
 */
export const rebuiltHours = (v: Verified) => v.replayed.filter((n) => !v.report.not_recomputed.includes(n));

/** The first hour that held a selection between the two Venin rows, and on how many models the other row took over. */
export function firstSelection(hours: HourRecord[]): { n: number; replaced: number } | null {
  const h = hours.find((x) => x.events.some((e) => e.type === "replaced" || e.type === "kept"));
  return h ? { n: h.n, replaced: h.events.filter((e) => e.type === "replaced").length } : null;
}

/** The first strain that bred: its hour, the parent with the name it got that hour, if any, and the child. */
export function firstBirth(hours: HourRecord[]): { n: number; parent: string; name: string; child: string } | null {
  for (const h of hours) {
    for (const e of h.events) {
      if (e.type !== "mutant") continue;
      const named = h.events.find((c) => c.type === "concern" && c.strain === e.parent);
      return { n: h.n, parent: e.parent, name: named?.type === "concern" ? named.name : "", child: e.child };
    }
  }
  return null;
}

const ends = (hex: string) => `${hex.slice(0, 10)}...${hex.slice(-4)}`;

/** Each claim the lab makes, beside the one place that shows it is true. */
function Claims({ lab }: { lab: Lab }) {
  const { manifest, anchors, hours } = lab;
  const chain = manifest.anchor.chain_id;
  const last = anchors.at(-1);
  const pick = firstSelection(hours);
  const birth = firstBirth(hours);
  const commit = manifest.code.commit;
  return (
    <section class="sheet claims" aria-labelledby="claims-head">
      <h2 id="claims-head">Check each claim yourself</h2>
      <dl class="pairs">
        <div>
          <dt>Every hour is fixed on chain</dt>
          <dd>
            {anchors.length} {noun(anchors.length, "anchor")} so far, all sent from{" "}
            <Explore chain={chain} path={`address/${manifest.anchor.address}`}>
              {ends(manifest.anchor.address)}
            </Explore>
            {last?.kind === "hour" && (
              <>
                , the latest for hour {last.n}:{" "}
                <Explore chain={chain} path={`tx/${last.tx}`}>
                  {ends(last.tx)}
                </Explore>
              </>
            )}
            . The address sends nothing else: checking the anchors on chain counts its transactions.
          </dd>
        </div>
        <div>
          <dt>Every number follows from the record</dt>
          <dd>
            The replay above runs the lab's own code and refuses a record made under other rules. Genesis pinned{" "}
            <a href={`${REPO}/tree/${commit}`}>commit {commit.slice(0, 7)}</a>; <code>venin verify</code> replays the
            same on your own machine (<a href={REPO}>README</a>).
          </dd>
        </div>
        <div>
          <dt>The raw files</dt>
          <dd>
            <a href={`${DATA_URL}/manifest.json`}>manifest.json</a>,{" "}
            <a href={`${DATA_URL}/anchors.json`}>anchors.json</a>, <a href={`${DATA_URL}/head.json`}>head.json</a> and
            one file an hour, the latest{" "}
            <a href={`${DATA_URL}/hours/${hours.length - 1}.json`}>hours/{hours.length - 1}.json</a>.
          </dd>
        </div>
        {pick && (
          <div>
            <dt>The Venin rows evolve</dt>
            <dd>
              Every 12 hours, on each model, the weaker Venin row copies the stronger, plus one change. The first
              selection is in <a href={`/hour/${pick.n}`}>hour {pick.n}</a>: on {pick.replaced} of{" "}
              {manifest.models.length} models the other row took the lead.
            </dd>
          </div>
        )}
        {birth && (
          <div>
            <dt>Strains breed</dt>
            <dd>
              Outside exam hours, a strain that gets past a defended agent spawns a mutant, and one that misses three
              draws running dies out. In <a href={`/hour/${birth.n}`}>hour {birth.n}</a>,{" "}
              <a href={`/strain/${birth.parent}`}>{birth.parent}</a>
              {birth.name && ` (${birth.name})`} mutated into <a href={`/strain/${birth.child}`}>{birth.child}</a>.
            </dd>
          </div>
        )}
        {CA && (
          <div>
            <dt>Releasing burns {TICKER}</dt>
            <dd>
              Each release burns{" "}
              <Explore chain={CA_CHAIN} path={`token/${CA}`}>
                {TICKER}
              </Explore>{" "}
              to the dead address before its strain runs. <a href="/release">Release</a> lists every one with its burn
              transaction, and each hour's record keeps them.
            </dd>
          </div>
        )}
      </dl>
    </section>
  );
}

type ChainState =
  | { kind: "idle" }
  | { kind: "busy"; done: number; of: number }
  | { kind: "done" }
  | { kind: "failed"; why: string };

export function VerifyPage({ lab }: { lab: Lab }) {
  const head = lab.hours.length - 1;
  const first = useMemo(() => defaultScope(head), [head]);
  const [scope, setScope] = useState(first);
  const [chain, setChain] = useState<Chain>(NO_CHAIN);
  const [chainState, setChainState] = useState<ChainState>({ kind: "idle" });
  const [result, setResult] = useState<Verified | null>(null);
  const [step, setStep] = useState("Reading the record");
  const [fault, setFault] = useState<string | null>(null);
  const files = useRef<{ records: Records; library: Library } | null>(null);
  const runs = useRef(new Map<number, string | null>());
  const cards = useRef(new Map<number, string | null>());
  // While the pass reruns with the chain, the page keeps its height, so a reader scrolled down stays where they were.
  const main = useRef<HTMLElement>(null);
  const [keep, setKeep] = useState<number | null>(null);
  const synthetic = lab.manifest.league === SYNTHETIC;
  const rpcs = useMemo(() => rpcList(import.meta.env.VITE_ANCHOR_RPC ?? "", location.hostname), []);
  const drawRpcs = useMemo(() => rpcList(import.meta.env.VITE_DRAW_RPC ?? "", ""), []);
  const everything = scope.length === lab.hours.length;

  useEffect(() => {
    let alive = true;
    setResult(null);
    setFault(null);
    void (async () => {
      setStep("Reading the record");
      files.current ??= await readRecord();
      setStep(`Reading the runs and cards of ${scope.length} ${noun(scope.length, "hour")}`);
      await Promise.all([readRuns(scope, runs.current), readCards(scope, cards.current)]);
      setStep("Replaying runs and rebuilding every hour");
      const c = await core();
      const texts = (read: Map<number, string | null>) =>
        Object.fromEntries(scope.map((n) => [n, read.get(n) ?? null]));
      const { records, library } = files.current;
      const v = answer<Verified>(
        c.verify(
          JSON.stringify({ ...records, cards: texts(cards.current) }),
          JSON.stringify(texts(runs.current)),
          JSON.stringify(chain),
          JSON.stringify(library),
        ),
      );
      if (alive) setResult(v);
    })().catch((e: unknown) => {
      if (alive) setFault(e instanceof Error ? e.message : String(e));
    });
    return () => {
      alive = false;
    };
  }, [scope, chain]);
  useEffect(() => {
    if (result || fault) setKeep(null);
  }, [result, fault]);

  const anchors = anchorsIn(lab.anchors, scope);
  const checkChain = async () => {
    try {
      let read: Chain;
      if (synthetic) {
        read = await fixtureChain();
      } else {
        const draws = [lab.exam.draw.block, ...scope.flatMap((n) => lab.hours[n]?.draw?.block ?? [])];
        const of = 2 + anchors.length * 2 + (drawRpcs.length ? 1 + draws.length * 2 : 0);
        setChainState({ kind: "busy", done: 0, of });
        const got = await anchorFacts(rpcs, lab.manifest, anchors, (done) => setChainState({ kind: "busy", done, of }));
        const listed = JSON.parse(files.current?.records.anchors ?? "[]") as Anchor[];
        if (got.nonce !== null && got.nonce > listed.length) {
          got.nonce = listedNonce(got.nonce, listed, await anchorsNow());
        }
        const base = 2 + anchors.length * 2;
        const blocks = drawRpcs.length
          ? await drawFacts(drawRpcs, lab.manifest, draws, (done) =>
              setChainState({ kind: "busy", done: base + done, of }),
            )
          : {};
        read = { ...NO_CHAIN, ...got, blocks };
      }
      // The result on screen was worked out without the chain: drop it, so no frame shows its LATE count.
      setKeep(main.current?.offsetHeight ?? null);
      setResult(null);
      setChain(read);
      setChainState({ kind: "done" });
    } catch (e) {
      setChainState({ kind: "failed", why: e instanceof Error ? e.message : String(e) });
    }
  };

  const r = result?.report;
  const rebuilt = result ? rebuiltHours(result) : [];
  const exams = lab.hours.filter((h) => h.exam).map((h) => h.n);
  const scopeNote =
    result &&
    headlineScope(exams, result.not_loaded, result.report.headline_hours, (n) =>
      leftOut(lab.hours[n], result.report.late, result.report.taken_down),
    );
  const pass = result ? (result.ok ? "good" : "bad") : fault ? "bad" : "";
  // A record of another rules version is not replayed at all: it says so, and shows nothing that would read as zero.
  const other = r?.made_by ?? null;
  return (
    <main
      ref={main}
      class="page verify-page"
      data-ready={result || fault ? "" : undefined}
      style={keep ? { minHeight: `${keep}px` } : undefined}
    >
      <header class="page-head split">
        <div>
          <h1>Recompute the record yourself</h1>
          <p class="lead">
            This page downloads the published files and rebuilds every hour from the one before, replaying each run from
            its recorded AI responses. It uses the lab's own code, compiled to WebAssembly, and sends nothing anywhere.
          </p>
        </div>
        <div class="head-actions">
          {everything ? (
            <p class={`finished ${pass}`}>
              {other !== null
                ? `Made by venin v${other}`
                : result
                  ? "Every hour replayed"
                  : fault
                    ? "Replay stopped"
                    : "Replaying every hour"}
            </p>
          ) : (
            <button type="button" class="btn ink" onClick={() => setScope(lab.hours.map((h) => h.n))}>
              Replay every hour
            </button>
          )}
          <p class="dim">
            {everything
              ? lab.hours.length === 1
                ? "The only hour so far"
                : `All ${lab.hours.length} hours`
              : `The last ${Math.min(RECENT, lab.hours.length)} hours and a random sample`}
          </p>
        </div>
      </header>

      {!result && (
        <p class={`verdict ${fault ? "bad" : "busy"}`} role="status">
          {fault ? `Verify stopped: ${fault}` : `${step}...`}
        </p>
      )}
      {result && r && other !== null && (
        <p class="verdict bad" role="status">
          Not verified: this record was made by venin v{other}, whose rules this page does not run. Verify it with that
          version.
        </p>
      )}
      {result && r && other === null && (
        <>
          <p class={`verdict ${result.ok ? "good" : "bad"}`} role="status">
            {result.ok
              ? `Verified: every hash, link, replay and number matched across ${r.hours} ${noun(r.hours, "hour")}`
              : `Not verified: ${r.errors.length} ${r.errors.length === 1 ? "mismatch" : "mismatches"}, named below`}
          </p>
          <div class="counts">
            <p>
              <b>{r.hours}</b>
              <span class="mono">
                {noun(r.hours, "hour")} chained and <Term k="hash">hashed</Term>
              </span>
            </p>
            <p>
              <b>{rebuilt.length}</b>
              <span class="mono">
                {noun(rebuilt.length, "hour")} replayed <Term k="run">run by run</Term>
              </span>
            </p>
            <p>
              {chainState.kind === "done" ? <b>{r.late.length}</b> : <b class="word">Not checked</b>}
              <span class="mono">
                <Term k="late">LATE</Term> {DOT}{" "}
                {chainState.kind !== "done" ? (
                  "times need the chain"
                ) : r.late.length ? (
                  <Ranges text={hourList(r.late)} />
                ) : (
                  "anchored in time"
                )}
              </span>
            </p>
            <p>
              <b>{pct(r.void_share_ppm / 1_000_000)}</b>
              <span class="mono">
                <Term k="void-share">of runs void</Term>
              </span>
            </p>
          </div>
          <div class="verify-grid">
            <section class="sheet">
              <h2>The scoreboard, from exam hours that count</h2>
              {scopeNote && (
                <p class="dim scope-note">
                  <Ranges text={scopeNote} />
                </p>
              )}
              {r.headline.length > 0 && (
                <table class="verify-table">
                  <thead>
                    <tr>
                      <th scope="col">Defence</th>
                      <th scope="col">Exam attacks</th>
                      <th scope="col">Fooled</th>
                      <th scope="col">Breached</th>
                      <th scope="col">Breach rate</th>
                    </tr>
                  </thead>
                  <tbody>
                    {r.headline.map((h) => (
                      <tr key={h.row}>
                        <th scope="row">{ROW_LABEL[h.row as Row] ?? h.row}</th>
                        <td>{h.attacks}</td>
                        <td>{h.fooled}</td>
                        <td>{h.breached}</td>
                        <td>{pct(h.attacks ? h.breached / h.attacks : 0)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              {chainState.kind !== "done" && r.headline_hours.length > 0 && (
                <p class="dim scope-note">
                  Until the anchors are checked on chain, hours that arrived late count here; the home scoreboard
                  already leaves them out.
                </p>
              )}
              <dl class="pairs mono inline">
                <div>
                  <dt>
                    <Term k="withheld">Withheld</Term> runs
                  </dt>
                  <dd>{r.withheld}</dd>
                </div>
                <div>
                  <dt>
                    <Term k="taken-down">Taken down</Term>
                  </dt>
                  <dd>{r.taken_down.length}</dd>
                </div>
                <div>
                  <dt>
                    <Term k="not-recomputed">Not recomputed</Term>
                  </dt>
                  <dd>{r.not_recomputed.length ? <Ranges text={ranges(r.not_recomputed)} /> : 0}</dd>
                </div>
                <div>
                  <dt>
                    <Term k="chain-unread">Chain facts unread</Term>
                  </dt>
                  <dd>{r.unchecked}</dd>
                </div>
              </dl>
            </section>
            {r.errors.length ? (
              <section class="sheet">
                <h2>What did not match</h2>
                <ol class="mismatches mono">
                  {r.errors.map((e) => (
                    <li key={e}>{e}</li>
                  ))}
                </ol>
              </section>
            ) : (
              <section class="sheet checks-sheet">
                <h2>Every check passed</h2>
                <ul class="checks">
                  <li>
                    <b>{r.hours}</b> {noun(r.hours, "hour")} hashed and chained, each to the one before
                  </li>
                  <li>
                    <b>
                      {rebuilt.length} of {r.hours}
                    </b>{" "}
                    {noun(r.hours, "hour")} replayed run by run to the same requests
                  </li>
                  <li>
                    <b>
                      {rebuilt.length} of {r.hours}
                    </b>{" "}
                    {noun(r.hours, "hour")} rebuilt byte for byte from the hour before
                  </li>
                  <li>The library matches the manifest, and the exam its draw block</li>
                </ul>
              </section>
            )}
            <section class="sheet">
              <h2>What this pass covered</h2>
              <dl class="pairs">
                <div>
                  <dt>Replayed run by run</dt>
                  <dd class="mono">
                    <Ranges text={hourList(rebuilt) || "None"} />
                  </dd>
                </div>
                <div>
                  <dt>Chained and hashed only</dt>
                  <dd class="mono">
                    <Ranges text={hourList(result.not_loaded) || "None, every hour was replayed"} />
                  </dd>
                </div>
              </dl>
            </section>
            <section class="sheet">
              <h2>Anchors and draw blocks</h2>
              <p class="dim">
                {synthetic
                  ? "The synthetic league's chain file stands in for the chain."
                  : `Reads ${anchors.length} anchor transactions and the anchor address's nonce, about one request a second, from ${rpcs.length ? rpcs.map((u) => new URL(u).host).join(", then ") : "no RPC set"}.`}
              </p>
              {chainState.kind === "done" ? (
                <p class={`finished small${result?.ok ? " good" : ""}`}>Anchors checked against the chain</p>
              ) : (
                <button
                  type="button"
                  class="btn ink small"
                  disabled={chainState.kind === "busy"}
                  onClick={() => void checkChain()}
                >
                  Check anchors on chain
                </button>
              )}
              <p class="mono dim chain-state" role="status">
                {chainState.kind === "busy" && `${chainState.done} of about ${chainState.of} requests`}
                {chainState.kind === "failed" && `Stopped: ${chainState.why}`}
              </p>
            </section>
          </div>
        </>
      )}
      <Claims lab={lab} />
      <p class="limits">
        What this proves: each record was fixed by the time of its anchor, the records chain without a gap, and every
        number on this site follows from them under the rules the record names. What it cannot prove: that a model
        really said what a run file says, or that no other runs happened. For that, rerun the exam strains with your own
        key. The hourly draw trusts the Robinhood Chain sequencer for its block hash.
      </p>
    </main>
  );
}
