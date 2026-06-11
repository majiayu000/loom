import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PanelApp } from "./pages/PanelApp";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/panel/shell.css";
import "./styles/panel/primitives.css";
import "./styles/panel/data.css";
import "./styles/panel/tweaks.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root mount point missing");

createRoot(root).render(
  <StrictMode>
    <PanelApp />
  </StrictMode>,
);
