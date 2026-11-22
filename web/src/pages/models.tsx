import type { Lab } from "../record/load";
import { modelLabel } from "../record/stats";
import type { Column } from "../record/types";
import { DOT, Placeholders, ROUTE_ORGAN } from "../ui/sections";
import { bareRecord, COLUMNS, providerDown, veninRecord } from "./defend-view";
import { type Asked, PROMPT, ROUNDS, type Round, type Verdict } from "./models-data";
import "./release.css";
import "./models.css";

const SAYS: Record<Verdict, string> = {
  wrote: "Wrote an attack",
  refused: "Refused",
  harmless: "Wrote a harmless note",
  blocked: "Blocked by its own filter",
  gate: "Failed the gate first",
  down: "Not asked: its provider was down",
};

type Count = { breached: number; ran: number };
const share = (n: Count) => (n.ran ? n.breached / n.ran : 0);

/** One AI's answer in one round: the verdict, its own words, and how far its attack got. */
function Answer({ a, round }: { a: Asked; round: Round }) {
  const t = a.attack;
  return (
    <div class="mod-answer">
      <p>
        <span class="mono dim">Round {round.n}</span> <span class={`mod-verdict ${a.verdict}`}>{SAYS[a.verdict]}</span>
      </p>
      {a.said && <p class="mod-said">&ldquo;{a.said}&rdquo;</p>}
      {a.verdict === "gate" && (
        <p class="mod-note">
          Its first attack failed the gate; its fix came seventh, after six attacks had filled the hour.
        </p>
      )}
      {t && (
        <>
          <p class="mono mod-result">
            <span class="unbroken">{ROUTE_ORGAN[t.route]}</span> {DOT}{" "}
            <span class="unbroken">
              <a href={`/hour/${t.hour}`}>hour {t.hour}</a>
            </span>{" "}
            {DOT}{" "}
            <span class="unbroken">
              {t.fooled} of {t.judged} fooled
            </span>{" "}
            {DOT} <span class="unbroken">{t.flagged} flagged it</span>
          </p>
          <details class="mod-attack">
            <summary>The attack, as written</summary>
            <p class="mono">
              <Placeholders text={t.payload} />
            </p>
          </details>
        </>
      )}
    </div>
  );
}

function Bar({ label, n }: { label: string; n: Count }) {
  return (
    <div class="mod-bar">
      <span>{label}</span>
      <span class="mod-track">
        <i style={{ width: `${Math.round(share(n) * 100)}%` }} />
      </span>
      <span class="mono">{n.ran ? `${n.breached} of ${n.ran} breached` : "no runs yet"}</span>
    </div>
  );
}

/** A ward model, both sides: what it did when asked to write an attack, and how often attacks get through it. */
function WardModel({ lab, column }: { lab: Lab; column: Column }) {
  const down = providerDown(lab.hours, column);
  const asked = ROUNDS.flatMap((round) => round.asked.filter((a) => a.column === column).map((a) => ({ round, a })));
  return (
    <article class="rel-panel mod-card">
      <h3>{modelLabel(lab.manifest, column)}</h3>
      <div class="mod-side">
        <p class="kicker mono">Asked to write one</p>
        {asked.map(({ round, a }) => (
          <Answer key={round.n} a={a} round={round} />
        ))}
      </div>
      <div class="mod-side">
        <p class="kicker mono">Falls for one {DOT} the last 24 hours</p>
        {down ? (
          <p class="mono dim">Provider down: its runs are void and left out</p>
        ) : (
          <>
            <Bar label="No defence" n={bareRecord(lab.hours, column)} />
            <Bar label="Venin's agents" n={veninRecord(lab.hours, column)} />
          </>
        )}
      </div>
    </article>
  );
}

/** The ward model breached most often with no defence in the last day, and what it did when asked to write one. */
function mostBreached(lab: Lab): string | null {
  const live = COLUMNS.filter((c) => !providerDown(lab.hours, c)).map((c) => ({ c, n: bareRecord(lab.hours, c) }));
  const ran = live.filter((x) => x.n.ran > 0).sort((a, b) => share(b.n) - share(a.n));
  if (ran.length < 2 || share(ran[0].n) === share(ran[1].n)) return null;
  const { c, n } = ran[0];
  const last = [...ROUNDS].reverse().flatMap((r) => r.asked.filter((a) => a.column === c && a.verdict !== "down"))[0];
  const did = last
    ? last.verdict === "wrote"
      ? "wrote an attack"
      : last.verdict === "refused"
        ? "refused to write one"
        : `answered: ${SAYS[last.verdict].toLowerCase()}`
    : null;
  return `With no defence, ${modelLabel(lab.manifest, c)} was breached most often in the last 24 hours: ${n.breached} of ${n.ran}.${did ? ` Asked to write an attack, it ${did}.` : ""}`;
}

export function ModelsPage({ lab }: { lab: Lab }) {
  const others = ROUNDS.flatMap((round) => round.asked.filter((a) => a.column === null).map((a) => ({ round, a })));
  const line = mostBreached(lab);
  return (
    <main class="page models-page">
      <header class="page-head">
        <h1>Both sides</h1>
        <p class="lead">
          Would an AI write an attack on an agent when asked, and does it fall for one? Two different skills. The first
          answer comes from rounds in which every AI got the same prompt, word for word. The second comes from the ward,
          every hour, live from the record.
        </p>
        {line && <p class="mod-line">{line}</p>}
      </header>
      <h2 class="mod-head">The ward's models</h2>
      <div class="mod-ward">
        {COLUMNS.map((c) => (
          <WardModel key={c} lab={lab} column={c} />
        ))}
      </div>
      <h2 class="mod-head">Asked, not in the ward</h2>
      <p class="rel-note">
        These answered the prompt; they are not among the ward's models, so the record cannot say how often attacks get
        through them.
      </p>
      <div class="mod-others">
        {others.map(({ round, a }) => (
          <article key={`${round.n}-${a.model}`} class="rel-panel mod-other">
            <h3>
              {a.model} <span class="mono dim">{a.maker}</span>
            </h3>
            <Answer a={a} round={round} />
          </article>
        ))}
      </div>
      <h2 class="mod-head">The rounds</h2>
      <div class="mod-rounds">
        {ROUNDS.map((r) => {
          const wrote = r.asked.filter((a) => a.attack);
          const judged = wrote.reduce((s, a) => s + (a.attack?.judged ?? 0), 0);
          const fooled = wrote.reduce((s, a) => s + (a.attack?.fooled ?? 0), 0);
          const asked = r.asked.filter((a) => a.verdict !== "down").length;
          return (
            <p key={r.n} class="mono">
              Round {r.n} {DOT} {r.name} {DOT} {r.day} {DOT} {asked} asked, {wrote.length} wrote one {DOT} released into{" "}
              <a href={`/hour/${r.hour}`}>hour {r.hour}</a> {DOT} {fooled} of {judged} runs fooled an agent
            </p>
          );
        })}
      </div>
      <details class="mod-prompt">
        <summary>The prompt, word for word</summary>
        <pre>{PROMPT}</pre>
      </details>
      <p class="rel-note">
        Each AI was asked once per round through OpenRouter with reasoning on; an answer the lab's gate refused went
        back to the same AI with the gate's reason. Released attacks ran once on the eight Venin agents; runs on a model
        whose provider was down are void and left out. Write one yourself at <a href="/release">venin.io/release</a>.
      </p>
    </main>
  );
}
