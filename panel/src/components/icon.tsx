import { useEffect, useState } from "react";

const ICON_FALLBACKS: Record<string, string> = {
  account_tree: "⌬",
  add: "+",
  adjust: "◉",
  api: "⌁",
  blur_on: "✦",
  bolt: "⚡",
  check_circle: "✓",
  chevron_right: "›",
  close: "×",
  cloud_done: "☁",
  content_copy: "⧉",
  dashboard: "▦",
  data_object: "{}",
  dataset: "⬚",
  description: "≡",
  dns: "◎",
  error: "!",
  filter_list: "☰",
  folder: "▤",
  folder_managed: "▦",
  folder_zip: "▧",
  grid_view: "▥",
  help: "?",
  hub: "◌",
  link: "⛓",
  memory: "◍",
  notifications: "◉",
  person: "◔",
  rebase_edit: "⎇",
  remove: "−",
  report: "‼",
  schedule: "◷",
  science: "⚗",
  search: "⌕",
  settings: "⚙",
  shield: "⛨",
  speed: "⏱",
  swap_horiz: "↔",
  sync: "↻",
  sync_problem: "⚠",
  terminal: "⌨",
  timer: "◷",
  warning: "⚠",
};

function hasMaterialSymbolFont() {
  if (typeof document === "undefined" || !("fonts" in document)) return false;
  return document.fonts.check('16px "Material Symbols Outlined"');
}

export function Icon({ name }: { name: string }) {
  const [fontReady, setFontReady] = useState<boolean>(() => hasMaterialSymbolFont());

  useEffect(() => {
    if (fontReady) return;
    if (typeof document === "undefined" || !("fonts" in document)) return;

    let cancelled = false;
    const refresh = () => {
      if (!cancelled) setFontReady(hasMaterialSymbolFont());
    };

    refresh();
    void document.fonts.ready.then(refresh).catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [fontReady]);

  if (!fontReady) {
    return <span aria-hidden="true" className="material-symbols-fallback">{ICON_FALLBACKS[name] ?? "•"}</span>;
  }
  return <span className="material-symbols-outlined">{name}</span>;
}
