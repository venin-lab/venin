import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { DATA_URL, type Lab, loadCard } from "../record/load";
import { bodyState, columnOf, focal, ROW_LABEL, routeOf, rowOf } from "../record/stats";
import { hourSha, storyLines, utc } from "../record/views";
import { loadAnatomy, ORGAN_ANCHORS } from "../three/anatomy";
import { BodyView, drawStill, warmBody } from "../three/body";
import { figureState } from "../three/ward";
import { leaguePill } from "./header";
import { STORY, useLate, useLoad } from "./hour";
import { Mark } from "./mark";
import { DOT, Ids, metBy, ROUTE_ORGAN } from "./sections";
import { pillText, WardPlate } from "./ward";

/** The standing focal patient, its ink run to the end. */
function Focal({ lab, n, body, onReady }: { lab: Lab; n: number; body: string; onReady: () => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let view: BodyView | null = null;
    let alive = true;
    const ready = () =>
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (alive) onReady();
        }),
      );
    // Without the anatomy the card keeps its words and is ready without the figure.
    loadAnatomy().then((anatomy) => {
      const el = canvas.current;
      if (!alive || !el) return;
      const state = figureState(lab.hours[n], body);
      try {
        // A still: no roll or beat, so the picture does not depend on when it was taken.
        view = new BodyView(el, anatomy, () => {}, { still: true });
      } catch {
        // Without WebGL the same finished patient is drawn once on a 2D canvas.
        void warmBody(anatomy).then((cloud) => {
          if (alive) drawStill(el, cloud, ORGAN_ANCHORS(anatomy), state);
          ready();
        });
        return;
      }
      view.resize(el.clientWidth, el.clientHeight);
      view.focus(null, false, 0);
      view.show(state, -100, -100);
      ready();
    }, ready);
    return () => {
      alive = false;
      view?.dispose();
    };
  }, [n, body]);
  return <canvas ref={canvas} class="card-focal-canvas" />;
}

/**
 * How much of its 1600x900 the card gets in this window. The window's own width, not innerWidth: once a page is wider
 * than a phone, innerWidth is the zoomed-out width.
 */
function fitNow(): number {
  const { clientWidth: w, clientHeight: h } = document.documentElement;
  return Math.min(1, w / 1600, h / 900);
}

/** Under this share of its size the card is too small to read: its words follow it, and the runner's picture stands in. */
const SMALL = 0.6;

/** The hourly card at 1600x900: the frame band, the plate with the focal patient, the ward in miniature, the record. */
export function CardPage({ lab, n }: { lab: Lab; n: number }) {
  const h = lab.hours[n];
  const card = useLoad(() => loadCard(n), [n]);
  const late = useLate(lab, h);
  const [drawn, setDrawn] = useState(0);
  const [fonts, setFonts] = useState(false);
  const [fit, setFit] = useState(fitNow);
  const [shown, setShown] = useState(false);
  const [broken, setBroken] = useState(false);
  const f = card?.focal ?? focal(lab.hours.slice(0, n + 1));
  const sha = hourSha(lab, n);
  const anchor = lab.anchors.find((a) => a.kind === "hour" && a.n === n);
  const tag = leaguePill(lab.manifest.league);
  const { line, notes } = storyLines(card, h);
  const fState = f ? bodyState(lab.hours[f.n], f.body) : null;
  const model = f ? lab.manifest.models.find((m) => m.column === columnOf(f.body)) : undefined;
  // A gene by its code and its name, so the phone's words need no lookup.
  const geneNamed = (id: string) => {
    const name = lab.genes.find((g) => g.id === id)?.name;
    return name ? `${id} (${name})` : id;
  };
  const step = () => setDrawn((d) => d + 1);
  // The picture the runner drew of this card, offered on a phone once it is there: a server that answers a missing
  // file with the app page still says 200, so an HTML answer counts as missing.
  const png = `${DATA_URL}/cards/${n}.png`;
  const saved = useLoad(
    () =>
      fetch(png, { method: "HEAD" })
        .then((r) => r.ok && !(r.headers.get("content-type") ?? "").includes("text/html"))
        .catch(() => false),
    [n],
  );
  const small = fit < SMALL;
  // Small, the card is the runner's picture: twenty point clouds drawn that small pile up into dark shapes, while the
  // picture, drawn at full size, keeps its pencil. Until the server says whether it has one, the frame waits empty.
  // An hour with no picture yet is drawn live, its ward thinned to the device pixels its beds get.
  const picture = small && saved === true && !broken;
  const density = Math.min(1, (fit * Math.min(devicePixelRatio, 2)) ** 2);
  const live = !small || saved === false || broken;
  const ready = fonts && card !== undefined && (picture ? shown : live && drawn >= (f ? 2 : 1));

  // Before the first paint, so a phone never lays out the card at 1600px and zooms out to fit it.
  useLayoutEffect(() => {
    void document.fonts.ready.then(() => setFonts(true));
    const scale = () => {
      const at = fitNow();
      document.documentElement.style.setProperty("--fit", String(at));
      setFit(at);
    };
    scale();
    addEventListener("resize", scale);
    return () => {
      removeEventListener("resize", scale);
    };
  }, []);

  // The words are read once: from the card when it is drawn live, from the words under it when it is a picture.
  const Line = picture ? "h1" : "p";
  const echo = picture ? undefined : true;
  const liveCard = (
    <>
      <div class="card-plate">
        <div class="card-left">
          {card === undefined ? null : f && fState ? (
            <>
              <Focal lab={lab} n={f.n} body={f.body} onReady={step} />
              <div class="card-focal">
                <span class={`pill ${fState.pill}`}>{pillText(fState)}</span>
                <p>
                  <b>{ROW_LABEL[rowOf(f.body)]}</b> on {model?.label}
                </p>
                <p class="mono dim">
                  {metBy(fState)}
                  {f.n !== n ? ` ${DOT} hour ${f.n}` : ""}
                </p>
              </div>
            </>
          ) : (
            <p class="mono dim">No run has counted yet.</p>
          )}
        </div>
        <div class="card-main">
          <div class="card-top mono">
            <span>
              Hour {n}
              {h.strain ? ` ${DOT} ${h.strain} ${DOT} ${ROUTE_ORGAN[routeOf(h.strain)]}` : ""}
            </span>
            {card && <span class={`story ${card.story}`}>{STORY[card.story]}</span>}
            {tag && <span class="league">{tag}</span>}
          </div>
          {/* Over 100 characters the caption steps down a size, so it sits on two lines instead of three. */}
          <h1 class={`card-story${line.length > 100 ? " long" : ""}`}>
            <Ids text={line} />
          </h1>
          {notes.map((t) => (
            <p key={t} class="note mono">
              Note: {t}
            </p>
          ))}
          <WardPlate
            lab={lab}
            record={h}
            focalBody={f?.n === n ? f.body : null}
            still
            density={density}
            onReady={step}
            class="card-ward"
          />
          <dl class="card-foot mono">
            <div>
              <dt>Record {n}</dt>
              <dd>{sha ? sha.slice(0, 16) : "pending"}</dd>
            </div>
            <div>
              <dt>Anchor{late ? " (LATE)" : ""}</dt>
              <dd>{anchor ? `${anchor.tx.slice(0, 18)} ${DOT} block ${anchor.block}` : "pending"}</dd>
            </div>
            <div>
              <dt>Time</dt>
              <dd>{utc(h.start)}</dd>
            </div>
          </dl>
        </div>
      </div>
      <div class="card-band">
        <span class="card-brand">
          <Mark size={48} class="card-mark" />
          <img class="card-word" src="/brand/venin-wordmark-light.svg" alt="venin" width="118" height="32" />
        </span>
        <span class="card-url mono">venin.io</span>
      </div>
    </>
  );
  return (
    <main class={`card-view${small ? " small" : ""}`} data-ready={ready ? "1" : undefined}>
      <div class={`card-page${picture ? " picture" : ""}`}>
        {picture ? (
          <img
            class="card-png"
            src={png}
            alt=""
            width={1600}
            height={900}
            onLoad={() => setShown(true)}
            onError={() => setBroken(true)}
          />
        ) : live ? (
          liveCard
        ) : null}
      </div>
      {/* Only when the card is too small to read: its words again, a way back and the picture. */}
      <div class="card-back">
        <Line class="card-back-line" aria-hidden={echo}>
          <Ids text={line} />
        </Line>
        <p class="card-back-date mono" aria-hidden={echo}>
          Hour {n} {DOT} <span class="unbroken">{utc(h.start)}</span>
        </p>
        {notes.map((t) => (
          <p key={t} class="note" aria-hidden={echo}>
            Note: {t}
          </p>
        ))}
        {f && fState && (
          <p class="mono dim">
            {ROW_LABEL[rowOf(f.body)]} on {model?.label} {DOT}{" "}
            {fState.pill === "held" && fState.gene ? `held by ${geneNamed(fState.gene)}` : fState.pill} {DOT}{" "}
            {metBy(fState)}
            {f.n !== n ? ` ${DOT} hour ${f.n}` : ""}
          </p>
        )}
        <p class="card-back-actions">
          <a class="btn ghost small" href={`/hour/${n}`}>
            Back to hour {n}
          </a>
          {saved && (
            <a class="btn ghost small" href={png} download={`venin-hour-${n}.png`}>
              Save image
            </a>
          )}
        </p>
      </div>
    </main>
  );
}
