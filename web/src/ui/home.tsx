import type { Lab } from "../record/load";
import { Checkup, Defend, Guard, Headline, HourFeed, HowItWorks, Release, Strains, Verify } from "./sections";
import { WardStage } from "./ward";

/** The ward first, then how an attack runs, so the steps are read before the scoreboard they explain. */
export function Home({ lab, hour, setHour }: { lab: Lab; hour: number; setHour: (n: number) => void }) {
  return (
    <main class="home">
      <WardStage lab={lab} hour={hour} setHour={setHour} />
      <HowItWorks />
      <Headline lab={lab} />
      <HourFeed lab={lab} />
      <Strains lab={lab} />
      <Guard lab={lab} />
      <Defend />
      <Checkup />
      <Verify lab={lab} />
      <Release />
    </main>
  );
}
