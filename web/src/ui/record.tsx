import { useMemo } from "preact/hooks";
import { type Lab, loadAnchorTimes } from "../record/load";
import { bodyState, noun, pct, ROW_LABEL, ROWS } from "../record/stats";
import type { HourRecord } from "../record/types";
import { hourSha, hourState, isLate, uptime, utc, voidRates, wardOrder } from "../record/views";
import { Hash, StateTags, useLoad } from "./hour";
import { chainName, DOT, eventLine, Ids } from "./sections";
import { Term } from "./terms";

const fmt = (n: number) => n.toLocaleString("en-US");
const breached = (pills: Map<string, string>) => [...pills.values()].filter((p) => p === "breached").length;
/** What changed in the pool, as the hour feed tells it: the gene shuffles stay on the hour's own page. */
const changes = (h: HourRecord) => h.events.filter((e) => e.type !== "remutated" && e.type !== "kept").slice(0, 2);

export function RecordPage({ lab }: { lab: Lab }) {
  const times = useLoad(() => loadAnchorTimes(lab), []);
  const voids = useMemo(() => voidRates(lab.hours), [lab.hours]);
  const up = uptime(lab.hours);
  const lateOf = (n: number) => {
    const at = times?.get(n);
    return at === undefined ? null : isLate(lab.manifest, lab.hours[n].start, at);
  };
  const late = times ? lab.hours.filter((h) => lateOf(h.n)).length : null;
  const runs = [...voids.values()].reduce((a, t) => ({ hits: a.hits + t.hits, total: a.total + t.total }), {
    hits: 0,
    total: 0,
  });
  const order = wardOrder(lab);
  const hourly = lab.anchors.filter((a) => a.kind === "hour").length;
  return (
    <main class="page record-page">
      <header class="page-head split">
        <div>
          <h1>Every hour, as it was written</h1>
          <p class="lead">
            One <Term k="record">record</Term> per hour, even when nothing ran. Each is fingerprinted, chained to the
            hour before, and anchored on chain before the next one starts. Nothing is filled in afterwards.
          </p>
        </div>
        <div class="head-actions">
          <a class="btn ink" href="/verify">
            Verify in your browser
          </a>
          <p class="dim">Replays every run and recomputes every number from these files.</p>
        </div>
      </header>
      <div class="counts">
        <p>
          <b>{fmt(lab.hours.length)}</b>
          <span class="mono">{noun(lab.hours.length, "hour")} recorded</span>
        </p>
        <p>
          <b>{pct(up.total ? up.hits / up.total : 0)}</b>
          <span class="mono">
            <Term k="uptime">uptime</Term> {DOT} hours that counted
          </span>
        </p>
        <p>
          <b>{pct(runs.total ? runs.hits / runs.total : 0)}</b>
          <span class="mono">
            <Term k="void-share">of runs void</Term>
          </span>
        </p>
        <p>
          <b>{late === null ? "-" : fmt(late)}</b>
          <span class="mono">
            <Term k="late">LATE</Term>{" "}
            {late === null ? "hours: checked by verify" : `${noun(late, "hour")}, off the scoreboard`}
          </span>
        </p>
      </div>
      <section class="sheet voids">
        <div class="sheet-head">
          <h2>{runs.hits ? "Runs that did not count, by defence and model" : "Runs that did not count"}</h2>
          <dl class="pairs mono inline">
            <div>
              <dt>
                <Term k="anchor">Anchor</Term> address
              </dt>
              <dd>
                <Hash value={lab.manifest.anchor.address} />
              </dd>
            </div>
            <div>
              <dt>Chain</dt>
              <dd>{chainName(lab.manifest.anchor.chain_id)}</dd>
            </div>
            <div>
              <dt>Anchors</dt>
              <dd>
                {fmt(lab.anchors.length)} {noun(lab.anchors.length, "anchor")}, {fmt(hourly)} of them hourly
              </dd>
            </div>
          </dl>
        </div>
        {runs.hits === 0 ? (
          <p class="void-none">
            <b>0</b> of {fmt(runs.total)} runs void in {fmt(lab.hours.length)} {noun(lab.hours.length, "hour")}: every
            run counted.
          </p>
        ) : (
          <div class="void-grid" style={{ "--cols": lab.manifest.models.length }}>
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
                  const t = voids.get(`${m.column}-${row}`) ?? { hits: 0, total: 0 };
                  const share = t.total ? t.hits / t.total : 0;
                  return (
                    <span key={m.column} class="void-cell" style={{ "--v": share }}>
                      <b>{pct(share)}</b>
                      <small class="mono dim">
                        {t.hits} of {t.total}
                      </small>
                    </span>
                  );
                })}
              </>
            ))}
          </div>
        )}
      </section>
      <section class="ledger-list">
        {/* The key stays on phones, where the column head is hidden. */}
        <p class="tag-key mono">
          Tags:{" "}
          <em class="tag exam">
            <Term k="exam">exam</Term>
          </em>{" "}
          <em class="tag late">
            <Term k="late">LATE</Term>
          </em>{" "}
          <em class="tag sealed">
            <Term k="sealed">sealed</Term>
          </em>{" "}
          <em class="tag void">
            <Term k="void">void</Term>
          </em>
        </p>
        <div class="rec-row rec-head mono">
          <span>Hour</span>
          <span>
            <Term k="strain">Strain</Term>
          </span>
          <span>What changed {DOT} state</span>
          <span>
            <Term k="ward">Ward</Term> {DOT} breached
          </span>
          <span>
            <Term k="hash">Fingerprint</Term>
          </span>
          <span>
            <Term k="anchor">Anchor</Term>
          </span>
        </div>
        {[...lab.hours].reverse().map((h) => {
          const anchor = lab.anchors.find((a) => a.kind === "hour" && a.n === h.n);
          const byId = new Map(lab.manifest.bodies.map((b) => [b.id, bodyState(h, b.id).pill]));
          return (
            <a key={h.n} class="rec-row" href={`/hour/${h.n}`}>
              <span class="rec-n">
                <b>{h.n}</b>
                <small class="mono dim">{utc(h.start)}</small>
              </span>
              <span class="rec-strain">
                {h.strain ?? <span class="dim">{hourState(h).kind === "sealed" ? "Sealed" : "Void hour"}</span>}
              </span>
              {/* What changed starts the column, so its lines share one left edge; the state tags end it. */}
              <span class="rec-state">
                <span class="rec-what">
                  {changes(h).length ? (
                    changes(h).map((e) => (
                      <span key={eventLine(e, lab.manifest)}>
                        <Ids text={eventLine(e, lab.manifest, true)} />
                      </span>
                    ))
                  ) : (
                    <span class="dim">No change to the scam pool</span>
                  )}
                </span>
                <StateTags h={h} late={lateOf(h.n)} />
              </span>
              <span class="rec-ward">
                <span class="mini" aria-hidden="true">
                  {order.map((id) => (
                    <i key={id} class={byId.get(id) ?? "void"} />
                  ))}
                </span>
                <b class={breached(byId) ? "hit" : ""}>{breached(byId)}</b>
              </span>
              <span class="mono rec-hash">{hourSha(lab, h.n)?.slice(0, 16) ?? "-"}</span>
              <span class="mono rec-hash dim">
                {anchor ? `${anchor.tx.slice(0, 12)} ${DOT} ${fmt(anchor.block)}` : "pending"}
              </span>
            </a>
          );
        })}
      </section>
    </main>
  );
}
