import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { LandingPage } from "./pages/LandingPage";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/landing/layout.css";
import "./styles/landing/nav.css";
import "./styles/landing/hero.css";
import "./styles/landing/marquee.css";
import "./styles/landing/features.css";
import "./styles/landing/how.css";
import "./styles/landing/cli.css";
import "./styles/landing/compare.css";
import "./styles/landing/closing.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root mount point missing");

createRoot(root).render(
  <StrictMode>
    <LandingPage />
  </StrictMode>,
);
