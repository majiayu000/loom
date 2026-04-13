import { pick } from "../i18n";
import type { Locale } from "../i18n";
import type { PageId } from "../types";

export const NAV_ITEMS: Array<{ id: PageId; icon: string }> = [
  { id: "overview", icon: "dashboard" },
  { id: "skills", icon: "bolt" },
  { id: "bindings", icon: "link" },
  { id: "targets", icon: "adjust" },
  { id: "projections", icon: "grid_view" },
  { id: "ops", icon: "terminal" },
  { id: "settings", icon: "settings" },
];

export function getPageFromHash(): PageId {
  const raw = window.location.hash.replace("#", "");
  const item = NAV_ITEMS.find((entry) => entry.id === raw);
  return item?.id ?? "overview";
}

export function navLabel(locale: Locale, pageId: PageId) {
  switch (pageId) {
    case "overview":
      return pick(locale, "Overview", "总览");
    case "skills":
      return pick(locale, "Skills", "技能");
    case "bindings":
      return pick(locale, "Bindings", "绑定");
    case "targets":
      return pick(locale, "Targets", "目标");
    case "projections":
      return pick(locale, "Projections", "投影");
    case "ops":
      return pick(locale, "Ops", "操作");
    case "settings":
      return pick(locale, "Environment", "环境");
  }
}

export function topbarSearchPlaceholder(page: PageId, locale: Locale) {
  if (page === "skills") {
    return pick(locale, "Search skills…", "搜索技能…");
  }
  return pick(locale, "Search pending ops…", "搜索待处理操作…");
}
