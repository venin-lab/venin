// Mirrors docs/RECORD.md field for field. Values that can pass 2^53 - 1 arrive as decimal strings.

export type Hex = string;
export type Column = "m1" | "m2" | "m3" | "m4";
export type Row = "none" | "textbook" | "priors" | "venin1" | "venin2";
export type Route = "EYE" | "EAR" | "MEM" | "SKL";
export type RunKind = "attack" | "benign" | "decoy" | "holder" | "defend";
export type OutcomeClass = "infected" | "resisted" | "blocked_upstream" | "malformed" | "void";
export type VoidReason =
  | "timeout"
  | "transport"
  | "provider_mismatch"
  | "safeguard"
  | "runner_down"
  | "paused"
  | "budget"
  | "clock_skew"
  | "rpc_unavailable"
  | "rpc_mismatch"
  | "host_busy";
export type Flag = "sealed" | "advisory" | "paused" | "budget";
export type HolderStatus = "run" | "reused" | "burn_not_found" | "gate" | "chain";

export interface Sampling {
  temperature_milli: number;
  top_p_milli: number;
  top_k: number;
  max_tokens: number;
}

export interface Model {
  column: Column;
  id: string;
  label: string;
  served_by: string;
  weights_sha: Hex;
  runtime: string;
  sampling: Sampling;
}

export interface BodyRef {
  id: string;
  column: Column;
  row: Row;
}

export interface Manifest {
  rules_version: number;
  league: string;
  genesis: number;
  period_s: number;
  deadline_s: number;
  late_s: number;
  exam_delay_s: number;
  k: number;
  library_sha: Hex;
  tasks_sha: Hex;
  genes_sha: Hex;
  rows_sha: Hex;
  names_sha: Hex;
  code: { tree: Hex; commit: Hex };
  models: Model[];
  bodies: BodyRef[];
  anchor: { address: string; chain_id: number };
  draw_chain_id: number;
  release_chain_id: number;
}

export interface Draw {
  block: number;
  hash: Hex;
  ts: number;
  prev_ts: number;
  rpc_agree: number;
}

export interface Exam {
  draw: Draw;
  families: string[];
}

export interface RunRef {
  body: string;
  kind: RunKind;
  sha: Hex;
}

export interface Withheld {
  body: string;
  kind: RunKind;
  commit: Hex;
}

export interface Outcome {
  class: OutcomeClass;
  reason: VoidReason | null;
  exposed: boolean;
  reached_brain: boolean;
  fooled: boolean;
  held: string | null;
  breached: boolean;
  flagged: boolean;
  done: boolean | null;
}

export interface OutcomeRef {
  body: string;
  kind: RunKind;
  outcome: Outcome;
}

export type Event =
  | {
      type: "kept" | "replaced";
      column: Column;
      incumbent: string;
      challenger: string;
      margin_ppm: number;
      pairs: number;
    }
  | { type: "remutated"; body: string; op: "add" | "drop" | "swap" | "import"; gene: string }
  | { type: "mutant"; parent: string; child: string; operator: string }
  | { type: "extinct"; strain: string }
  | { type: "arrived"; strain: string }
  | { type: "concern"; strain: string; name: string };

export interface Holder {
  received_ms: number;
  tx: Hex;
  signer: string;
  signature: Hex;
  route: Route;
  payload_sha: Hex;
  status: HolderStatus;
  /** Null when the payload failed the gate. */
  payload: string | null;
}

/** A holder's agent sent to the ward: their rules on the model of one column. */
export interface Defender {
  received_ms: number;
  tx: Hex;
  signer: string;
  signature: Hex;
  column: Column;
  rules_sha: Hex;
  status: HolderStatus;
  /** Null when the rules failed the gate. */
  rules: string | null;
}

/** An agent in the ward: from period `from` for 24 periods, on every training hour's strain. */
export interface Defending {
  /** The first 16 hex digits of its burn; its body is `<column>-defender-<id>`. */
  id: string;
  column: Column;
  rules: string;
  from: number;
}

export interface WindowEntry {
  n: number;
  breached: boolean;
  benign: boolean;
  decoy: boolean;
  prompt_tokens: number;
}

export interface ColumnRoles {
  incumbent: string;
  challenger: string;
  since: number;
}

export interface PoolEntry {
  id: string;
  draws: number;
  misses: number;
  toll: number;
}

export interface Strain {
  id: string;
  route: Route;
  carrier: string;
  family: number;
  technique: string;
  goal: string;
  subject: string | null;
  tool_name: string | null;
  template: string;
  split: string;
  source: string;
  source_ref: string;
  operators?: string[];
  origin?: "mutant" | "holder";
  parent?: string;
}

export interface State {
  genomes: Record<string, string[]>;
  windows: Record<string, WindowEntry[]>;
  columns: Record<string, ColumnRoles>;
  pool: PoolEntry[];
  mutants: Strain[];
  exam_left: string[];
  concern_names: number;
  concerns: Record<string, string>;
  holder_cursor: string;
  burns: Hex[];
  payloads: Hex[];
  /** Absent before the first defend entry. */
  defender_cursor?: string;
  defending?: Defending[];
}

export interface HourRecord {
  n: number;
  start: number;
  prev: Hex;
  rules_version: number;
  draw: Draw | null;
  exam: boolean;
  strain: string | null;
  runs: RunRef[];
  withheld: Withheld[];
  outcomes: OutcomeRef[];
  events: Event[];
  holders: Holder[];
  /** Absent in hours without defend entries. */
  defenders?: Defender[];
  spend_micro: number;
  void_share_ppm: number;
  flags: Flag[];
  state: State;
}

export interface Targets {
  wallet: string;
  domain: string;
  nonce: string;
  canary: string;
}

export interface Attempt {
  step: number;
  attempt: number;
  status: number;
  latency_ms: number;
  body: string;
}

export interface Call {
  step: number;
  id: string;
  name: string;
  arguments: string;
  result: string | null;
  stopped_by: string | null;
  executed: boolean;
  filtered_by: string[];
}

export interface RunLine {
  body: string;
  kind: RunKind;
  task: string;
  strain: string | null;
  salt: Hex;
  targets: Targets;
  genome: string[];
  model: string;
  responses: Attempt[];
  calls: Call[];
  external: { request: string; response: string }[];
  usage: { prompt_tokens: number; completion_tokens: number };
  latency_ms: number;
  cost_micro: number;
  outcome: Outcome;
}

export interface Anchor {
  kind: "manifest" | "exam" | "hour";
  n: number;
  tx: Hex;
  block: number;
}

export interface Head {
  n: number;
  sha: Hex;
  written_s: number;
}

/** An entry of takedowns.json: a file removed for a legal reason, its hash kept. */
export interface Takedown {
  path: string;
  sha: Hex;
  date_s: number;
  reason: string;
}

export interface Status {
  n: number;
  anchor_lag_s: number;
  runway_days: number;
  void_share_ppm: number;
  models: { column: Column; valid: number; void: number; malformed: number }[];
  updated_s: number;
}

export interface Gene {
  id: string;
  kind: "instinct" | "skin";
  name: string;
  text: string | null;
  rule: string | null;
  params: Record<string, unknown>;
  source: string;
  url: string;
}

export interface Task {
  id: string;
  kind: "attack" | "benign" | "decoy";
  world: "ward" | "priors";
  prompt: string;
  setup: { balance_cents?: number; [key: string]: unknown };
}

export type Story = "advisory" | "void" | "concern" | "outbreak" | "exam" | "gene" | "extinction" | "move" | "quiet";

/** cards/<n>.json, written by the runner after each period. */
export interface Card {
  n: number;
  story: Story;
  caption: string;
  note: string | null;
  focal: { n: number; body: string } | null;
}
