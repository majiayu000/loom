export type Locale = "en" | "zh-CN";

const LOCALE_STORAGE_KEY = "loom-panel.locale";

export function detectLocale(): Locale {
  if (typeof window === "undefined") return "en";

  const saved = window.localStorage.getItem(LOCALE_STORAGE_KEY);
  if (saved === "en" || saved === "zh-CN") {
    return saved;
  }

  return "en";
}

export function persistLocale(locale: Locale) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(LOCALE_STORAGE_KEY, locale);
  window.document.documentElement.lang = locale;
}

export function pick(locale: Locale, en: string, zh: string) {
  return locale === "zh-CN" ? zh : en;
}

export function missingLabel(locale: Locale) {
  return pick(locale, "n/a", "无");
}

export function formatTime(locale: Locale, value?: string | null) {
  if (!value) return missingLabel(locale);
  try {
    return new Date(value).toLocaleString(locale === "zh-CN" ? "zh-CN" : "en-US");
  } catch {
    return value;
  }
}

export function countLabel(
  locale: Locale,
  count: number,
  singular: string,
  plural: string,
  zhUnit: string,
) {
  return locale === "zh-CN"
    ? `${count} ${zhUnit}`
    : `${count} ${count === 1 ? singular : plural}`;
}

export function syncStateLabel(locale: Locale, value?: string | null) {
  switch ((value ?? "").toUpperCase()) {
    case "SYNCED":
      return pick(locale, "synced", "已同步");
    case "ACTIVE":
      return pick(locale, "active", "活跃");
    case "DIVERGED":
      return pick(locale, "diverged", "已分叉");
    case "CONFLICTED":
      return pick(locale, "conflicted", "冲突");
    case "PENDING_PUSH":
      return pick(locale, "pending push", "待推送");
    case "LOCAL_ONLY":
      return pick(locale, "local only", "仅本地");
    default:
      return value || pick(locale, "unknown", "未知");
  }
}

export function healthLabel(locale: Locale, value?: string | null) {
  switch ((value ?? "").toLowerCase()) {
    case "healthy":
      return pick(locale, "healthy", "健康");
    case "warning":
      return pick(locale, "warning", "警告");
    case "drifted":
      return pick(locale, "drifted", "已漂移");
    case "idle":
      return pick(locale, "idle", "空闲");
    default:
      return value || pick(locale, "unknown", "未知");
  }
}

export function methodLabel(locale: Locale, value?: string | null) {
  switch ((value ?? "").toLowerCase()) {
    case "symlink":
      return pick(locale, "symlink", "符号链接");
    case "copy":
      return pick(locale, "copy", "复制");
    case "materialize":
      return pick(locale, "materialize", "实体化");
    default:
      return value || pick(locale, "unknown", "未知");
  }
}

export function ownershipLabel(locale: Locale, value?: string | null) {
  switch ((value ?? "").toLowerCase()) {
    case "managed":
      return pick(locale, "managed", "托管");
    case "observed":
      return pick(locale, "observed", "观察");
    case "external":
      return pick(locale, "external", "外部");
    default:
      return value || pick(locale, "unknown", "未知");
  }
}

export function matcherKindLabel(locale: Locale, value?: string | null) {
  switch ((value ?? "").toLowerCase()) {
    case "path_prefix":
      return pick(locale, "path prefix", "路径前缀");
    case "exact_path":
      return pick(locale, "exact path", "精确路径");
    case "name":
      return pick(locale, "name match", "名称匹配");
    default:
      return value || pick(locale, "unknown", "未知");
  }
}

export function capabilityLabel(locale: Locale, value: "symlink" | "copy" | "watch") {
  if (value === "symlink") return pick(locale, "symlink", "符号链接");
  if (value === "copy") return pick(locale, "copy", "复制");
  return pick(locale, "watch", "观察");
}

export function activeLabel(locale: Locale, active: boolean) {
  return active ? pick(locale, "active", "启用") : pick(locale, "inactive", "停用");
}

export function onOffLabel(locale: Locale, active: boolean) {
  return active ? pick(locale, "on", "开") : pick(locale, "off", "关");
}
