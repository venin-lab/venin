import { type Env, queue, Refused, release, reply } from "./release";

/** Only the handler is exported here: the runtime treats every export of the entry module as an entrypoint. */
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const path = new URL(request.url).pathname;
    try {
      if (request.method === "OPTIONS" && path === "/api/release") return reply(env, 204, null);
      if (request.method === "GET" && path === "/api/queue") return await queue(request, env);
      const ip = request.headers.get("cf-connecting-ip") ?? "local";
      if (!(await env.LIMITER.limit({ key: ip })).success) throw new Refused(429, "too many requests; wait a minute");
      if (request.method === "POST" && path === "/api/release") return await release(request, env, Date.now());
      throw new Refused(404, "no such endpoint");
    } catch (e) {
      if (e instanceof Refused) return reply(env, e.status, { error: e.message });
      return reply(env, 500, { error: "the release could not be checked; try again" });
    }
  },
};
