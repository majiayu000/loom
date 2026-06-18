export type SkillMDensity = "compact" | "regular" | "comfy";

export interface SkillMPreferences {
  dark: boolean;
  density: SkillMDensity;
  accent: string[];
}

const STORAGE_KEY = "loom.skillm.preferences";
const DEFAULT_ACCENT = ["#ff0080", "#7928ca", "#00d9ff"];
const DEFAULT_PREFS: SkillMPreferences = { dark: true, density: "regular", accent: DEFAULT_ACCENT };

export function loadSkillMPreferences(): SkillMPreferences {
  if (typeof window === "undefined") return DEFAULT_PREFS;
  try {
    const parsed = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "null") as Partial<SkillMPreferences> | null;
    return {
      dark: typeof parsed?.dark === "boolean" ? parsed.dark : DEFAULT_PREFS.dark,
      density: isDensity(parsed?.density) ? parsed.density : DEFAULT_PREFS.density,
      accent: isAccent(parsed?.accent) ? parsed.accent : DEFAULT_PREFS.accent,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function saveSkillMPreferences(preferences: SkillMPreferences): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // Appearance preferences are best-effort; blocked storage must not break the panel.
  }
}

function isDensity(value: unknown): value is SkillMDensity {
  return value === "compact" || value === "regular" || value === "comfy";
}

function isAccent(value: unknown): value is string[] {
  return Array.isArray(value) && value.length === 3 && value.every((color) => typeof color === "string" && color.length > 0);
}
