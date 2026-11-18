import { type Anatomy, buildCloud } from "./anatomy";

/** Builds the standing body's cloud off the main thread and hands its arrays back without copying them. */
addEventListener("message", (e: MessageEvent<{ anatomy: Anatomy; density: number }>) => {
  const c = buildCloud(e.data.anatomy, true, e.data.density);
  postMessage(c, { transfer: [c.position, c.part, c.u, c.order, c.chain, c.nrm].map((a) => a.buffer as ArrayBuffer) });
});
