import { afterEach, describe, expect, it, vi } from "vitest";
import { loadSkillMPreferences, saveSkillMPreferences } from "./skillm_prefs";

const STORAGE_KEY = "loom.skillm.preferences";

afterEach(() => {
  vi.restoreAllMocks();
  window.localStorage.clear();
});

describe("SkillM preferences", () => {
  it("round-trips local appearance preferences", () => {
    saveSkillMPreferences({ dark: false, density: "compact", accent: ["#111111", "#222222", "#333333"] });

    expect(loadSkillMPreferences()).toEqual({
      dark: false,
      density: "compact",
      accent: ["#111111", "#222222", "#333333"],
    });
  });

  it("falls back to defaults when stored preferences are corrupt", () => {
    window.localStorage.setItem(STORAGE_KEY, "{not-json");

    expect(loadSkillMPreferences()).toEqual({
      dark: true,
      density: "regular",
      accent: ["#ff0080", "#7928ca", "#00d9ff"],
    });
  });

  it("does not throw when localStorage rejects writes", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });

    expect(() => saveSkillMPreferences({ dark: false, density: "compact", accent: ["#111111", "#222222", "#333333"] })).not.toThrow();
  });
});
