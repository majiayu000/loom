import { AgentMarquee } from "../components/landing/AgentMarquee";
import { CliShowcase } from "../components/landing/CliShowcase";
import { Cta, LandingFooter, PullQuote } from "../components/landing/Closing";
import { Compare } from "../components/landing/Compare";
import { Features } from "../components/landing/Features";
import { LandingHero } from "../components/landing/Hero";
import { HowItWorks } from "../components/landing/HowItWorks";
import { LandingNav } from "../components/landing/Nav";

export function LandingPage() {
  return (
    <>
      <LandingNav />
      <LandingHero />
      <AgentMarquee />
      <Features />
      <HowItWorks />
      <CliShowcase />
      <Compare />
      <PullQuote />
      <Cta />
      <LandingFooter />
    </>
  );
}
