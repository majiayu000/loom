import { useEffect, useMemo, useState, useTransition } from "react";
import {
  detectLocale,
  persistLocale,
  pick,
} from "./i18n";
import "./styles.css";
import type { Locale } from "./i18n";
import type { PageId, PanelData } from "./types";
import { Icon } from "./components/icon";
import { buildSkillViews, EMPTY_PANEL_DATA, loadPanelData } from "./lib/panel_data";
import { getPageFromHash, NAV_ITEMS, navLabel, topbarSearchPlaceholder } from "./lib/panel_navigation";
import { OverviewPage } from "./pages/overview_page";
import { SkillsPage } from "./pages/skills_page";
import { BindingsPage } from "./pages/bindings_page";
import { TargetsPage } from "./pages/targets_page";
import { ProjectionsPage } from "./pages/projections_page";
import { OpsPage } from "./pages/ops_page";
import { SettingsPage } from "./pages/settings_page";

function usePanelApp(locale: Locale) {
  const [data, setData] = useState<PanelData>(EMPTY_PANEL_DATA);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [page, setPage] = useState<PageId>(() => getPageFromHash());
  const [navPending, startNavTransition] = useTransition();

  useEffect(() => {
    let cancelled = false;

    async function run() {
      setLoading(true);
      setLoadError(null);
      try {
        const next = await loadPanelData(locale);
        if (!cancelled) {
          setData(next);
        }
      } catch (error) {
        if (!cancelled) {
          const detail = error instanceof Error ? error.message : pick(locale, "panel failed to load", "面板加载失败");
          setLoadError(`${pick(locale, "Panel failed to load", "面板加载失败")}: ${detail}`);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    void run();
    return () => {
      cancelled = true;
    };
  }, [locale]);

  useEffect(() => {
    const onHash = () => {
      startNavTransition(() => setPage(getPageFromHash()));
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  return {
    data,
    loading,
    loadError,
    page,
    navPending,
    navigate(next: PageId) {
      window.location.hash = next;
      startNavTransition(() => setPage(next));
    },
    async refresh() {
      setLoading(true);
      setLoadError(null);
      try {
        const next = await loadPanelData(locale);
        setData(next);
      } catch (error) {
        const detail = error instanceof Error ? error.message : pick(locale, "panel failed to load", "面板加载失败");
        setLoadError(`${pick(locale, "Panel failed to load", "面板加载失败")}: ${detail}`);
      } finally {
        setLoading(false);
      }
    },
  };
}

function showSearchForPage(page: PageId) {
  return page === "overview" || page === "skills" || page === "ops";
}

function renderPage(args: {
  page: PageId;
  data: PanelData;
  locale: Locale;
  query: string;
}) {
  const { page, data, locale, query } = args;
  const skillViews = buildSkillViews(data);

  if (page === "skills") {
    return <SkillsPage data={data} locale={locale} query={query} skillViews={skillViews} />;
  }
  if (page === "bindings") {
    return <BindingsPage data={data} locale={locale} skillViews={skillViews} />;
  }
  if (page === "targets") {
    return <TargetsPage data={data} locale={locale} />;
  }
  if (page === "projections") {
    return <ProjectionsPage data={data} locale={locale} skillViews={skillViews} />;
  }
  if (page === "ops") {
    return <OpsPage data={data} locale={locale} query={query} />;
  }
  if (page === "settings") {
    return <SettingsPage data={data} locale={locale} />;
  }
  return <OverviewPage data={data} locale={locale} query={query} skillViews={skillViews} />;
}

export function App() {
  const [locale, setLocale] = useState<Locale>(() => detectLocale());
  const [topbarQuery, setTopbarQuery] = useState("");
  const { data, loading, loadError, navPending, page, navigate, refresh } = usePanelApp(locale);
  const content = useMemo(() => renderPage({ page, data, locale, query: topbarQuery }), [page, data, locale, topbarQuery]);
  const showTopbarSearch = showSearchForPage(page);

  useEffect(() => {
    persistLocale(locale);
    window.document.title = pick(locale, "Loom Panel", "Loom 控制台");
  }, [locale]);

  useEffect(() => {
    if (!showTopbarSearch && topbarQuery) {
      setTopbarQuery("");
    }
  }, [showTopbarSearch, topbarQuery]);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <div className="brand-icon">
              <Icon name="blur_on" />
            </div>
            <div>
              <h1 className="brand-title">LOOM</h1>
              <p className="brand-meta">{pick(locale, "workspace control plane", "工作区控制平面")}</p>
            </div>
          </div>
        </div>

        <ul className="nav-list">
          {NAV_ITEMS.map((item) => (
            <li key={item.id}>
              <button
                className={`nav-item ${page === item.id ? "active" : ""}`}
                onClick={() => navigate(item.id)}
                type="button"
              >
                <Icon name={item.icon} />
                <span className="nav-label">{navLabel(locale, item.id)}</span>
              </button>
            </li>
          ))}
        </ul>

        <div className="sidebar-footer">
          <button className="nav-item" onClick={() => navigate("settings")} type="button">
            <Icon name="description" />
            <span className="nav-label">{pick(locale, "Environment", "环境")}</span>
          </button>
          <button className="nav-item" onClick={refresh} type="button">
            <Icon name="sync" />
            <span className="nav-label">{pick(locale, "Refresh", "刷新")}</span>
          </button>
        </div>
      </aside>

      <div className="main-shell">
        <header className="topbar">
          <div className="topbar-left">
            <span className="topbar-wordmark">LOOM</span>
            <nav className="topbar-links">
              <button className="topbar-link active" onClick={() => navigate(page)} type="button">
                {navLabel(locale, page)}
              </button>
            </nav>
          </div>

          <div className="topbar-right">
            {showTopbarSearch ? (
              <label className="topbar-search-shell">
                <Icon name="search" />
                <input
                  className="toolbar-search"
                  placeholder={topbarSearchPlaceholder(page, locale)}
                  value={topbarQuery}
                  onChange={(event) => setTopbarQuery(event.target.value)}
                />
              </label>
            ) : null}
            <button
              className="icon-button"
              title={pick(locale, "Refresh", "刷新")}
              onClick={() => void refresh()}
              type="button"
            >
              <Icon name="sync" />
            </button>
            <button
              className="icon-button"
              title={pick(locale, "Environment", "环境")}
              onClick={() => navigate("settings")}
              type="button"
            >
              <Icon name="settings" />
            </button>
            <button
              className="topbar-avatar"
              onClick={() => setLocale((current) => (current === "zh-CN" ? "en" : "zh-CN"))}
              title={pick(locale, "Toggle language", "切换语言")}
              type="button"
            >
              <span className="topbar-avatar-core" />
              <span className="topbar-avatar-badge">{locale === "zh-CN" ? "中" : "EN"}</span>
            </button>
          </div>
        </header>

        <div className="page-scroll">
          {(loadError || navPending || loading) ? (
            <div className="page-banner-stack">
              {loadError ? (
                <span className="status-pill is-danger">
                  <Icon name="error" />
                  {loadError}
                </span>
              ) : null}
              {navPending ? (
                <span className="status-pill is-primary">
                  <Icon name="swap_horiz" />
                  {pick(locale, "switching page", "切换页面中")}
                </span>
              ) : null}
              {loading ? (
                <span className="status-pill is-primary">
                  <Icon name="schedule" />
                  {pick(locale, "loading latest state", "正在加载最新状态")}
                </span>
              ) : null}
            </div>
          ) : null}
          {content}
        </div>

        <footer className="shell-statusbar">
          <div className={`shell-status-item ${data.live ? "is-success" : ""}`}>
            <Icon name={data.live ? "check_circle" : "science"} />
            <span>{data.live ? pick(locale, "API connected", "API 已连接") : pick(locale, "API unavailable", "API 不可用")}</span>
          </div>
          <div className="shell-status-item">
            <Icon name="sync" />
            <span>{pick(locale, `Pending ops: ${data.pending.count}`, `待处理操作: ${data.pending.count}`)}</span>
          </div>
          <div className="shell-status-item">
            <Icon name="grid_view" />
            <span>{pick(locale, `Projections: ${data.v3.projections.length}`, `投影: ${data.v3.projections.length}`)}</span>
          </div>
          <div className="shell-status-item">
            <Icon name="schedule" />
            <span>{pick(locale, `Updated: ${data.lastUpdated ? new Date(data.lastUpdated).toLocaleTimeString() : "n/a"}`, `更新时间: ${data.lastUpdated ? new Date(data.lastUpdated).toLocaleTimeString() : "无"}`)}</span>
          </div>
        </footer>
      </div>
    </div>
  );
}
