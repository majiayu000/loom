import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { TeamApp } from "./team/TeamApp";
import "./team/team.css";
import "./team/studio.css";

const root = document.getElementById("root");
if (!root) throw new Error("Team app mount point missing");
createRoot(root).render(<StrictMode><TeamApp /></StrictMode>);
