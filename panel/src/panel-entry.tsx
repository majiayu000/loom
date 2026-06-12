import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { SkillMPanel } from "./pages/SkillMPanel";
import "./styles/panel/skillm.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root mount point missing");

createRoot(root).render(
  <StrictMode>
    <SkillMPanel />
  </StrictMode>,
);
