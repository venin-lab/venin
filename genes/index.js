// Venin Genes: the lab's tool-side checks and read filters, built from crates/core. Importing this module compiles the
// WebAssembly once.
import { readFileSync } from "node:fs";
import {
  genes_check,
  genes_clean,
  genes_clean_all,
  genes_entries,
  genes_guard,
  genes_instructions,
  genes_leaks,
  genes_rules,
  genes_tools,
  genes_visible_all,
  initSync,
} from "./venin_core.js";

initSync({ module: readFileSync(new URL("./venin_core_bg.wasm", import.meta.url)) });

export { scan } from "./venin_core.js";

/** The default policy, as policy.json holds it. */
export const defaultPolicy = JSON.parse(readFileSync(new URL("./policy.json", import.meta.url), "utf8"));

function read(out) {
  const value = JSON.parse(out);
  if (value.error) throw new Error(value.error);
  return value;
}

/** A guard for one session. Throws on a policy it cannot read. */
export function createGuard(policy) {
  const text = JSON.stringify(policy);
  const { genes, rules, acting } = read(genes_guard(text));
  let actions = 0;
  return {
    genes,
    rules,
    acting,
    check(tool, args = {}) {
      const blocked = (reason) => ({ allow: false, actions: 0, gene: null, reason });
      if (typeof tool !== "string") return blocked("the tool name is not a string");
      let out;
      try {
        out = JSON.parse(genes_check(text, actions, tool, JSON.stringify(args)));
      } catch (e) {
        return blocked(`the arguments cannot be read: ${e.message}`);
      }
      const decision = out.error ? blocked(out.error) : out;
      if (decision.allow) actions += decision.actions;
      return decision;
    },
    clean(content) {
      return read(genes_clean(text, content)).text;
    },
    cleanAll(contents) {
      return read(genes_clean_all(text, JSON.stringify(contents))).texts;
    },
    entries(list) {
      return read(genes_entries(text, JSON.stringify(list)));
    },
    visibleAll(contents) {
      return read(genes_visible_all(JSON.stringify(contents))).texts;
    },
    leaks(message) {
      return read(genes_leaks(text, JSON.stringify(message))).gene;
    },
    tools(list) {
      return read(genes_tools(text, JSON.stringify(list)));
    },
    instructions(content) {
      return read(genes_instructions(text, content));
    },
  };
}

/** The instinct rules of the default policy, to paste into a system prompt. */
export const rules = read(genes_rules(JSON.stringify(defaultPolicy.genes))).rules;
