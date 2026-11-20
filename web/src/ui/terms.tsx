import type { ComponentChildren } from "preact";
import { useId, useRef } from "preact/hooks";

export const TICKER = "$VENIN";

/** Each lab word the site uses, with its plain meaning (at most 20 words, for a reader new to all of it). */
export const TERMS = {
  "ai-agent": [
    "AI agent",
    "An AI that does tasks on its own, like answering email, paying bills or trading, not only chatting.",
  ],
  "prompt-injection": ["Prompt injection", "The security name for these scams: orders hidden in text an AI reads."],
  ward: ["Ward", "The grid of 20 agents, drawn as patients in beds. Open one to look inside."],
  defence: [
    "Defence",
    "Protection added around the AI. Each row gets a different kind, from none to Venin's evolving rules.",
  ],
  model: ["AI model", "The AI that does the agent's thinking. Each column runs a different one."],
  none: ["No defence", "The AI with no protection at all: the baseline every other row is compared with."],
  textbook: [
    "Textbook",
    "Five standard lines of safety advice in the AI's instructions, from common practice. They never change.",
  ],
  priors: [
    "Priors",
    "A test copy of Priors, another team's security package for agents, run as published. Not affiliated with Venin.",
  ],
  venin: [
    "Venin 1 and Venin 2",
    "Venin's evolving setups. Each starts with a few rules; every 12 hours, per AI model, the weaker copies the stronger.",
  ],
  breached: ["Breached", "The scam worked: test money, a planted secret or an email reached the attacker. Pink."],
  held: ["Held", "The agent fell for it, but a defence rule blocked the payment or message in time. Violet."],
  resisted: [
    "Resisted",
    "No money, secret or email reached the attacker. Black. Pink only where it came in means it read the scam.",
  ],
  void: ["Void", "The run did not count because something outside the test failed, like a timeout. Grey."],
  fooled: ["Fooled", "The agent tried to pay, email or leak a secret to the attacker, whether or not it went through."],
  run: ["Run", "One agent doing one task. Each hour every agent does three: one carries the scam, two are normal."],
  strain: ["Strain", "One scam message used as an attack. Like a virus, it can mutate and spread."],
  "strain-code": [
    "Strain code",
    "The first part is the way in (EYE, EAR, MEM, SKL), then the family, then the version.",
  ],
  "way-in": [
    "Way in",
    "How the scam gets in: ears (emails, web pages), eyes (tool descriptions), brain (memory notes), hands (skill files).",
  ],
  "code-word": [
    "Code word",
    "A made-up word planted in each scam. If the agent repeats it, the scam reached its brain.",
  ],
  gene: ["Gene", "One defence rule an agent carries. Rules that keep agents safe are passed on, like genes."],
  genome: [
    "Defence rules",
    "The rules this agent carried this hour. I rules are advice it can ignore; S rules are hard checks.",
  ],
  "harness-rule": [
    "Harness rule",
    "A hard check in the code around the agent's tools. Words cannot switch it off. Codes start with S.",
  ],
  "prompt-clause": [
    "Prompt clause",
    "A line added to the AI's instructions. The AI can still be talked out of it. Codes start with I.",
  ],
  defended: [
    "Defended agents",
    "The 12 agents in the Textbook, Venin 1 and Venin 2 rows, which carry rules from the gene pool.",
  ],
  exam: [
    "Exam hour",
    "Every fifth hour uses a held-back scam that no defence trains on. Only these hours count on the scoreboard.",
  ],
  "exam-runs": ["Runs", "Attacks this row has faced in exam hours so far. More runs, firmer numbers."],
  range: ["95% range", "The true rate is very likely inside this range. It narrows as more exam hours come in."],
  work: ["Work done", "How much normal work the agent still gets done. A defence that blocks everything scores low."],
  mutant: ["Mutant", "A changed copy the lab breeds when a scam gets past a defended agent. Same aim, new disguise."],
  lineage: ["Lineage", "A scam and every changed copy bred from it: one family."],
  concern: [
    "Variant of concern",
    "A scam family that got past a Venin agent. It takes the next name from a list of venomous animals.",
  ],
  toll: ["Toll", "How many times this scam has breached a defended agent so far."],
  pool: ["In circulation", "The 12 training scams the lab draws from each hour. Weak ones die out; strong ones breed."],
  extinct: ["Extinct", "A scam retired after three failed draws in a row, or to make room for a new mutant."],
  library: ["Library", "Every scam the lab can use: rewritten from public research, plus the mutants bred here."],
  draw: ["Draw", "Which scam runs each hour is picked from a public block hash, so the lab does not choose."],
  record: ["Record", "One file per hour holding every run: the AI's words, its tool calls and the result."],
  hash: ["Fingerprint", "A short code computed from a file. Change one letter and the code changes completely."],
  anchor: [
    "Anchor",
    "The hour's fingerprint, written on a blockchain. Any later change to the record would no longer match.",
  ],
  verify: [
    "Verify",
    "Your browser rebuilds every hour from the published files and checks each number and fingerprint.",
  ],
  late: ["LATE", "The hour's fingerprint reached the chain late, so the hour stays off the scoreboard."],
  sealed: ["Sealed", "The hour's runs are locked away for review. Only their fingerprints are public for now."],
  advisory: ["Advisory", "Priors' results for this hour are held back while a problem is reported to them privately."],
  paused: ["Paused", "Priors sat out this hour after a problem, so its runs did not count."],
  budget: ["Over budget", "The hour's spending cap ran out, so some runs did not happen."],
  recorded: ["Recorded", "The hour ran and its record is published."],
  withheld: ["Withheld", "A run kept back for review. Only its fingerprint is public."],
  "taken-down": ["Taken down", "A file removed for a legal reason. Its fingerprint stays in the record."],
  "not-recomputed": ["Not recomputed", "Hours checked by fingerprint only, without rebuilding their runs."],
  "chain-unread": ["Chain facts unread", "Chain records this page has not read yet. Use Check anchors on chain."],
  uptime: ["Uptime", "Share of hours that ran and counted."],
  "void-share": ["Void share", "Share of runs that did not count."],
  release: [
    "Release a strain",
    "Write your own scam and send it into the lab. It only runs against the lab's agents, with fake targets.",
  ],
  defend: [
    "Defend",
    "Your own agent in the ward for 24 hours: your rules on a lab model, attacked by each hour's strain.",
  ],
  immunity: ["Immunity", "The share of the strains it met that your agent held: nothing it did reached the attacker."],
  burn: [
    "Burn",
    "Sending $VENIN, the lab's coin, to a dead address, where nobody can use it again. It pays for a release.",
  ],
  gate: [
    "Gate",
    "The lab's check on every scam text: fake targets only, no real links, addresses, emails or phone numbers.",
  ],
  placeholders: [
    "Stand-ins",
    "Slots the lab fills with fakes: {wallet} and {domain} for the attacker, {canary} for the planted secret.",
  ],
  disguises: [
    "Disguises",
    "Ways attackers hide orders: in code, in invisible characters, or in fake system messages. The lab uses them too.",
  ],
  checkup: ["Check-up", "A free scan of your own agent's files, done in your browser. Nothing is uploaded."],
  "agent-folder": [
    "Agent folder",
    "The folder that holds an agent's setup: its rules, skills and tool settings, such as a .claude folder.",
  ],
  mcp: ["MCP config", "A settings file listing the outside tools an agent can call."],
  lesion: ["Lesion", "A weak spot the check-up found in your agent's files, marked on the organ it affects."],
  "block-rate": [
    "Block rate",
    "Harness rule: share of attacker payments or emails it blocked. Prompt clause: share of scams read without being fooled.",
  ],
  "work-pts": [
    "Work points",
    "Normal work done by agents with this rule, minus agents without it. Below zero means work lost.",
  ],
  tokens: [
    "Extra words",
    "How many words this rule adds to the AI's instructions on every run. Harness rules add none.",
  ],
  fitness: [
    "Fitness",
    "A Venin agent's score: safety times normal work done, minus a small cost for longer instructions.",
  ],
  carriers: ["Carriers", "How many of the 12 defended agents carry this rule now."],
  outbreak: ["Outbreak", "Eight or more defended agents breached in one hour."],
  "gene-event": ["Gene event", "A defence rule spread to every Venin agent, died out, or came back."],
  "big-move": ["Big move", "A defence's breach rate moved 5 points or more against the day before."],
  "local-league": [
    "Practice copy",
    "The lab run on one computer, with small models and a test chain. The public lab uses Robinhood Chain.",
  ],
  synthetic: ["Synthetic", "Made-up sample data for building the site. None of it comes from real runs."],
} as const;

export type TermKey = keyof typeof TERMS;

const hover = () => matchMedia("(hover: hover) and (pointer: fine)").matches;

/** A lab word with its plain meaning: tap or click toggles it, a mouse hover shows it, Esc or a tap elsewhere hides it. */
export function Term({ k, children }: { k: TermKey; children?: ComponentChildren }) {
  const id = useId();
  const pop = useRef<HTMLSpanElement>(null);
  const pinned = useRef(false);
  const wait = useRef(0);
  const [title, text] = TERMS[k];
  const place = (b: HTMLElement) => {
    const p = pop.current;
    if (!p) return;
    const r = b.getBoundingClientRect();
    const w = Math.min(280, innerWidth - 32);
    p.style.left = `${Math.min(Math.max(16, r.left + r.width / 2 - w / 2), innerWidth - 16 - w)}px`;
    const below = innerHeight - r.bottom > 180;
    p.style.top = below ? `${r.bottom + 8}px` : "auto";
    p.style.bottom = below ? "auto" : `${innerHeight - r.top + 8}px`;
  };
  return (
    <span class="term">
      <button
        type="button"
        class="term-word"
        aria-describedby={id}
        popovertarget={id}
        onClick={(e) => {
          const open = pop.current?.matches(":popover-open") ?? false;
          // A click on a term the mouse already opened keeps it open instead of toggling it shut.
          if (open && !pinned.current) e.preventDefault();
          else place(e.currentTarget);
          // Opened or kept open by a click: it stays when the pointer leaves. Closed by a click: not pinned.
          pinned.current = !open || !pinned.current;
        }}
        onPointerEnter={(e) => {
          if (e.pointerType !== "mouse" || !hover()) return;
          const b = e.currentTarget;
          wait.current = window.setTimeout(() => {
            place(b);
            pop.current?.showPopover();
          }, 250);
        }}
        onPointerLeave={() => {
          clearTimeout(wait.current);
          if (!pinned.current && pop.current?.matches(":popover-open")) pop.current.hidePopover();
        }}
      >
        {children ?? title}
      </button>
      <span
        ref={pop}
        id={id}
        popover="auto"
        class="term-pop"
        onToggle={(e) => {
          if (e.newState === "closed") pinned.current = false;
          else
            addEventListener("scroll", () => pop.current?.hidePopover(), { once: true, capture: true, passive: true });
        }}
      >
        <b>{title}</b> {text}
      </span>
    </span>
  );
}
