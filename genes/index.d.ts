/** A tool that moves money: the arguments that may hold its one recipient (default `["to"]`) and its amount (default
 * `amount`). */
export interface PayTool {
  to?: string[];
  amount?: string;
}

/** A tool that sends a message: the arguments holding its recipients (default `["to"]`). */
export interface SendTool {
  to?: string[];
}

export interface Policy {
  /** Gene ids from the lab's genes.json: instinct genes (I..) make `rules`, skin genes (S..) the checks. */
  genes: string[];
  /** The owner's e-mail address: their domain for S08, their own address for S09. S08 and S09 need it. */
  owner?: string;
  /** Payees the owner keeps, for S01 and S09. */
  address_book?: string[];
  /** The owner's message for the task, for S09. */
  plan?: string;
  /** The most one payment may move, for S02, as a decimal with at most two places; the gene's own cap when absent. */
  cap?: string | number;
  /** E-mail domains S08 accepts besides the owner's, matched exactly; `$owner.domain` is the owner's. */
  domains?: string[];
  /** Values that must never leave, for S07, which needs at least one. S07 finds them plain, in ROT13, hex, base64 or
   * percent-encoding, in any string, object key or number of the arguments. */
  secrets?: string[];
  /** Tools that move money, by name (matched trimmed and in any case). */
  pay?: Record<string, PayTool>;
  /** Tools that send a message, by name (matched trimmed and in any case). */
  send?: Record<string, SendTool>;
}

export interface Decision {
  allow: boolean;
  /** The actions the call takes, one per recipient, for S10: 0 for a tool that neither pays nor sends, and for a
   * blocked call. */
  actions: number;
  /** The gene that blocked the call; null when the guard would not read an argument. */
  gene: string | null;
  reason: string | null;
}

export interface ToolNote {
  tool: string;
  /** The gene that withheld the description, or the check-up finding. */
  why: string;
}

export interface Guard {
  /** The policy's genes, in id order. */
  genes: string[];
  /** The policy's instinct genes, as the lab adds them to a system prompt. */
  rules: string;
  /** The names of the pay and send tools, trimmed and in lower case. */
  acting: string[];
  /** Decides one tool call. Calls to tools the policy does not list as pay or send are allowed; a tool name that is
   * not a string, or arguments the guard cannot serialise, are blocked rather than thrown on. */
  check(tool: string, args?: Record<string, unknown>): Decision;
  /** Text the agent reads, after S03, S04 and S05 as the policy holds them. */
  clean(text: string): string;
  /** `clean` on each text of a list, in one call. */
  cleanAll(texts: string[]): string[];
  /** Each text with the characters that render as nothing dropped (tags, zero-width, bidi), whatever the policy. */
  visibleAll(texts: string[]): string[];
  /** The gene (S07) that stops a message for carrying a watched secret anywhere in it, or null. */
  leaks(message: unknown): string | null;
  /** A third-party list of prompts, resources or resource templates, treated as `tools` treats tools. */
  entries(list: object[]): { entries: object[]; notes: { entry: string; why: string }[] };
  /** A third-party tool list after S06, or after S03 and S04 and the check-up's findings. */
  tools(list: object[]): { tools: object[]; notes: ToolNote[] };
  /** A third-party server's instructions after S06, or after S03 and S04 and the check-up's findings; `why` is null
   * when they were kept. */
  instructions(text: string): { text: string; why: string | null };
}

/** The default policy (policy.json). */
export const defaultPolicy: Policy;

/** The default policy's instinct rules, to paste into a system prompt. */
export const rules: string;

/** A guard for one session. Throws when the policy has a key, gene, tool or value it cannot read. */
export function createGuard(policy: Policy): Guard;

/** The check-up, as in @veninlab/venin: JSON text of `{files: [{path, content}]}` in, a report out. */
export function scan(files: string): string;
