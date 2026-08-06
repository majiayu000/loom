import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { SkillMPanel } from "./pages/SkillMPanel";
import "./styles/panel/primitives.css";
import "./styles/panel/data.css";
import "./styles/panel/skillm.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root mount point missing");

createRoot(root).render(
  <StrictMode>
    <SkillMPanel />
  </StrictMode>,
);
