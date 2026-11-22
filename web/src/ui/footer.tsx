import { Mark } from "./mark";
import { REPO, TokenLine } from "./sections";

const DATASETS = "LLMail-Inject, AgentDojo, InjecAgent, Agent Security Bench, deepset prompt-injections and garak";

export function Footer() {
  return (
    <footer class="foot">
      <div class="foot-brand">
        <span class="foot-mark">
          <Mark size={220} />
        </span>
        <div>
          <img class="foot-word" src="/brand/venin-wordmark-light.svg" alt="venin" width="320" height="87" />
          <p>An open immune lab for AI agents. Every hour on the record, every number recomputable.</p>
          <nav class="foot-nav" aria-label="Pages">
            <a href="/#what" aria-haspopup="dialog">
              What is this?
            </a>
            <a href="/#ward">Ward</a>
            <a href="/strains">Strains</a>
            <a href="/genes">Genes</a>
            <a href="/record">Record</a>
            <a href="/verify">Verify</a>
            <a href="/checkup">Check-up</a>
            <a href="/release">Release</a>
            <a href="/models">Models</a>
            <a href="/defend">Defend</a>
          </nav>
        </div>
      </div>
      <div class="credits">
        <TokenLine />
        <p>
          Code: <a href={REPO}>github.com/venin-lab/venin</a>, MIT licence: the runner, the verifier and this site.
        </p>
        <p>
          Anatomy: BodyParts3D, (c) The Database Center for Life Science, licensed under CC Attribution 4.0
          International; point clouds sampled and modified by Venin.
        </p>
        <p>Strains and tasks adapted from {DATASETS}; licences in THIRD_PARTY.md.</p>
        <p>
          Sandbox copy of Priors' public package v0.20.0 (tool list, fence, host allowlist and caps; network and chain
          stubbed). Unaffiliated.
        </p>
        <p>Type: Funnel Display, Funnel Sans and Spline Sans Mono, SIL Open Font License 1.1.</p>
        <p>Venin is not affiliated with any model provider. Nothing here is financial advice.</p>
      </div>
    </footer>
  );
}
