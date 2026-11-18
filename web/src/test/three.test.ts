import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { HourRecord, Outcome } from "../record/types";
import { type Anatomy, buildCloud, eyesAndEars } from "../three/anatomy";
import { labelX, ORGANS, organTone, spread } from "../three/body";
import { ease, Flight, frame } from "../three/camera";
import { arrival, chainAt, knotTime, PART, reachTime } from "../three/ink";
import { indexAt, playIndex, slots, tick } from "../three/ledger";
import { figureState, stateVector } from "../three/ward";

describe("venom front", () => {
  it("runs 3.6 s at rate 1 and stops at the reach", () => {
    expect(knotTime(1)).toBeCloseTo(0.4);
    expect(knotTime(3)).toBeCloseTo(1.3);
    expect(knotTime(5)).toBeCloseTo(2.25);
    expect(knotTime(6.6)).toBeCloseTo(3.1);
    expect(knotTime(7)).toBeCloseTo(3.6);
    expect(reachTime(0)).toBe(-1);
    expect(reachTime(7)).toBeCloseTo(3.6);
  });

  it("fills organs like ink in paper and conducts nerves evenly", () => {
    expect(arrival("EAR", PART.ears, 0.5)).toBeCloseTo(0.1);
    expect(arrival("EAR", PART.nerveEar, 0.5)).toBeCloseTo(0.575);
    expect(arrival("SKL", PART.nerveHands, 1), "the hand end inks first").toBeCloseTo(0.4);
    expect(arrival("EYE", PART.ears, 0.5)).toBe(-1);
    const eye = [
      PART.eyes,
      PART.nerveEye,
      PART.brain,
      PART.nerveHands,
      PART.hands,
      PART.nerveHeart,
      PART.heart,
      PART.artery,
    ].map((p) => arrival("EYE", p, 1));
    expect(eye).toEqual([...eye].sort((a, b) => a - b));
  });

  it("stops a held body's ink at the hands and never starts one the attack did not reach", () => {
    const s = (pill: "held" | "resisted", reach: number) =>
      ({ body: "m1-venin1", pill, gene: null, reach, route: "EAR" }) as const;
    expect(stateVector(s("held", 5))[0]).toBeCloseTo(2.25);
    expect(stateVector(s("held", 5))[1]).toBe(1);
    expect(stateVector(s("resisted", 0))[0]).toBe(-1);
  });
});

describe("a brain the run never reached", () => {
  const hour = (strain: string, o: Partial<Outcome>) =>
    ({
      n: 3,
      strain,
      flags: [],
      outcomes: [
        {
          body: "m1-venin1",
          kind: "attack",
          outcome: { class: "resisted", exposed: true, reached_brain: false, fooled: false, held: null, ...o },
        },
      ],
    }) as unknown as HourRecord;
  const tones = (h: HourRecord) => ORGANS.map((o) => organTone(figureState(h, "m1-venin1"), o));

  it("stays umber while the ink runs past it to the hands, as the record says", () => {
    // ORGANS: eyes, ears, brain, hands, heart. Acted for the attacker without repeating the code word.
    const acted = hour("EAR-1.0", { fooled: true, held: "S09" });
    expect(tones(acted)).toEqual(["", "ink", "", "held", ""]);
    expect(stateVector(figureState(acted, "m1-venin1"))[1], "held hands, spared brain").toBe(3);
    const breached = hour("EYE-2.0", { class: "infected", fooled: true, breached: true });
    expect(tones(breached)).toEqual(["ink", "", "", "ink", "ink"]);
  });

  it("still inks once the run repeats the code word, and a memory strain still enters there", () => {
    expect(tones(hour("EAR-1.0", { reached_brain: true, fooled: true, held: "S09" }))).toEqual([
      "",
      "ink",
      "ink",
      "held",
      "",
    ]);
    expect(stateVector(figureState(hour("EAR-1.0", { reached_brain: true }), "m1-venin1"))[1]).toBe(0);
    expect(tones(hour("MEM-1.0", { fooled: true })), "the way in is the brain").toEqual(["", "", "ink", "ink", ""]);
  });
});

describe("anatomy", () => {
  const meta = JSON.parse(readFileSync(new URL("../../public/anatomy/anatomy.json", import.meta.url), "utf8"));
  const bin = readFileSync(new URL("../../public/anatomy/anatomy.bin", import.meta.url));
  const a = { ...meta, raw: new Int16Array(bin.buffer, bin.byteOffset, bin.byteLength / 2) } as Anatomy;

  it("splits the eyeballs from the ears and starts each nerve on its own organ", () => {
    const { eyes, ears } = eyesAndEars(a);
    expect(eyes.every(([x]) => Math.abs(x) < 0.058)).toBe(true);
    expect(ears.every(([x]) => Math.abs(x) >= 0.058)).toBe(true);
    const c = buildCloud(a, false);
    const starts = (part: number) =>
      Array.from(c.part.keys())
        .filter((i) => c.part[i] === part && c.u[i] === 0)
        .map((i) => Math.abs(c.position[i * 3]));
    expect(starts(PART.nerveEye).length).toBe(6);
    expect(starts(PART.nerveEye).every((x) => x < 0.058)).toBe(true);
    expect(starts(PART.nerveEar).every((x) => x >= 0.058)).toBe(true);
    // Every inked point arrives within the 3.6 s story, and the five organs are drawn with the ward's budget.
    expect(Math.max(...c.chain)).toBeCloseTo(3.6);
    expect(c.part.filter((p) => p === PART.ears).length).toBe(150);
    expect(c.part.filter((p) => p === PART.heart).length).toBe(380);
  });

  it("keeps the twenty patients and the standing body inside their point budgets", () => {
    // The ward is the same on every screen; a phone thins only the standing body's context, to three quarters.
    expect(buildCloud(a, false).count * 20).toBe(149520);
    expect(buildCloud(a, true).count).toBe(215402);
    expect(buildCloud(a, true, 0.75).count, "a phone keeps every organ point").toBe(180341);
  });
});

describe("ink chain", () => {
  it("runs from the entry organ through the brain and hands to the heart", () => {
    expect(chainAt("EYE", PART.eyes, 0)).toBe(0);
    expect(chainAt("EYE", PART.nerveEye, 0.5)).toBe(1.5);
    expect(chainAt("EYE", PART.brain, 1)).toBe(3);
    expect(chainAt("EYE", PART.hands, 0)).toBe(4);
    expect(chainAt("EYE", PART.heart, 0)).toBe(6);
    expect(chainAt("EYE", PART.artery, 1)).toBe(7);
    expect(chainAt("EYE", PART.ears, 0)).toBe(-1);
    expect(chainAt("EAR", PART.ears, 1)).toBe(1);
    expect(chainAt("EAR", PART.nerveEye, 0)).toBe(-1);
  });

  it("starts in the brain for memory and in the hands for skills", () => {
    expect(chainAt("MEM", PART.brain, 1)).toBe(3);
    expect(chainAt("MEM", PART.eyes, 0)).toBe(-1);
    expect(chainAt("SKL", PART.hands, 0)).toBe(0);
    // The hand nerve runs backwards: its hand end (u = 1) inks first.
    expect(chainAt("SKL", PART.nerveHands, 1)).toBe(1);
    expect(chainAt("SKL", PART.nerveHands, 0)).toBe(2);
    expect(chainAt("SKL", PART.heart, 1)).toBeCloseTo(6.6);
  });

  it("never inks pencil parts", () => {
    for (const part of [PART.skin, PART.bones, PART.muscle, PART.organs]) expect(chainAt("EAR", part, 0.5)).toBe(-1);
  });
});

describe("camera", () => {
  it("eases from 0 to 1 symmetrically", () => {
    expect(ease(0)).toBe(0);
    expect(ease(1)).toBe(1);
    expect(ease(0.5)).toBeCloseTo(0.5);
    expect(ease(0.25) + ease(0.75)).toBeCloseTo(1);
  });

  it("flies between views and lands on the target", () => {
    const f = new Flight({ x: 0, y: 0, zoom: 1 });
    f.go({ x: 100, y: 50, zoom: 4 }, 1000, 0);
    expect(f.done(500)).toBe(false);
    const mid = f.at(500);
    expect(mid.x).toBeCloseTo(50);
    expect(mid.zoom).toBeCloseTo(2);
    expect(f.at(2000)).toEqual({ x: 100, y: 50, zoom: 4 });
    f.jump({ x: 1, y: 2, zoom: 3 });
    expect(f.done(0)).toBe(true);
    expect(f.at(0)).toEqual({ x: 1, y: 2, zoom: 3 });
  });

  it("flies a turn and a focus with the camera", () => {
    const f = new Flight({ x: 0, y: 0, zoom: 1, yaw: 0, amp: 0.28, focus: 0 });
    f.go({ x: 0, y: 0, zoom: 1, yaw: 1, amp: 0.08, focus: 1 }, 1000, 0);
    expect(f.at(500).yaw).toBeCloseTo(0.5);
    expect(f.at(500).focus).toBeCloseTo(0.5);
    expect(f.at(1000)).toEqual({ x: 0, y: 0, zoom: 1, yaw: 1, amp: 0.08, focus: 1 });
  });

  it("frames a box by its tighter side", () => {
    expect(frame(10, 20, 100, 50, 1000, 1000, 0)).toEqual({ x: 10, y: 20, zoom: 10 });
    expect(frame(0, 0, 100, 400, 1000, 1000, 0).zoom).toBe(2.5);
  });
});

describe("ledger", () => {
  const o = (body: string, breached: boolean, held: string | null, cls = "resisted") => ({
    body,
    kind: "attack",
    outcome: { class: cls, breached, held, fooled: breached || held !== null },
  });

  it("counts breached and held defended bodies, and marks void hours", () => {
    const hour = {
      n: 7,
      exam: false,
      strain: "EAR-1.0",
      outcomes: [
        o("m1-none", true, null),
        o("m1-venin1", true, null),
        o("m2-venin2", false, "S09"),
        o("m3-textbook", false, null),
      ],
    } as unknown as HourRecord;
    expect(tick(hour)).toEqual({ n: 7, exam: false, void: false, breached: 1, held: 1, defended: 3, hit: true });
    expect(tick({ ...hour, strain: null } as HourRecord).void).toBe(true);
  });

  it("maps a pointer to a tick and a play clock to an index", () => {
    expect(indexAt(-5, 480, 48)).toBe(0);
    expect(indexAt(479, 480, 48)).toBe(47);
    expect(indexAt(240, 480, 48)).toBe(24);
    expect(indexAt(10, 0, 48)).toBe(0);
    expect(playIndex(0, 48)).toBe(0);
    expect(playIndex(19.9, 48)).toBe(47);
    expect(playIndex(100, 48)).toBe(47);
    expect(playIndex(0.5, 2)).toBe(0);
    expect(slots(1)).toBe(24);
    expect(slots(48)).toBe(48);
  });
});

describe("organ labels", () => {
  it("take the colour the figure inks each organ: every organ the attack reached, hands that held in violet", () => {
    const s = (pill: "breached" | "held" | "resisted", reach: number, route: "EAR" | "MEM" = "EAR") =>
      ({ body: "m1-venin1", pill, gene: null, reach, route }) as const;
    const tones = (st: ReturnType<typeof s>) => ORGANS.map((o) => organTone({ ...st }, o));
    // ORGANS: eyes, ears, brain, hands, heart.
    expect(tones(s("breached", 7))).toEqual(["", "ink", "ink", "ink", "ink"]);
    expect(tones(s("held", 5))).toEqual(["", "ink", "ink", "held", ""]);
    expect(tones(s("resisted", 1)), "read the strain and stopped there").toEqual(["", "ink", "", "", ""]);
    expect(tones(s("resisted", 0)), "never read it").toEqual(["", "", "", "", ""]);
    expect(tones(s("breached", 7, "MEM")), "a memory strain enters at the brain").toEqual([
      "",
      "",
      "ink",
      "ink",
      "ink",
    ]);
  });

  it("keeps labels apart in one fixed order, the ear over the eye, without moving free ones", () => {
    const at = (ears: number, eyes: number) =>
      ({ brain: [0, 100], ears: [20, ears], eyes: [0, eyes], heart: [0, 300], hands: [10, 500] }) as Parameters<
        typeof spread
      >[0];
    expect(spread(at(141, 140), 40)).toEqual({ brain: 100, ears: 141, eyes: 181, heart: 300, hands: 500 });
    // The eye rolls a pixel above the ear: the labels keep their places instead of swapping.
    expect(spread(at(139, 140), 40)).toEqual({ brain: 100, ears: 140, eyes: 180, heart: 300, hands: 500 });
    expect(spread(at(400, 420), 40), "a label pushed down pushes the ones below").toEqual({
      brain: 100,
      ears: 400,
      eyes: 440,
      heart: 480,
      hands: 520,
    });
  });

  it("start right of the right-most anchor in view, and stay inside the edge", () => {
    const at = { eyes: [500, 100], ears: [520, 110], brain: [500, 80], hands: [640, 500], heart: [505, 300] } as const;
    const where = (a: Record<string, readonly [number, number]>) => a as Parameters<typeof labelX>[0];
    expect(labelX(where(at), 1000, 800, 90, 120)).toBe(730);
    // A close-up of the head: the hand is below the canvas and no longer sets the column.
    expect(labelX(where({ ...at, hands: [640, 1300], heart: [505, 900] }), 1000, 800, 90, 120)).toBe(610);
    expect(labelX(where(at), 700, 800, 90, 120)).toBe(580);
  });
});
