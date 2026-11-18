import { useEffect, useState } from "preact/hooks";
import { type Lab, loadAnchorTimes, loadCard } from "../record/load";
import { findStrain, focal, lineageOf, noun, type Pill, pillOf, routeOf } from "../record/stats";
import type { Gene, HourRecord, Story } from "../record/types";
import { hourSha, hourState, isLate, REASON, storyLines, upcoming, utc } from "../record/views";
import { DOT, eventLine, Ids, Placeholders, ROUTE_ORGAN, tollAt } from "./sections";
import { Term, type TermKey } from "./terms";
import { liveLine, WardPlate } from "./ward";

/** The value of `load` once it settles; undefined until then. */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[]): T | undefined {
  const [value, setValue] = useState<T | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    setValue(undefined);
    void load().then((v) => {
      if (alive) setValue(v);
    });
    return () => {
      alive = false;
    };
  }, deps);
  return value;
}

export const STORY: Record<Story, string> = {
  advisory: "Advisory",
  void: "Void hour",
  concern: "Variant of concern",
  outbreak: "Outbreak",
  exam: "Exam hour",
  gene: "Gene event",
  extinction: "Extinction",
  move: "Big move",
  quiet: "Hourly bulletin",
};
/** The lab word each story chip defines; a quiet hour's bulletin needs none. */
const STORY_TERM: Partial<Record<Story, TermKey>> = {
  advisory: "advisory",
  void: "void",
  concern: "concern",
  outbreak: "outbreak",
  exam: "exam",
  gene: "gene-event",
  extinction: "extinct",
  move: "big-move",
};

/** LATE, on time, or unknown (null) until the anchor's block time is known. */
export function useLate(lab: Lab, h: HourRecord): boolean | null {
  const times = useLoad(() => loadAnchorTimes(lab), []);
  const at = times?.get(h.n);
  return at === undefined ? null : isLate(lab.manifest, h.start, at);
}

/**
 * The hour's state as tags. With `terms` (the hour page's head) each tag defines its word, except `defined`, which the
 * story chip beside it already defines; the record's rows are links and pass neither, and leave out RECORDED, which
 * nearly every row would carry, so the exceptions stand out.
 */
export function StateTags({
  h,
  late,
  terms = false,
  defined,
}: {
  h: HourRecord;
  late: boolean | null;
  terms?: boolean;
  defined?: TermKey | undefined;
}) {
  const s = hourState(h);
  const word = (k: TermKey, text: string = k) => (terms && k !== defined ? <Term k={k}>{text}</Term> : text);
  return (
    <span class="tags">
      {s.kind === "ok" && terms && <em class="tag ok">{word("recorded")}</em>}
      {s.kind === "void" && (
        <em class="tag void">
          {word("void")} {s.reason ? `${DOT} ${REASON[s.reason]}` : ""}
        </em>
      )}
      {s.kind === "sealed" && <em class="tag sealed">{word("sealed")}</em>}
      {/* An exam hour whose story chip already says so takes no second exam label beside it. */}
      {h.exam && defined !== "exam" && <em class="tag exam">{word("exam")}</em>}
      {h.flags
        .filter((f) => f !== "sealed" && f !== s.reason)
        .map((f) => (
          <em key={f} class={`tag ${f}`}>
            {word(f)}
          </em>
        ))}
      {late && <em class="tag late">{word("late", "LATE")}</em>}
    </span>
  );
}

/** A template with its target placeholders drawn as chips. */
export function Template({ text }: { text: string }) {
  return (
    <p class="template mono">
      <Placeholders text={text} />
    </p>
  );
}

const PILLS: [Pill, string][] = [
  ["breached", "breached"],
  ["held", "held"],
  ["resisted", "resisted"],
  ["void", "void"],
];

/** The hour's twenty attacks by state pill; a state no attack ended in is outlined, not filled, since it did not happen. */
function Tally({ h }: { h: HourRecord }) {
  const shown = h.flags.includes("sealed") ? [] : h.outcomes;
  const pills = shown.filter((o) => o.kind === "attack").map((o) => pillOf(o.outcome));
  return (
    <dl class="tally">
      {PILLS.map(([pill, label]) => {
        const count = pills.filter((p) => p === pill).length;
        return (
          <div key={pill} class={count ? pill : `${pill} zero`}>
            <dt class="mono">
              <Term k={pill}>{label}</Term>
            </dt>
            <dd>{count}</dd>
          </div>
        );
      })}
    </dl>
  );
}

const ELLIPSIS = String.fromCodePoint(0x2026);

/** A rule change with each gene's name beside its code, "gained I09 (Encoded text is data)", so none is looked up. */
const geneNames = (line: string, genes: Gene[]) =>
  line.replace(/\b[IS]\d{2}\b/g, (id) => {
    const name = genes.find((g) => g.id === id)?.name;
    return name ? `${id} (${name})` : id;
  });

/** A long hash cut to its two ends, with the whole of it in the title and on copy; /verify prints it in full. */
export function Hash({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <span class="hash-short">
      <span title={value}>{value.length > 24 ? `${value.slice(0, 10)}${ELLIPSIS}${value.slice(-8)}` : value}</span>
      <button
        type="button"
        class="copy-hash"
        aria-label={copied ? "Copied" : `Copy ${value}`}
        onClick={() =>
          navigator.clipboard?.writeText(value).then(
            () => setCopied(true),
            () => setCopied(false),
          )
        }
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </span>
  );
}

export function HourPage({ lab, n }: { lab: Lab; n: number }) {
  const h = lab.hours[n];
  const card = useLoad(() => loadCard(n), [n]);
  const late = useLate(lab, h);
  const sha = hourSha(lab, n);
  const anchor = lab.anchors.find((a) => a.kind === "hour" && a.n === n);
  const strain = h.strain ? findStrain(h.strain, lab.strains, lab.hours) : undefined;
  const named = h.strain ? h.state.concerns[lineageOf(h.strain)] : undefined;
  const focalBody = card?.focal ?? focal(lab.hours.slice(0, n + 1));
  const { line, notes } = storyLines(card, h);
  const reason = hourState(h).reason;
  const events = h.events.filter((e) => e.type !== "remutated");
  const remutated = h.events.filter((e) => e.type === "remutated");
  const last = lab.hours.length - 1;
  // A long hour folds its gene changes into one line, so its card does not leave the cards beside it half empty.
  const fold = events.length + remutated.length > 6 && remutated.length > 1;
  // On the latest hour the next one takes the After slot: when its strain is drawn.
  const next = n === last ? upcoming(lab.manifest, h, Date.now() / 1000) : null;
  const storyTerm = card ? STORY_TERM[card.story] : undefined;
  return (
    <main class="page hour-page">
      <nav class="crumbs mono" aria-label="Where you are">
        <a href="/record">Record</a>
        <span>/</span>
        <b>Hour {n}</b>
        <span class="step">
          {n > 0 && <a href={`/hour/${n - 1}`}>Earlier</a>}
          {n < last && <a href={`/hour/${n + 1}`}>Later</a>}
        </span>
      </nav>
      <header class="page-head split">
        <div>
          <h1>
            <Ids text={line} />
          </h1>
          {notes.map((t) => (
            <p key={t} class="note">
              Note: {t}
            </p>
          ))}
          {/* The hour's time, its story and its state, under the headline they qualify. */}
          <p class="hour-meta">
            <span class="mono dim">
              Hour {n} {DOT} <span class="unbroken">{utc(h.start)}</span>
            </span>
            {card && (
              <span class={`story mono ${card.story}`}>
                {storyTerm ? <Term k={storyTerm}>{STORY[card.story]}</Term> : STORY[card.story]}
              </span>
            )}
            <StateTags h={h} late={late} terms defined={storyTerm} />
          </p>
        </div>
        <Tally h={h} />
      </header>
      <WardPlate
        lab={lab}
        record={h}
        focalBody={focalBody?.n === n ? focalBody.body : null}
        href={(id) => `/body/${id}?hour=${n}`}
        class="hour-ward"
      />
      <p class="plate-hint dim">Open any patient to see what it read, thought and did in hour {n}.</p>
      <div class="hour-cards">
        <section class="sheet">
          <p class="kicker mono">The strain</p>
          {h.strain ? (
            <>
              <h2>
                <a href={`/strain/${h.strain}`}>{h.strain}</a>
              </h2>
              <p class="chips">
                <span class={`chip route-${routeOf(h.strain)}`}>
                  <Term k="way-in">{ROUTE_ORGAN[routeOf(h.strain)]}</Term>
                </span>
                {strain?.technique && <span class="chip">{strain.technique.replaceAll("_", " ")}</span>}
                {strain?.origin === "mutant" && <span class="chip dark">mutant of {strain.parent}</span>}
                {named && <span class="chip concern">{named}</span>}
              </p>
              <dl class="pairs">
                <div>
                  <dt>
                    <Term k="lineage">Lineage</Term>
                  </dt>
                  <dd>{lineageOf(h.strain)}</dd>
                </div>
                <div>
                  <dt>
                    <Term k="toll">Toll</Term> so far
                  </dt>
                  <dd>{tollAt(lab.hours, n)}</dd>
                </div>
              </dl>
              {strain && <Template text={strain.template} />}
              <a class="btn ghost small" href={`/strain/${h.strain}`}>
                The whole strain
              </a>
            </>
          ) : (
            <>
              <h2>No strain ran</h2>
              <p class="fine">
                Every run of this hour is in the record as void
                {reason ? `, with the reason "${REASON[reason]}"` : ""}. The scam pool and the defence rules carry over
                unchanged to hour {n + 1}, and the hour counts toward the void share the record shows.
              </p>
            </>
          )}
        </section>
        <section class="sheet">
          <p class="kicker mono">The record</p>
          <h2>Record {n}</h2>
          <dl class="pairs mono">
            <div>
              <dt>
                <Term k="hash">Fingerprint</Term> of hours/{n}.json
              </dt>
              <dd>{sha ? <Hash value={sha} /> : "not yet chained"}</dd>
            </div>
            <div>
              <dt>
                <Term k="anchor">Anchor</Term>
              </dt>
              <dd>{anchor ? <Hash value={anchor.tx} /> : "not landed yet"}</dd>
            </div>
            {anchor && (
              <div>
                <dt>Anchor block</dt>
                <dd>
                  {anchor.block.toLocaleString("en-US")}
                  {late === null ? "" : late ? ` ${DOT} LATE` : ` ${DOT} on time`}
                </dd>
              </div>
            )}
            <div>
              <dt>
                <Term k="draw">Draw</Term> block
              </dt>
              <dd>{h.draw ? `${h.draw.block.toLocaleString("en-US")} ${DOT} ${h.draw.hash.slice(0, 16)}` : "none"}</dd>
            </div>
            <div>
              <dt>
                <Term k="void-share">Void share</Term>
              </dt>
              <dd>{(h.void_share_ppm / 10_000).toFixed(1)}%</dd>
            </div>
          </dl>
          <a class="btn ghost small" href={`/card/${n}`}>
            Open the hour's card
          </a>
        </section>
        <section class="sheet">
          <p class="kicker mono">What changed</p>
          <h2>
            {events.length + remutated.length
              ? `${events.length + remutated.length} ${noun(events.length + remutated.length, "event")}`
              : "No events"}
          </h2>
          <ul class="events">
            {events.map((e) => (
              <li key={eventLine(e, lab.manifest)} class={`ev ${e.type}`}>
                <Ids text={eventLine(e, lab.manifest)} />
              </li>
            ))}
            {fold ? (
              <li class="ev remutated">
                <details>
                  <summary>{remutated.length} rule changes in the Venin rows</summary>
                  <ul class="ev-genes">
                    {remutated.map((e) => (
                      <li key={eventLine(e, lab.manifest)}>
                        <Ids text={geneNames(eventLine(e, lab.manifest), lab.genes)} />
                      </li>
                    ))}
                  </ul>
                </details>
              </li>
            ) : (
              remutated.map((e) => (
                <li key={eventLine(e, lab.manifest)} class="ev remutated">
                  <Ids text={geneNames(eventLine(e, lab.manifest), lab.genes)} />
                </li>
              ))
            )}
            {events.length + remutated.length === 0 && (
              <li class="ev quiet">The scam pool and the defence rules stayed as they were.</li>
            )}
          </ul>
          <div class="near-hours">
            {[n - 1, n + 1]
              .filter((i) => i >= 0 && i <= last)
              .map((i) => (
                <a key={i} href={`/hour/${i}`}>
                  <span class="mono dim">
                    {i < n ? "Before" : "After"}
                    {lab.hours[i].exam ? ` ${DOT} exam hour` : ""}
                  </span>
                  <span>
                    <Ids text={liveLine(lab.hours[i], false)} />
                  </span>
                </a>
              ))}
            {next && (
              <p class="next">
                <span class="mono dim">Next</span>
                <span>
                  Hour {next.n} {DOT} drawn at {new Date(next.start * 1000).toISOString().slice(11, 16)} UTC
                </span>
              </p>
            )}
          </div>
        </section>
      </div>
    </main>
  );
}
