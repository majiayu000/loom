use std::fs;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{
    AgentKind, BindingAddArgs, CaptureArgs, Cli, Command, MigrateCommand, MigrateV2ToV3Args,
    ProjectArgs, ProjectionMethod, TargetAddArgs, TargetCommand, TargetOwnership,
    WorkspaceBindingCommand, WorkspaceCommand, WorkspaceMatcherKind,
};
use crate::commands::{list_skills, remote_status_payload};
use crate::state::{AppContext, resolve_agent_skill_dirs};
use crate::v3::V3StatePaths;

#[derive(Clone)]
struct PanelState {
    ctx: Arc<AppContext>,
    dist_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct TargetAddRequest {
    agent: AgentKind,
    path: String,
    #[serde(default)]
    ownership: Option<TargetOwnership>,
}

#[derive(Debug, Deserialize)]
struct BindingAddRequest {
    agent: AgentKind,
    profile: String,
    matcher_kind: WorkspaceMatcherKind,
    matcher_value: String,
    target: String,
    #[serde(default)]
    policy_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectRequest {
    skill: String,
    binding: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    method: Option<ProjectionMethod>,
}

#[derive(Debug, Deserialize)]
struct CaptureRequest {
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    binding: Option<String>,
    #[serde(default)]
    instance: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

pub async fn run_panel(ctx: AppContext, port: u16) -> Result<()> {
    let dist_dir = ctx.root.join("panel/dist");
    ensure_panel_dist(&dist_dir)?;

    let state = PanelState {
        ctx: Arc::new(ctx),
        dist_dir,
    };

    let app = Router::new()
        .route("/", get(frontend_index))
        .route("/api/health", get(health))
        .route("/api/info", get(info))
        .route("/api/skills", get(skills))
        .route("/api/targets", get(targets))
        .route("/api/v3/status", get(v3_status))
        .route("/api/v3/bindings", get(v3_bindings))
        .route("/api/v3/bindings/{binding_id}", get(v3_binding_show))
        .route("/api/v3/targets", get(v3_targets))
        .route("/api/v3/targets/{target_id}", get(v3_target_show))
        .route("/api/v3/migration/plan", get(v3_migration_plan))
        .route("/api/v3/migration/apply", post(v3_migration_apply))
        .route("/api/v3/targets", post(v3_target_add))
        .route("/api/v3/targets/{target_id}/remove", post(v3_target_remove))
        .route("/api/v3/bindings", post(v3_binding_add))
        .route(
            "/api/v3/bindings/{binding_id}/remove",
            post(v3_binding_remove),
        )
        .route("/api/v3/project", post(v3_project))
        .route("/api/v3/capture", post(v3_capture))
        .route("/api/remote/status", get(remote_status))
        .route("/api/pending", get(pending))
        .route("/{*path}", get(frontend_static_asset))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("panel listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// The compiled Vite bundle is the authoritative panel surface. Keep the legacy
// inline fallback deck around for future recovery-mode work without shipping a warning.
#[allow(dead_code)]
async fn index() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html lang='en'>
  <head>
    <meta charset='UTF-8' />
    <meta name='viewport' content='width=device-width, initial-scale=1.0' />
    <title>Loom Control Deck</title>
    <link rel='icon' type='image/svg+xml' href='/favicon.svg' />
    <link rel='preconnect' href='https://fonts.googleapis.com' />
    <link rel='preconnect' href='https://fonts.gstatic.com' crossorigin />
    <link href='https://fonts.googleapis.com/css2?family=Baloo+2:wght@600;700&family=IBM+Plex+Sans:wght@400;500;600&family=JetBrains+Mono:wght@500&display=swap' rel='stylesheet' />
    <style>
      :root {
        --bg-a: #fff8ea;
        --bg-b: #ffe8ce;
        --bg-c: #ffd2bf;
        --ink: #2f2642;
        --muted: #6d607e;
        --card: #fffdf8cc;
        --line: #ead9bf;
        --accent: #ff6ea6;
        --accent-2: #58cdb7;
        --danger: #ff8f8f;
      }

      * { box-sizing: border-box; }

      body {
        margin: 0;
        min-height: 100vh;
        color: var(--ink);
        font-family: 'IBM Plex Sans', sans-serif;
        background:
          radial-gradient(circle at 6% -12%, #ffe5f5 0%, transparent 34%),
          radial-gradient(circle at 96% 12%, #d5f5ff 0%, transparent 36%),
          linear-gradient(145deg, var(--bg-a), var(--bg-b) 58%, var(--bg-c));
      }

      .shell {
        width: min(1240px, 95vw);
        margin: 22px auto 36px;
        display: grid;
        grid-template-columns: 260px 1fr;
        gap: 14px;
      }

      .pane {
        border: 1px solid var(--line);
        border-radius: 22px;
        background: var(--card);
        backdrop-filter: blur(10px);
        box-shadow: 0 16px 38px #7f684f1f;
      }

      .sidebar {
        padding: 16px;
        position: sticky;
        top: 18px;
        height: fit-content;
      }

      .brand {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 4px 2px 12px;
      }

      .brand img {
        width: 46px;
        height: 46px;
        border-radius: 14px;
        box-shadow: 0 8px 20px #ff8db53f;
      }

      .brand h1 {
        margin: 0;
        font-family: 'Baloo 2', cursive;
        font-size: 34px;
        line-height: 1;
      }

      .brand p {
        margin: 2px 0 0;
        color: var(--muted);
        font-size: 12px;
      }

      .nav {
        margin-top: 10px;
        display: grid;
        gap: 8px;
      }

      .nav-btn {
        border: 1px solid var(--line);
        border-radius: 12px;
        background: #fff9ef;
        text-align: left;
        padding: 10px 12px;
        font-size: 14px;
        font-weight: 600;
        color: var(--ink);
        cursor: pointer;
      }

      .nav-btn.active {
        background: linear-gradient(180deg, #ffedf4, #ffe8f2);
        border-color: #eeb8cc;
        box-shadow: inset 0 0 0 1px #ffd3e1;
      }

      .side-note {
        margin-top: 12px;
        border: 1px dashed #dfc8a8;
        border-radius: 12px;
        padding: 10px;
        background: #fffaf2;
        font-size: 12px;
        color: #645977;
      }

      .main {
        padding: 16px;
      }

      .topbar {
        display: flex;
        flex-wrap: wrap;
        justify-content: space-between;
        align-items: center;
        gap: 10px;
      }

      .chips {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
      }

      .chip {
        border: 1px solid var(--line);
        border-radius: 999px;
        padding: 6px 10px;
        font-size: 12px;
        background: #fff9ef;
      }

      .chip.sync { font-weight: 700; }
      .chip.synced { background: #e9fff8; border-color: #b6ecd9; }
      .chip.pending_push { background: #fff4e8; border-color: #ffd9ac; }
      .chip.diverged,
      .chip.conflicted { background: #ffeef0; border-color: #ffc3cb; }
      .chip.local_only { background: #f5f0ff; border-color: #d9cbff; }

      .btn {
        border: 1px solid #d9bbc8;
        border-radius: 11px;
        background: linear-gradient(180deg, #ffedf5, #ffe7f1);
        color: #5b2b44;
        padding: 8px 12px;
        font-weight: 700;
        cursor: pointer;
      }

      .view-title {
        margin: 16px 0 8px;
        font-family: 'Baloo 2', cursive;
        font-size: 30px;
        line-height: 1;
      }

      .view-sub {
        color: var(--muted);
        margin: 0 0 14px;
      }

      .grid {
        display: grid;
        gap: 12px;
      }

      .grid.overview {
        grid-template-columns: repeat(4, minmax(0, 1fr));
      }

      .card {
        border: 1px solid var(--line);
        border-radius: 14px;
        background: #fffaf2;
        padding: 12px;
      }

      .metric-k { font-size: 12px; color: var(--muted); }
      .metric-v {
        margin-top: 8px;
        font-size: 26px;
        font-family: 'Baloo 2', cursive;
        line-height: 1;
      }

      .mono { font-family: 'JetBrains Mono', monospace; }

      .commands { display: grid; gap: 8px; margin-top: 12px; }

      .command-item {
        display: grid;
        grid-template-columns: 1fr auto;
        gap: 8px;
        border: 1px solid var(--line);
        border-radius: 10px;
        background: #fff;
        padding: 8px;
      }

      .command-item code {
        white-space: nowrap;
        overflow: auto;
      }

      .cmd-btn {
        border: 1px solid #d8c6aa;
        border-radius: 8px;
        background: #fff;
        padding: 5px 8px;
        font-size: 12px;
        cursor: pointer;
      }

      .cmd-btn.danger {
        border-color: #efb0bc;
        background: #fff3f6;
        color: #8d3554;
      }

      .skills-head {
        display: flex;
        flex-wrap: wrap;
        justify-content: space-between;
        align-items: center;
        gap: 8px;
      }

      .skills-search {
        width: min(320px, 100%);
        border: 1px solid var(--line);
        border-radius: 10px;
        background: #fff;
        padding: 8px 10px;
      }

      .skills-layout {
        margin-top: 10px;
        display: grid;
        grid-template-columns: 1.3fr 1fr;
        gap: 10px;
      }

      .skills-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
        gap: 10px;
      }

      .skill-card {
        border: 1px solid var(--line);
        border-radius: 12px;
        background: #fff;
        padding: 10px;
        cursor: pointer;
      }

      .skill-card.active {
        border-color: #efbdd0;
        box-shadow: inset 0 0 0 1px #ffd8e5;
        background: #fff5fa;
      }

      .skill-name {
        margin: 0 0 8px;
        font-size: 16px;
        font-weight: 700;
      }

      .chip-wrap {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
      }

      .tiny-chip {
        border: 1px solid var(--line);
        border-radius: 999px;
        padding: 2px 8px;
        font-size: 11px;
      }

      .tiny-chip.linked {
        background: #ebfff7;
        border-color: #b8eadb;
      }

      .tiny-chip.unlinked {
        background: #fff4e9;
        border-color: #ffd8b0;
      }

      .tiny-chip.drift {
        background: #ffeef0;
        border-color: #ffc3cb;
      }

      .skill-detail {
        min-height: 280px;
      }

      .action-card {
        border: 1px solid var(--line);
        border-radius: 12px;
        background: #fff;
        padding: 10px;
      }

      .action-card + .action-card {
        margin-top: 10px;
      }

      .form-grid {
        display: grid;
        gap: 10px;
      }

      .field {
        display: grid;
        gap: 6px;
      }

      .field.two {
        grid-template-columns: 1fr 1fr;
      }

      .field label {
        font-size: 12px;
        color: var(--muted);
        font-weight: 700;
      }

      .field input,
      .field select {
        width: 100%;
        border: 1px solid var(--line);
        border-radius: 10px;
        background: #fffdf8;
        padding: 9px 10px;
        font: inherit;
        color: var(--ink);
      }

      .action-row {
        display: flex;
        gap: 8px;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
      }

      .action-note {
        font-size: 12px;
        color: var(--muted);
      }

      .result-card {
        margin: 12px 0;
        border: 1px solid var(--line);
        border-radius: 14px;
        background: #fffdf8;
        padding: 12px;
      }

      .result-card.error {
        background: #fff4f6;
        border-color: #ffc3cb;
      }

      .result-body {
        margin-top: 8px;
        border-radius: 10px;
        background: #fff;
        border: 1px solid var(--line);
        padding: 10px;
        max-height: 220px;
        overflow: auto;
        white-space: pre-wrap;
        word-break: break-word;
      }

      .skill-stat-grid {
        display: grid;
        grid-template-columns: repeat(4, minmax(0, 1fr));
        gap: 8px;
        margin: 12px 0;
      }

      .stat {
        border: 1px solid var(--line);
        border-radius: 10px;
        background: #fff;
        padding: 8px;
      }

      .stat .metric-v {
        font-size: 22px;
      }

      .split-grid {
        margin-top: 12px;
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 12px;
      }

      .stack {
        display: grid;
        gap: 10px;
      }

      .section-label {
        font-size: 11px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        color: var(--muted);
        margin-bottom: 8px;
      }

      .note {
        margin: 0;
        color: var(--muted);
        font-size: 13px;
      }

      .entity-list {
        display: grid;
        gap: 8px;
      }

      .entity-row {
        border: 1px solid var(--line);
        border-radius: 11px;
        background: #fff;
        padding: 10px;
      }

      .entity-row.drift {
        background: #fff4f6;
        border-color: #ffc3cb;
      }

      .entity-title {
        font-weight: 700;
      }

      .entity-path {
        margin-top: 4px;
        color: #645977;
        font-size: 12px;
        word-break: break-word;
      }

      .timeline {
        display: grid;
        gap: 8px;
        max-height: 520px;
        overflow: auto;
      }

      .event {
        border: 1px solid var(--line);
        border-radius: 11px;
        background: #fff;
        padding: 8px 10px;
      }

      .event-head {
        display: flex;
        justify-content: space-between;
        gap: 8px;
        font-size: 12px;
      }

      .warning {
        margin-top: 10px;
        border: 1px solid #ffcf9f;
        background: #fff3e7;
        border-radius: 12px;
        padding: 10px;
      }

      .warning ul {
        margin: 8px 0 0;
        padding-left: 16px;
      }

      .toast {
        position: fixed;
        right: 16px;
        bottom: 16px;
        border-radius: 10px;
        padding: 10px 12px;
        border: 1px solid #bde7da;
        background: #effff8;
        box-shadow: 0 10px 28px #2a7f6840;
        display: none;
      }

      .status-danger {
        color: #9f2f52;
        font-weight: 700;
      }

      @media (max-width: 1100px) {
        .grid.overview { grid-template-columns: repeat(2, minmax(0, 1fr)); }
      }

      @media (max-width: 940px) {
        .shell { grid-template-columns: 1fr; }
        .sidebar {
          position: static;
          display: grid;
          grid-template-columns: 1fr;
        }
        .nav {
          grid-template-columns: repeat(4, minmax(0, 1fr));
        }
        .nav-btn { text-align: center; }
        .skills-layout { grid-template-columns: 1fr; }
        .split-grid { grid-template-columns: 1fr; }
        .field.two { grid-template-columns: 1fr; }
      }

      @media (max-width: 620px) {
        .nav { grid-template-columns: repeat(2, minmax(0, 1fr)); }
        .grid.overview { grid-template-columns: 1fr; }
        .command-item { grid-template-columns: 1fr; }
        .skill-stat-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
      }
    </style>
  </head>
  <body>
    <main class='shell'>
      <aside class='pane sidebar'>
        <div class='brand'>
          <img src='/favicon.svg' alt='Loom icon' />
          <div>
            <h1>Loom</h1>
            <p>Agent-first Skill Deck</p>
          </div>
        </div>
        <nav class='nav' id='nav'></nav>
        <div class='side-note'>
          Tip: All actions mirror CLI commands for reproducible agent workflows.
        </div>
      </aside>
      <section class='pane main'>
        <div class='topbar'>
          <div class='chips' id='top-chips'></div>
          <button class='btn' id='refresh-btn'>Refresh</button>
        </div>
        <div id='view'></div>
      </section>
    </main>
    <div class='toast mono' id='toast'></div>

<script>
      const ROUTES = ['overview', 'skills', 'ops', 'settings'];

      const state = {
        info: null,
        health: null,
        skills: [],
        legacyTargets: { skills: {} },
        v3: { ok: false, error: null, data: null },
        v3View: null,
        remote: null,
        remoteWarnings: [],
        pending: { count: 0, ops: [] },
        migration: { ok: false, data: null, error: null },
        query: '',
        route: 'overview',
        selectedSkill: null,
        loadedAt: null,
        lastAction: null,
      };

      function esc(value) {
        return String(value ?? '')
          .replace(/&/g, '&amp;')
          .replace(/</g, '&lt;')
          .replace(/>/g, '&gt;')
          .replace(/\"/g, '&quot;')
          .replace(/'/g, '&#39;');
      }

      function syncClass(value) {
        const v = String(value || 'unknown').toLowerCase();
        return v.replace(/\s+/g, '_');
      }

      function ts(iso) {
        if (!iso) return '-';
        try {
          return new Date(iso).toLocaleString();
        } catch {
          return String(iso);
        }
      }

      function showToast(text) {
        const node = document.getElementById('toast');
        node.textContent = text;
        node.style.display = 'block';
        clearTimeout(showToast._timer);
        showToast._timer = setTimeout(() => { node.style.display = 'none'; }, 1400);
      }

      function emptyV3View() {
        return {
          available: false,
          error: null,
          counts: {
            skills: 0,
            targets: 0,
            bindings: 0,
            active_bindings: 0,
            rules: 0,
            projections: 0,
            drifted_projections: 0,
            operations: 0,
          },
          bindings: [],
          targets: [],
          rules: [],
          projections: [],
          checkpoint: null,
          skillMap: {},
          drifted: [],
        };
      }

      function createSkillView(name) {
        return {
          name,
          rules: [],
          projections: [],
          bindingIds: [],
          targetIds: [],
          methods: [],
          driftedCount: 0,
        };
      }

      async function copyText(text) {
        try {
          await navigator.clipboard.writeText(text);
          showToast('Copied command');
        } catch {
          showToast('Copy failed');
        }
      }

      async function postJson(url, body) {
        const response = await fetch(url, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(body),
        });
        return response.json();
      }

      function rememberAction(env) {
        state.lastAction = {
          at: new Date().toISOString(),
          env,
        };
      }

      function renderActionResult() {
        const result = state.lastAction;
        if (!result?.env) return '';
        const env = result.env;
        const ok = !!env.ok;
        const warnings = env.meta?.warnings || [];
        const opId = env.meta?.op_id;
        const body = ok ? env.data : env.error;
        return `
          <aside class='result-card ${ok ? '' : 'error'}'>
            <div class='action-row'>
              <div>
                <div class='section-label'>Last Action</div>
                <div><strong>${esc(env.cmd || 'panel.action')}</strong> • ${esc(ts(result.at))}</div>
                <div class='action-note'>request ${esc(env.request_id || '-')} ${opId ? `• op ${esc(opId)}` : ''}</div>
              </div>
              <span class='tiny-chip ${ok ? 'linked' : 'drift'}'>${ok ? 'ok' : 'error'}</span>
            </div>
            ${warnings.length > 0 ? `<div class='action-note' style='margin-top:8px;'>warnings: ${warnings.map((item) => esc(item)).join(' • ')}</div>` : ''}
            <div class='result-body mono'>${esc(JSON.stringify(body, null, 2))}</div>
          </aside>
        `;
      }

      async function runAction(url, body, successText) {
        try {
          const env = await postJson(url, body);
          rememberAction(env);
          if (env.ok) {
            showToast(successText || 'Action complete');
            await load();
            return;
          }
          showToast(env?.error?.code || 'Action failed');
          render();
        } catch (error) {
          rememberAction({
            ok: false,
            cmd: 'panel.action',
            request_id: 'panel',
            error: {
              code: 'NETWORK_ERROR',
              message: String(error?.message || error),
            },
            meta: {},
          });
          showToast('Network error');
          render();
        }
      }

      function routeFromHash() {
        const raw = (window.location.hash || '#overview').replace('#', '').toLowerCase();
        return ROUTES.includes(raw) ? raw : 'overview';
      }

      function matcherText(matcher) {
        if (!matcher) return 'unscoped';
        return `${matcher.kind}: ${matcher.value}`;
      }

      function projectionIsDrifted(projection) {
        return !!(projection?.observed_drift || projection?.health !== 'healthy');
      }

      function bindingUsage(bindingId) {
        const v3 = state.v3View || emptyV3View();
        return {
          rules: (v3.rules || []).filter((rule) => rule.binding_id === bindingId),
          projections: (v3.projections || []).filter((projection) => projection.binding_id === bindingId),
        };
      }

      function targetUsage(targetId) {
        const v3 = state.v3View || emptyV3View();
        return {
          bindings: (v3.bindings || []).filter((binding) => binding.default_target_id === targetId),
          rules: (v3.rules || []).filter((rule) => rule.target_id === targetId),
          projections: (v3.projections || []).filter((projection) => projection.target_id === targetId),
        };
      }

      function buildV3View(skillNames, payload) {
        const view = emptyV3View();
        if (!payload?.ok || !payload.data) {
          view.error = payload?.error || null;
          return view;
        }

        const data = payload.data;
        const bindings = Array.isArray(data.bindings) ? data.bindings : [];
        const targets = Array.isArray(data.targets) ? data.targets : [];
        const rules = Array.isArray(data.rules) ? data.rules : [];
        const projections = Array.isArray(data.projections) ? data.projections : [];
        const bindingMap = Object.fromEntries(bindings.map((binding) => [binding.binding_id, binding]));
        const targetMap = Object.fromEntries(targets.map((target) => [target.target_id, target]));
        const skillMap = {};

        function ensureSkill(name) {
          if (!skillMap[name]) {
            skillMap[name] = createSkillView(name);
          }
          return skillMap[name];
        }

        (skillNames || []).forEach((name) => ensureSkill(name));

        rules.forEach((rule) => {
          const entry = ensureSkill(rule.skill_id);
          entry.rules.push({
            ...rule,
            binding: bindingMap[rule.binding_id] || null,
            target: targetMap[rule.target_id] || null,
          });
        });

        projections.forEach((projection) => {
          const entry = ensureSkill(projection.skill_id);
          entry.projections.push({
            ...projection,
            binding: bindingMap[projection.binding_id] || null,
            target: targetMap[projection.target_id] || null,
          });
        });

        const drifted = [];
        Object.values(skillMap).forEach((entry) => {
          const bindingIds = new Set();
          const targetIds = new Set();
          const methods = new Set();

          entry.rules.forEach((rule) => {
            if (rule.binding_id) bindingIds.add(rule.binding_id);
            if (rule.target_id) targetIds.add(rule.target_id);
            if (rule.method) methods.add(rule.method);
          });

          entry.projections.forEach((projection) => {
            if (projection.binding_id) bindingIds.add(projection.binding_id);
            if (projection.target_id) targetIds.add(projection.target_id);
            if (projection.method) methods.add(projection.method);
            if (projectionIsDrifted(projection)) {
              drifted.push({
                skill_id: entry.name,
                projection,
              });
            }
          });

          entry.bindingIds = Array.from(bindingIds);
          entry.targetIds = Array.from(targetIds);
          entry.methods = Array.from(methods);
          entry.driftedCount = entry.projections.filter((projection) => projectionIsDrifted(projection)).length;
        });

        view.available = true;
        view.counts = { ...view.counts, ...(data.counts || {}) };
        view.bindings = bindings;
        view.targets = targets;
        view.rules = rules;
        view.projections = projections;
        view.checkpoint = data.checkpoint || null;
        view.skillMap = skillMap;
        view.drifted = drifted;
        return view;
      }

      function renderV3Notice() {
        if (state.v3View?.available) return '';
        const error = state.v3?.error || state.v3View?.error;
        const message = error?.message || 'v3 state not initialized yet.';
        return `
          <aside class='warning'>
            <strong class='status-danger'>V3 State Unavailable</strong>
            <div style='margin-top:6px;'>${esc(message)}</div>
          </aside>
        `;
      }

      function skillInfo(name) {
        return state.v3View?.skillMap?.[name] || createSkillView(name);
      }

      function setRoute(route) {
        const next = ROUTES.includes(route) ? route : 'overview';
        if (window.location.hash !== '#' + next) {
          window.location.hash = next;
        } else {
          state.route = next;
          render();
        }
      }

      function commandList(root) {
        return [
          `loom --json --root "${root}" workspace status`,
          `loom --json --root "${root}" workspace binding list`,
          `loom --json --root "${root}" target list`,
          `loom --json --root "${root}" sync pull`,
          `loom --json --root "${root}" sync push`,
        ];
      }

      function renderTopChips() {
        const remote = state.remote || {};
        const v3 = state.v3View || emptyV3View();
        const html = [
          `<span class='chip'>Health: ${state.health?.ok ? 'OK' : 'UNKNOWN'}</span>`,
          `<span class='chip sync ${syncClass(remote.sync_state)}'>Sync: ${esc(remote.sync_state || 'UNKNOWN')}</span>`,
          `<span class='chip'>Skills: ${esc(state.skills.length)}</span>`,
          `<span class='chip'>Bindings: ${esc(v3.counts.bindings || 0)}</span>`,
          `<span class='chip'>Targets: ${esc(v3.counts.targets || 0)}</span>`,
          `<span class='chip'>Drift: ${esc(v3.counts.drifted_projections || 0)}</span>`,
          `<span class='chip'>Pending: ${esc(state.pending?.count || 0)}</span>`,
        ].join('');
        document.getElementById('top-chips').innerHTML = html;
      }

      function renderNav() {
        const labels = {
          overview: 'Overview',
          skills: 'Skills',
          ops: 'Ops',
          settings: 'Settings',
        };

        document.getElementById('nav').innerHTML = ROUTES
          .map((route) => `<button class='nav-btn ${state.route === route ? 'active' : ''}' data-route='${route}'>${labels[route]}</button>`)
          .join('');

        document.querySelectorAll('.nav-btn').forEach((btn) => {
          btn.addEventListener('click', () => setRoute(btn.getAttribute('data-route')));
        });
      }

      function renderOverview(root) {
        const remote = state.remote || {};
        const v3 = state.v3View || emptyV3View();
        const projectedSkills = Object.values(v3.skillMap || {}).filter((entry) => entry.projections.length > 0).length;
        return `
          <h2 class='view-title'>Overview</h2>
          <p class='view-sub'>Runtime summary and quick commands • ${esc(ts(state.loadedAt))}</p>
          ${renderV3Notice()}
          <div class='grid overview'>
            <div class='card'><div class='metric-k'>Skills</div><div class='metric-v'>${state.skills.length}</div></div>
            <div class='card'><div class='metric-k'>Projected Skills</div><div class='metric-v'>${projectedSkills}</div></div>
            <div class='card'><div class='metric-k'>Bindings</div><div class='metric-v'>${esc(v3.counts.bindings || 0)}</div></div>
            <div class='card'><div class='metric-k'>Targets</div><div class='metric-v'>${esc(v3.counts.targets || 0)}</div></div>
            <div class='card'><div class='metric-k'>Projections</div><div class='metric-v'>${esc(v3.counts.projections || 0)}</div></div>
            <div class='card'><div class='metric-k'>Drifted</div><div class='metric-v'>${esc(v3.counts.drifted_projections || 0)}</div></div>
            <div class='card'><div class='metric-k'>Pending Ops</div><div class='metric-v'>${esc(state.pending?.count || 0)}</div></div>
            <div class='card'><div class='metric-k'>Ahead / Behind</div><div class='metric-v mono'>${esc(remote.ahead ?? 0)} / ${esc(remote.behind ?? 0)}</div></div>
          </div>
          <div class='split-grid'>
            <div class='card'>
              <div class='section-label'>Workspace Bindings</div>
              <div class='entity-list'>
                ${v3.bindings.length > 0
                  ? v3.bindings.map((binding) => `
                      <article class='entity-row'>
                        <div class='entity-title'>${esc(binding.binding_id)}</div>
                        <div class='entity-path'>${esc(binding.agent)} • ${esc(binding.profile_id)} • ${esc(matcherText(binding.workspace_matcher))}</div>
                        <div class='chip-wrap' style='margin-top:8px;'>
                          <span class='tiny-chip linked'>${binding.active ? 'active' : 'inactive'}</span>
                          <span class='tiny-chip'>target ${esc(binding.default_target_id)}</span>
                          <span class='tiny-chip'>${esc(binding.policy_profile)}</span>
                        </div>
                      </article>
                    `).join('')
                  : `<div class='event'>No v3 bindings loaded.</div>`}
              </div>
            </div>
            <div class='card'>
              <div class='section-label'>Projection Targets</div>
              <div class='entity-list'>
                ${v3.targets.length > 0
                  ? v3.targets.map((target) => `
                      <article class='entity-row'>
                        <div class='entity-title'>${esc(target.target_id)}</div>
                        <div class='entity-path'>${esc(target.agent)} • ${esc(target.path)}</div>
                        <div class='chip-wrap' style='margin-top:8px;'>
                          <span class='tiny-chip'>${esc(target.ownership)}</span>
                          ${target.capabilities?.symlink ? `<span class='tiny-chip linked'>symlink</span>` : ''}
                          ${target.capabilities?.copy ? `<span class='tiny-chip linked'>copy</span>` : ''}
                          ${target.capabilities?.watch ? `<span class='tiny-chip linked'>watch</span>` : ''}
                        </div>
                      </article>
                    `).join('')
                  : `<div class='event'>No v3 targets loaded.</div>`}
              </div>
            </div>
          </div>
          <div class='card' style='margin-top:12px;'>
            <div class='section-label'>Remote</div>
            <div class='mono'>${esc(state.info?.remote_url || '-')}</div>
            ${v3.checkpoint ? `<div class='entity-path'>Last scanned op: ${esc(v3.checkpoint.last_scanned_op_id || '-')} • ${esc(ts(v3.checkpoint.updated_at))}</div>` : ''}
          </div>
          <div class='commands'>
            ${commandList(root).map((cmd) => `
              <div class='command-item mono'>
                <code>${esc(cmd)}</code>
                <button class='cmd-btn copy-btn' data-cmd='${esc(cmd)}'>Copy</button>
              </div>
            `).join('')}
          </div>
        `;
      }

      function renderProjectForm(skillName, info) {
        const v3 = state.v3View || emptyV3View();
        const bindings = v3.bindings || [];
        if (!v3.available) {
          return `
            <div class='action-card'>
              <div class='section-label'>Project</div>
              <p class='action-note'>Initialize v3 state before projecting this skill.</p>
            </div>
          `;
        }
        if (bindings.length === 0) {
          return `
            <div class='action-card'>
              <div class='section-label'>Project</div>
              <p class='action-note'>Add a target and a workspace binding first.</p>
            </div>
          `;
        }
        const defaultBinding = info.bindingIds[0] || bindings[0]?.binding_id || '';
        const bindingOptions = bindings.map((binding) => {
          const label = `${binding.binding_id} • ${binding.agent} • ${binding.profile_id}`;
          return `<option value='${esc(binding.binding_id)}' ${binding.binding_id === defaultBinding ? 'selected' : ''}>${esc(label)}</option>`;
        }).join('');
        const targetOptions = (v3.targets || [])
          .filter((target) => target.ownership === 'managed')
          .map((target) => {
          const label = `${target.target_id} • ${target.agent} • ${target.ownership}`;
          return `<option value='${esc(target.target_id)}'>${esc(label)}</option>`;
        }).join('');
        return `
          <form class='action-card form-grid' id='project-form'>
            <div class='section-label'>Project</div>
            <input type='hidden' name='skill' value='${esc(skillName)}' />
            <div class='field'>
              <label for='project-binding'>Binding</label>
              <select id='project-binding' name='binding'>${bindingOptions}</select>
            </div>
            <div class='field two'>
              <div class='field'>
                <label for='project-target'>Target override</label>
                <select id='project-target' name='target'>
                  <option value=''>Use binding default</option>
                  ${targetOptions}
                </select>
              </div>
              <div class='field'>
                <label for='project-method'>Method</label>
                <select id='project-method' name='method'>
                  <option value='symlink'>symlink</option>
                  <option value='copy'>copy</option>
                  <option value='materialize'>materialize</option>
                </select>
              </div>
            </div>
            <div class='action-row'>
              <div class='action-note'>Projects <span class='mono'>skills/${esc(skillName)}</span> into a managed target.</div>
              <button class='btn' type='submit'>Project Skill</button>
            </div>
          </form>
        `;
      }

      function renderCaptureForm(skillName, info) {
        const projections = info.projections || [];
        if (projections.length === 0) {
          return `
            <div class='action-card'>
              <div class='section-label'>Capture</div>
              <p class='action-note'>Create a projection first. Capture only works from an explicit projection instance.</p>
            </div>
          `;
        }
        const options = projections.map((projection, index) => {
          const target = projection.target?.target_id || projection.target_id;
          const drift = projectionIsDrifted(projection) ? ' • drift' : '';
          const label = `${projection.instance_id} • ${target} • ${projection.method}${drift}`;
          return `<option value='${esc(projection.instance_id)}' ${index === 0 ? 'selected' : ''}>${esc(label)}</option>`;
        }).join('');
        return `
          <form class='action-card form-grid' id='capture-form'>
            <div class='section-label'>Capture</div>
            <input type='hidden' name='skill' value='${esc(skillName)}' />
            <div class='field'>
              <label for='capture-instance'>Projection instance</label>
              <select id='capture-instance' name='instance'>${options}</select>
            </div>
            <div class='field'>
              <label for='capture-message'>Commit message</label>
              <input id='capture-message' name='message' placeholder='capture(skill): sync live edits' />
            </div>
            <div class='action-row'>
              <div class='action-note'>Capture records the live projection back into canonical source and commits it.</div>
              <button class='btn' type='submit'>Capture Changes</button>
            </div>
          </form>
        `;
      }

      function renderSkills(root) {
        const query = state.query.trim().toLowerCase();
        const filtered = state.skills.filter((name) => name.toLowerCase().includes(query));

        if (!state.selectedSkill || !filtered.includes(state.selectedSkill)) {
          state.selectedSkill = filtered[0] || null;
        }

        const left = filtered.length === 0
          ? `<div class='event'>No skill matches your search.</div>`
          : `<div class='skills-grid'>${filtered.map((name) => {
              const info = skillInfo(name);
              const linked = state.legacyTargets?.skills?.[name];
              const chips = [];
              if (state.v3View?.available) {
                chips.push(`<span class='tiny-chip ${info.projections.length > 0 ? 'linked' : 'unlinked'}'>${info.projections.length || 0} projection${info.projections.length === 1 ? '' : 's'}</span>`);
                chips.push(`<span class='tiny-chip'>${info.bindingIds.length || 0} binding${info.bindingIds.length === 1 ? '' : 's'}</span>`);
                if (info.driftedCount) chips.push(`<span class='tiny-chip drift'>Drift ${info.driftedCount}</span>`);
                info.methods.slice(0, 2).forEach((method) => chips.push(`<span class='tiny-chip'>${esc(method)}</span>`));
                if (chips.length === 2 && info.methods.length === 0) chips.push(`<span class='tiny-chip unlinked'>No projection rule</span>`);
              } else {
                if (linked?.claude_path) chips.push(`<span class='tiny-chip linked'>Claude</span>`);
                if (linked?.codex_path) chips.push(`<span class='tiny-chip linked'>Codex</span>`);
                if (!linked) chips.push(`<span class='tiny-chip unlinked'>Unlinked</span>`);
                if (linked?.method) chips.push(`<span class='tiny-chip'>${esc(linked.method)}</span>`);
              }
              return `
                <article class='skill-card ${state.selectedSkill === name ? 'active' : ''}' data-skill='${esc(name)}'>
                  <h3 class='skill-name'>${esc(name)}</h3>
                  <div class='chip-wrap'>${chips.join('')}</div>
                </article>
              `;
            }).join('')}</div>`;

        let detail = `<div class='event'>Select a skill to inspect command shortcuts.</div>`;
        if (state.selectedSkill) {
          const info = skillInfo(state.selectedSkill);
          const bindings = info.bindingIds
            .map((bindingId) => state.v3View.bindings.find((binding) => binding.binding_id === bindingId))
            .filter(Boolean);
          const targets = info.targetIds
            .map((targetId) => state.v3View.targets.find((target) => target.target_id === targetId))
            .filter(Boolean);
          const linked = state.legacyTargets?.skills?.[state.selectedSkill] || {};
          const saveCmd = `loom --json --root "${root}" skill save ${state.selectedSkill}`;
          const snapCmd = `loom --json --root "${root}" skill snapshot ${state.selectedSkill}`;
          const relCmd = `loom --json --root "${root}" skill release ${state.selectedSkill} v1.0.0`;
          const diffCmd = `loom --json --root "${root}" skill diff ${state.selectedSkill} HEAD~1 HEAD`;
          const bindingCmd = bindings[0] ? `loom --json --root "${root}" workspace binding show ${bindings[0].binding_id}` : null;
          const targetCmd = targets[0] ? `loom --json --root "${root}" target show ${targets[0].target_id}` : null;
          const actions = `
            ${renderProjectForm(state.selectedSkill, info)}
            ${renderCaptureForm(state.selectedSkill, info)}
          `;
          detail = `
            <div class='card skill-detail'>
              <h3 style='margin:0 0 8px;'>${esc(state.selectedSkill)}</h3>
              <p class='note'>Canonical source: <span class='mono'>skills/${esc(state.selectedSkill)}</span></p>
              ${state.v3View?.available ? `
                <div class='skill-stat-grid'>
                  <div class='stat'><div class='metric-k'>Bindings</div><div class='metric-v'>${info.bindingIds.length}</div></div>
                  <div class='stat'><div class='metric-k'>Targets</div><div class='metric-v'>${info.targetIds.length}</div></div>
                  <div class='stat'><div class='metric-k'>Projections</div><div class='metric-v'>${info.projections.length}</div></div>
                  <div class='stat'><div class='metric-k'>Drift</div><div class='metric-v'>${info.driftedCount}</div></div>
                </div>
                <div class='stack'>
                  <div>
                    <div class='section-label'>Bindings</div>
                    ${bindings.length > 0 ? `<div class='entity-list'>
                      ${bindings.map((binding) => `
                        <article class='entity-row'>
                          <div class='entity-title'>${esc(binding.binding_id)}</div>
                          <div class='entity-path'>${esc(binding.agent)} • ${esc(binding.profile_id)} • ${esc(matcherText(binding.workspace_matcher))}</div>
                          <div class='chip-wrap' style='margin-top:8px;'>
                            <span class='tiny-chip ${binding.active ? 'linked' : 'unlinked'}'>${binding.active ? 'active' : 'inactive'}</span>
                            <span class='tiny-chip'>${esc(binding.policy_profile)}</span>
                            <span class='tiny-chip'>target ${esc(binding.default_target_id)}</span>
                          </div>
                        </article>
                      `).join('')}
                    </div>` : `<div class='event'>No v3 binding rules for this skill yet.</div>`}
                  </div>
                  <div>
                    <div class='section-label'>Projections</div>
                    ${info.projections.length > 0 ? `<div class='entity-list'>
                      ${info.projections.map((projection) => `
                        <article class='entity-row ${projectionIsDrifted(projection) ? 'drift' : ''}'>
                          <div class='entity-title'>${esc(projection.target?.target_id || projection.target_id)}</div>
                          <div class='entity-path'>${esc(projection.materialized_path)}</div>
                          <div class='chip-wrap' style='margin-top:8px;'>
                            <span class='tiny-chip ${projectionIsDrifted(projection) ? 'drift' : 'linked'}'>${esc(projection.health || 'unknown')}</span>
                            <span class='tiny-chip'>${esc(projection.method)}</span>
                            <span class='tiny-chip'>rev ${esc(projection.last_applied_rev || '-')}</span>
                          </div>
                        </article>
                      `).join('')}
                    </div>` : `<div class='event'>No active projections yet.</div>`}
                  </div>
                </div>
              ` : `
                <div class='event mono'>
                  <div>Method: ${esc(linked.method || 'n/a')}</div>
                  <div>Claude: ${esc(linked.claude_path || '-')}</div>
                  <div>Codex: ${esc(linked.codex_path || '-')}</div>
                </div>
              `}
              <div class='commands'>
                ${[saveCmd, snapCmd, relCmd, diffCmd, bindingCmd, targetCmd].filter(Boolean).map((cmd) => `
                  <div class='command-item mono'>
                    <code>${esc(cmd)}</code>
                    <button class='cmd-btn copy-btn' data-cmd='${esc(cmd)}'>Copy</button>
                  </div>
                `).join('')}
              </div>
              <div class='stack' style='margin-top:12px;'>
                ${actions}
              </div>
            </div>
          `;
        }

        return `
          <h2 class='view-title'>Skills</h2>
          <p class='view-sub'>Search and inspect managed skills with agent-ready commands.</p>
          ${renderV3Notice()}
          <div class='skills-head'>
            <div class='chips'>
              <span class='chip'>Total: ${state.skills.length}</span>
              <span class='chip'>Filtered: ${filtered.length}</span>
            </div>
            <input id='skill-query' class='skills-search' placeholder='Search skills...' value='${esc(state.query)}' />
          </div>
          <div class='skills-layout'>
            ${left}
            ${detail}
          </div>
        `;
      }

      function renderOps() {
        const ops = state.pending?.ops || [];
        const v3 = state.v3View || emptyV3View();
        const events = ops.length === 0
          ? `<div class='event'>Pending queue is empty.</div>`
          : ops.slice(-40).reverse().map((op) => `
              <article class='event'>
                <div class='event-head mono'>
                  <span>${esc(op.request_id || '-')}</span>
                  <span>${esc(ts(op.created_at))}</span>
                </div>
                <div style='margin-top:4px;font-weight:700;'>${esc(op.command || '-')}</div>
              </article>
            `).join('');

        const warnings = state.remoteWarnings || [];

        return `
          <h2 class='view-title'>Ops</h2>
          <p class='view-sub'>Queue timeline, sync warnings and operational status.</p>
          ${renderV3Notice()}
          <div class='grid overview' style='margin-bottom:12px;'>
            <div class='card'><div class='metric-k'>Queued</div><div class='metric-v'>${esc(state.pending?.count || 0)}</div></div>
            <div class='card'><div class='metric-k'>V3 Ops</div><div class='metric-v'>${esc(v3.counts.operations || 0)}</div></div>
            <div class='card'><div class='metric-k'>Drifted</div><div class='metric-v'>${esc(v3.counts.drifted_projections || 0)}</div></div>
            <div class='card'><div class='metric-k'>Last Scanned</div><div class='metric-v mono'>${esc(v3.checkpoint?.last_scanned_op_id || '-')}</div></div>
          </div>
          <div class='timeline'>${events}</div>
          ${v3.drifted.length > 0 ? `
            <aside class='warning'>
              <strong class='status-danger'>Projection drift</strong>
              <ul>${v3.drifted.slice(0, 8).map((item) => `<li><span class='mono'>${esc(item.skill_id)}</span> → ${esc(item.projection.materialized_path)}</li>`).join('')}</ul>
            </aside>
          ` : ''}
          ${warnings.length > 0 ? `
            <aside class='warning'>
              <strong class='status-danger'>Remote warnings</strong>
              <ul>${warnings.map((w) => `<li>${esc(w)}</li>`).join('')}</ul>
            </aside>
          ` : ''}
        `;
      }

      function renderSettings(root) {
        const info = state.info || {};
        const v3 = state.v3View || emptyV3View();
        const migration = state.migration || { ok: false, data: null, error: null };
        const migrationData = migration.data?.migration || null;
        const commands = [
          `loom --json --root "${root}" sync status`,
          `loom --json --root "${root}" workspace doctor`,
          `loom --json --root "${root}" workspace binding list`,
          `loom --json --root "${root}" target list`,
          `loom --json --root "${root}" migrate v2-to-v3 --plan`,
        ];

        return `
          <h2 class='view-title'>Settings</h2>
          <p class='view-sub'>Environment bindings and operator references.</p>
          ${renderV3Notice()}
          <div class='grid' style='grid-template-columns:1fr 1fr;'>
            <div class='card mono'>
              <div>Root: ${esc(info.root || '-')}</div>
              <div>State dir: ${esc(info.state_dir || '-')}</div>
              <div>V3 dir: ${esc(info.state_dir ? `${info.state_dir}/v3` : '-')}</div>
            </div>
            <div class='card mono'>
              <div>Claude dir: ${esc(info.claude_dir || '-')}</div>
              <div>Codex dir: ${esc(info.codex_dir || '-')}</div>
              <div>Remote URL: ${esc(info.remote_url || '-')}</div>
            </div>
          </div>
          <div class='split-grid'>
            <div class='card'>
              <div class='section-label'>State Model</div>
              <div class='chip-wrap'>
                <span class='tiny-chip ${v3.available ? 'linked' : 'unlinked'}'>${v3.available ? 'v3 loaded' : 'legacy fallback'}</span>
                <span class='tiny-chip'>bindings ${esc(v3.counts.bindings || 0)}</span>
                <span class='tiny-chip'>targets ${esc(v3.counts.targets || 0)}</span>
                <span class='tiny-chip'>projections ${esc(v3.counts.projections || 0)}</span>
              </div>
            </div>
            <div class='card'>
              <div class='section-label'>Checkpoint</div>
              <div class='mono'>last_scanned: ${esc(v3.checkpoint?.last_scanned_op_id || '-')}</div>
              <div class='mono'>updated_at: ${esc(ts(v3.checkpoint?.updated_at))}</div>
            </div>
          </div>
          <div class='card' style='margin-top:12px;'>
            <div class='section-label'>Migration Review</div>
            ${migration.ok && migrationData ? `
              <div class='chip-wrap'>
                <span class='tiny-chip'>legacy skills ${esc(migrationData.legacy_skill_count || 0)}</span>
                <span class='tiny-chip'>candidate targets ${esc((migrationData.candidate_targets || []).length)}</span>
                <span class='tiny-chip ${((migrationData.unresolved || []).length > 0) ? 'drift' : 'linked'}'>unresolved ${esc((migrationData.unresolved || []).length)}</span>
              </div>
              <div class='action-row' style='margin-top:10px;'>
                <div class='action-note'>Apply only writes observed targets into <span class='mono'>state/v3</span>. It does not rewrite live agent directories.</div>
                <button class='btn' id='migration-apply-btn' ${((migrationData.unresolved || []).length > 0 || (migrationData.candidate_targets || []).length === 0) ? 'disabled' : ''}>Apply Migration</button>
              </div>
              ${(migrationData.warnings || []).length > 0 ? `
                <aside class='warning' style='margin-top:10px;'>
                  <strong>Warnings</strong>
                  <ul>${migrationData.warnings.map((item) => `<li>${esc(item)}</li>`).join('')}</ul>
                </aside>
              ` : `<p class='action-note' style='margin-top:10px;'>No migration warnings.</p>`}
            ` : `
              <p class='action-note'>${esc(migration.error?.message || 'Migration plan unavailable.')}</p>
            `}
          </div>
          <div class='split-grid'>
            <form class='action-card form-grid' id='target-add-form'>
              <div class='section-label'>Add Target</div>
              <div class='field two'>
                <div class='field'>
                  <label for='target-agent'>Agent</label>
                  <select id='target-agent' name='agent'>
                    <option value='claude'>claude</option>
                    <option value='codex'>codex</option>
                  </select>
                </div>
                <div class='field'>
                  <label for='target-ownership'>Ownership</label>
                  <select id='target-ownership' name='ownership'>
                    <option value='managed'>managed</option>
                    <option value='observed'>observed</option>
                    <option value='external'>external</option>
                  </select>
                </div>
              </div>
              <div class='field'>
                <label for='target-path'>Absolute path</label>
                <input id='target-path' name='path' placeholder='/absolute/path/to/skills' />
              </div>
              <div class='action-row'>
                <div class='action-note'>Managed targets can be projected into. Observed and external targets are read-only references.</div>
                <button class='btn' type='submit'>Add Target</button>
              </div>
            </form>
            <form class='action-card form-grid' id='binding-add-form'>
              <div class='section-label'>Add Workspace Binding</div>
              <div class='field two'>
                <div class='field'>
                  <label for='binding-agent'>Agent</label>
                  <select id='binding-agent' name='agent'>
                    <option value='claude'>claude</option>
                    <option value='codex'>codex</option>
                  </select>
                </div>
                <div class='field'>
                  <label for='binding-target'>Default target</label>
                  <select id='binding-target' name='target'>
                    ${v3.targets.length > 0
                      ? v3.targets.map((target, index) => {
                          const label = `${target.target_id} • ${target.agent} • ${target.ownership}`;
                          return `<option value='${esc(target.target_id)}' ${index === 0 ? 'selected' : ''}>${esc(label)}</option>`;
                        }).join('')
                      : `<option value=''>No targets loaded</option>`}
                  </select>
                </div>
              </div>
              <div class='field'>
                <label for='binding-profile'>Profile</label>
                <input id='binding-profile' name='profile' placeholder='project-a' />
              </div>
              <div class='field two'>
                <div class='field'>
                  <label for='binding-matcher-kind'>Matcher kind</label>
                  <select id='binding-matcher-kind' name='matcher_kind'>
                    <option value='path-prefix'>path-prefix</option>
                    <option value='exact-path'>exact-path</option>
                    <option value='name'>name</option>
                  </select>
                </div>
                <div class='field'>
                  <label for='binding-policy'>Policy profile</label>
                  <input id='binding-policy' name='policy_profile' value='safe-capture' />
                </div>
              </div>
              <div class='field'>
                <label for='binding-matcher-value'>Matcher value</label>
                <input id='binding-matcher-value' name='matcher_value' placeholder='/Users/me/project-a' />
              </div>
              <div class='action-row'>
                <div class='action-note'>Bindings tell Loom which workspace maps to which default target.</div>
                <button class='btn' type='submit' ${v3.targets.length === 0 ? 'disabled' : ''}>Add Binding</button>
              </div>
            </form>
          </div>
          <div class='split-grid'>
            <div class='card'>
              <div class='section-label'>Registered Targets</div>
              <div class='entity-list'>
                ${v3.targets.length > 0
                  ? v3.targets.map((target) => {
                      const usage = targetUsage(target.target_id);
                      return `
                        <article class='entity-row'>
                          <div class='action-row'>
                            <div>
                              <div class='entity-title'>${esc(target.target_id)}</div>
                              <div class='entity-path'>${esc(target.agent)} • ${esc(target.ownership)} • ${esc(target.path)}</div>
                            </div>
                            <button class='cmd-btn danger remove-target-btn' data-target-id='${esc(target.target_id)}'>Remove</button>
                          </div>
                          <div class='chip-wrap' style='margin-top:8px;'>
                            <span class='tiny-chip'>bindings ${usage.bindings.length}</span>
                            <span class='tiny-chip'>rules ${usage.rules.length}</span>
                            <span class='tiny-chip'>projections ${usage.projections.length}</span>
                          </div>
                        </article>
                      `;
                    }).join('')
                  : `<div class='event'>No registered targets.</div>`}
              </div>
            </div>
            <div class='card'>
              <div class='section-label'>Workspace Bindings</div>
              <div class='entity-list'>
                ${v3.bindings.length > 0
                  ? v3.bindings.map((binding) => {
                      const usage = bindingUsage(binding.binding_id);
                      return `
                        <article class='entity-row'>
                          <div class='action-row'>
                            <div>
                              <div class='entity-title'>${esc(binding.binding_id)}</div>
                              <div class='entity-path'>${esc(binding.agent)} • ${esc(binding.profile_id)} • ${esc(matcherText(binding.workspace_matcher))}</div>
                            </div>
                            <button class='cmd-btn danger remove-binding-btn' data-binding-id='${esc(binding.binding_id)}'>Remove</button>
                          </div>
                          <div class='chip-wrap' style='margin-top:8px;'>
                            <span class='tiny-chip'>rules ${usage.rules.length}</span>
                            <span class='tiny-chip'>projections ${usage.projections.length}</span>
                            <span class='tiny-chip'>target ${esc(binding.default_target_id)}</span>
                          </div>
                        </article>
                      `;
                    }).join('')
                  : `<div class='event'>No workspace bindings.</div>`}
              </div>
            </div>
          </div>
          <div class='commands'>
            ${commands.map((cmd) => `
              <div class='command-item mono'>
                <code>${esc(cmd)}</code>
                <button class='cmd-btn copy-btn' data-cmd='${esc(cmd)}'>Copy</button>
              </div>
            `).join('')}
          </div>
        `;
      }

      function renderView() {
        const root = state.info?.root || '<root>';
        const body = state.route === 'skills'
          ? renderSkills(root)
          : state.route === 'ops'
            ? renderOps()
            : state.route === 'settings'
              ? renderSettings(root)
              : renderOverview(root);
        return `${renderActionResult()}${body}`;
      }

      function bindDynamicEvents() {
        document.getElementById('skill-query')?.addEventListener('input', (e) => {
          state.query = e.target.value;
          render();
        });

        document.getElementById('target-add-form')?.addEventListener('submit', (e) => {
          e.preventDefault();
          const form = new FormData(e.currentTarget);
          const path = String(form.get('path') || '').trim();
          if (!path) {
            showToast('path required');
            return;
          }
          void runAction('/api/v3/targets', {
            agent: String(form.get('agent') || 'claude'),
            ownership: String(form.get('ownership') || 'managed'),
            path,
          }, 'Target added');
        });

        document.getElementById('binding-add-form')?.addEventListener('submit', (e) => {
          e.preventDefault();
          const form = new FormData(e.currentTarget);
          const target = String(form.get('target') || '').trim();
          const profile = String(form.get('profile') || '').trim();
          const matcherValue = String(form.get('matcher_value') || '').trim();
          if (!target || !profile || !matcherValue) {
            showToast('binding fields required');
            return;
          }
          void runAction('/api/v3/bindings', {
            agent: String(form.get('agent') || 'claude'),
            profile,
            matcher_kind: String(form.get('matcher_kind') || 'path-prefix'),
            matcher_value: matcherValue,
            target,
            policy_profile: String(form.get('policy_profile') || 'safe-capture').trim() || 'safe-capture',
          }, 'Binding added');
        });

        document.getElementById('project-form')?.addEventListener('submit', (e) => {
          e.preventDefault();
          const form = new FormData(e.currentTarget);
          const skill = String(form.get('skill') || '').trim();
          const binding = String(form.get('binding') || '').trim();
          if (!skill || !binding) {
            showToast('project selector required');
            return;
          }
          const target = String(form.get('target') || '').trim();
          void runAction('/api/v3/project', {
            skill,
            binding,
            target: target || null,
            method: String(form.get('method') || 'symlink'),
          }, 'Projection updated');
        });

        document.getElementById('capture-form')?.addEventListener('submit', (e) => {
          e.preventDefault();
          const form = new FormData(e.currentTarget);
          const instance = String(form.get('instance') || '').trim();
          const skill = String(form.get('skill') || '').trim();
          if (!instance) {
            showToast('instance required');
            return;
          }
          const message = String(form.get('message') || '').trim();
          void runAction('/api/v3/capture', {
            skill: skill || null,
            instance,
            message: message || null,
          }, 'Capture committed');
        });

        document.querySelectorAll('.remove-target-btn').forEach((btn) => {
          btn.addEventListener('click', () => {
            const targetId = btn.getAttribute('data-target-id') || '';
            if (!targetId) return;
            void runAction(`/api/v3/targets/${encodeURIComponent(targetId)}/remove`, {}, 'Target removed');
          });
        });

        document.querySelectorAll('.remove-binding-btn').forEach((btn) => {
          btn.addEventListener('click', () => {
            const bindingId = btn.getAttribute('data-binding-id') || '';
            if (!bindingId) return;
            void runAction(`/api/v3/bindings/${encodeURIComponent(bindingId)}/remove`, {}, 'Binding removed');
          });
        });

        document.getElementById('migration-apply-btn')?.addEventListener('click', () => {
          void runAction('/api/v3/migration/apply', {}, 'Migration applied');
        });

        document.querySelectorAll('.skill-card').forEach((node) => {
          node.addEventListener('click', () => {
            state.selectedSkill = node.getAttribute('data-skill');
            render();
          });
        });

        document.querySelectorAll('.copy-btn').forEach((btn) => {
          btn.addEventListener('click', () => {
            const cmd = btn.getAttribute('data-cmd') || '';
            void copyText(cmd);
          });
        });
      }

      function render() {
        renderNav();
        renderTopChips();
        document.getElementById('view').innerHTML = renderView();
        bindDynamicEvents();
      }

      async function load() {
        const [h, i, s, t, v, r, p, m] = await Promise.all([
          fetch('/api/health').then((x) => x.json()),
          fetch('/api/info').then((x) => x.json()),
          fetch('/api/skills').then((x) => x.json()),
          fetch('/api/targets').then((x) => x.json()),
          fetch('/api/v3/status').then((x) => x.json()),
          fetch('/api/remote/status').then((x) => x.json()),
          fetch('/api/pending').then((x) => x.json()),
          fetch('/api/v3/migration/plan').then((x) => x.json()),
        ]);

        state.health = h;
        state.info = i;
        state.skills = s.skills || [];
        state.legacyTargets = t.targets || { skills: {} };
        state.v3 = v || { ok: false, error: { message: 'Missing v3 payload' } };
        state.v3View = buildV3View(state.skills, v);
        state.remote = r.remote || {};
        state.remoteWarnings = r.warnings || [];
        state.pending = p || { count: 0, ops: [] };
        state.migration = m || { ok: false, error: { message: 'Missing migration payload' } };
        state.loadedAt = new Date().toISOString();
        state.route = routeFromHash();

        render();
      }

      window.addEventListener('hashchange', () => {
        state.route = routeFromHash();
        render();
      });

      document.getElementById('refresh-btn').addEventListener('click', () => {
        void load();
      });

      state.v3View = emptyV3View();
      state.route = routeFromHash();
      void load();
    </script>
  </body>
</html>
"#,
    )
}

// Keep the embedded icon with the fallback deck so both assets can be reactivated together.
#[allow(dead_code)]
async fn favicon() -> impl IntoResponse {
    (
        [
            ("content-type", "image/svg+xml; charset=utf-8"),
            ("cache-control", "public, max-age=86400"),
        ],
        include_str!("../assets/loom-icon.svg"),
    )
}

async fn frontend_index(State(state): State<PanelState>) -> Response {
    serve_panel_asset(state.dist_dir.join("index.html"))
}

async fn frontend_static_asset(
    AxumPath(path): AxumPath<String>,
    State(state): State<PanelState>,
) -> Response {
    let asset_path = match resolve_panel_asset_path(&state.dist_dir, &path) {
        Some(path) => path,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "invalid panel asset path".to_string(),
            )
                .into_response();
        }
    };
    serve_panel_asset(asset_path)
}

fn ensure_panel_dist(dist_dir: &Path) -> Result<()> {
    let index_path = dist_dir.join("index.html");
    if index_path.is_file() {
        Ok(())
    } else {
        Err(anyhow!(
            "panel frontend not built; expected {}",
            index_path.display()
        ))
    }
}

fn resolve_panel_asset_path(dist_dir: &Path, requested: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in PathBuf::from(requested).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(dist_dir.join(relative))
}

fn serve_panel_asset(path: PathBuf) -> Response {
    match fs::read(&path) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", content_type_for(path.as_path()))
            .body(Body::from(bytes))
            .unwrap_or_else(|err| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to build asset response: {}", err),
                )
                    .into_response()
            }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (
            StatusCode::NOT_FOUND,
            format!("panel asset not found: {}", path.display()),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read panel asset {}: {}", path.display(), err),
        )
            .into_response(),
    }
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true, "service": "loom-panel"}))
}

async fn info(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let target_dirs = resolve_agent_skill_dirs();
    let remote_url = crate::gitops::remote_url(&state.ctx)
        .ok()
        .flatten()
        .unwrap_or_default();

    Json(json!({
        "root": state.ctx.root.display().to_string(),
        "state_dir": state.ctx.state_dir.display().to_string(),
        "targets_file": state.ctx.targets_file.display().to_string(),
        "claude_dir": target_dirs.claude.display().to_string(),
        "codex_dir": target_dirs.codex.display().to_string(),
        "remote_url": remote_url,
    }))
}

async fn skills(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match list_skills(&state.ctx) {
        Ok(skills) => Json(json!({"skills": skills})),
        Err(err) => Json(json!({"skills": [], "error": err.to_string()})),
    }
}

async fn targets(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match state.ctx.load_targets() {
        Ok(targets) => Json(json!({"targets": targets})),
        Err(err) => Json(json!({"targets": {"skills": {}}, "error": err.to_string()})),
    }
}

async fn v3_status(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(snapshot.status_view()),
        Err(err) => err,
    }
}

async fn v3_bindings(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(json!({
            "state_model": "v3",
            "count": snapshot.bindings.bindings.len(),
            "bindings": snapshot.bindings.bindings
        })),
        Err(err) => err,
    }
}

async fn v3_binding_show(
    AxumPath(binding_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let binding = match snapshot.binding(&binding_id).cloned() {
        Some(binding) => binding,
        None => {
            return v3_error(
                "BINDING_NOT_FOUND",
                format!("binding '{}' not found", binding_id),
            );
        }
    };

    v3_ok(json!({
        "state_model": "v3",
        "binding": binding,
        "default_target": snapshot.binding_default_target(&binding),
        "rules": snapshot.binding_rules(&binding.binding_id),
        "projections": snapshot.binding_projections(&binding.binding_id)
    }))
}

async fn v3_targets(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(json!({
            "state_model": "v3",
            "count": snapshot.targets.targets.len(),
            "targets": snapshot.targets.targets
        })),
        Err(err) => err,
    }
}

async fn v3_target_show(
    AxumPath(target_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let target = match snapshot.target(&target_id).cloned() {
        Some(target) => target,
        None => {
            return v3_error(
                "TARGET_NOT_FOUND",
                format!("target '{}' not found", target_id),
            );
        }
    };

    v3_ok(json!({
        "state_model": "v3",
        "target": target,
        "bindings": snapshot.target_bindings(&target_id),
        "rules": snapshot.target_rules(&target_id),
        "projections": snapshot.target_projections(&target_id)
    }))
}

async fn v3_migration_plan(State(state): State<PanelState>) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Migrate {
            command: MigrateCommand::V2ToV3(MigrateV2ToV3Args {
                plan: true,
                apply: false,
            }),
        },
    )
}

async fn v3_migration_apply(State(state): State<PanelState>) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Migrate {
            command: MigrateCommand::V2ToV3(MigrateV2ToV3Args {
                plan: false,
                apply: true,
            }),
        },
    )
}

async fn v3_target_add(
    State(state): State<PanelState>,
    Json(req): Json<TargetAddRequest>,
) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Target {
            command: TargetCommand::Add(TargetAddArgs {
                agent: req.agent,
                path: req.path,
                ownership: req.ownership.unwrap_or(TargetOwnership::Managed),
            }),
        },
    )
}

async fn v3_target_remove(
    AxumPath(target_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Target {
            command: TargetCommand::Remove(crate::cli::TargetShowArgs { target_id }),
        },
    )
}

async fn v3_binding_add(
    State(state): State<PanelState>,
    Json(req): Json<BindingAddRequest>,
) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Workspace {
            command: WorkspaceCommand::Binding {
                command: WorkspaceBindingCommand::Add(BindingAddArgs {
                    agent: req.agent,
                    profile: req.profile,
                    matcher_kind: req.matcher_kind,
                    matcher_value: req.matcher_value,
                    target: req.target,
                    policy_profile: req
                        .policy_profile
                        .unwrap_or_else(|| "safe-capture".to_string()),
                }),
            },
        },
    )
}

async fn v3_binding_remove(
    AxumPath(binding_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Workspace {
            command: WorkspaceCommand::Binding {
                command: WorkspaceBindingCommand::Remove(crate::cli::BindingShowArgs {
                    binding_id,
                }),
            },
        },
    )
}

async fn v3_project(
    State(state): State<PanelState>,
    Json(req): Json<ProjectRequest>,
) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Skill {
            command: crate::cli::SkillCommand::Project(ProjectArgs {
                skill: req.skill,
                binding: req.binding,
                target: req.target,
                method: req.method.unwrap_or(ProjectionMethod::Symlink),
            }),
        },
    )
}

async fn v3_capture(
    State(state): State<PanelState>,
    Json(req): Json<CaptureRequest>,
) -> Json<serde_json::Value> {
    run_panel_command(
        &state,
        Command::Skill {
            command: crate::cli::SkillCommand::Capture(CaptureArgs {
                skill: req.skill,
                binding: req.binding,
                instance: req.instance,
                message: req.message,
            }),
        },
    )
}

async fn remote_status(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match remote_status_payload(&state.ctx) {
        Ok((remote, meta)) => Json(json!({"remote": remote, "warnings": meta.warnings})),
        Err(err) => Json(json!({"error": err.message, "code": err.code.as_str()})),
    }
}

async fn pending(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match state.ctx.read_pending_report() {
        Ok(report) => Json(json!({
            "count": report.ops.len(),
            "ops": report.ops,
            "journal_events": report.journal_events,
            "history_events": report.history_events,
            "warnings": report.warnings
        })),
        Err(err) => Json(json!({"count": 0, "ops": [], "error": err.to_string()})),
    }
}

fn load_v3_snapshot(
    ctx: &AppContext,
) -> std::result::Result<crate::v3::V3Snapshot, Json<serde_json::Value>> {
    let paths = V3StatePaths::from_root(&ctx.root);
    match paths.maybe_load_snapshot() {
        Ok(Some(snapshot)) => Ok(snapshot),
        Ok(None) => Err(v3_error(
            "ARG_INVALID",
            format!("v3 state not initialized under {}", paths.v3_dir.display()),
        )),
        Err(err) => {
            let message = err.to_string();
            let code = if message.contains("schema version mismatch") {
                "SCHEMA_MISMATCH"
            } else {
                "STATE_CORRUPT"
            };
            Err(v3_error(code, message))
        }
    }
}

fn v3_ok(data: serde_json::Value) -> Json<serde_json::Value> {
    Json(json!({"ok": true, "data": data}))
}

fn v3_error(code: &str, message: String) -> Json<serde_json::Value> {
    Json(json!({"ok": false, "error": {"code": code, "message": message}}))
}

fn run_panel_command(state: &PanelState, command: Command) -> Json<serde_json::Value> {
    let app = crate::commands::App {
        ctx: (*state.ctx).clone(),
    };
    let cli = Cli {
        json: true,
        request_id: Some(Uuid::new_v4().to_string()),
        root: Some(state.ctx.root.clone()),
        command,
    };

    match app.execute(cli) {
        Ok((envelope, _code)) => {
            let payload = serde_json::to_value(envelope)
                .unwrap_or_else(|err| internal_error_payload(err.to_string()));
            Json(payload)
        }
        Err(err) => Json(internal_error_payload(err.to_string())),
    }
}

fn internal_error_payload(message: String) -> serde_json::Value {
    json!({
        "ok": false,
        "error": {
            "code": "INTERNAL_ERROR",
            "message": message
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{content_type_for, internal_error_payload, resolve_panel_asset_path};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn resolve_panel_asset_path_rejects_invalid_components() {
        let dist_dir = Path::new("/tmp/panel-dist");

        assert_eq!(
            resolve_panel_asset_path(dist_dir, "assets/index.js"),
            Some(dist_dir.join("assets/index.js"))
        );
        assert_eq!(
            resolve_panel_asset_path(dist_dir, "./assets/index.css"),
            Some(dist_dir.join("assets/index.css"))
        );
        assert_eq!(resolve_panel_asset_path(dist_dir, "../secret.txt"), None);
        assert_eq!(resolve_panel_asset_path(dist_dir, "/etc/passwd"), None);
    }

    #[test]
    fn content_type_for_maps_known_panel_extensions() {
        assert_eq!(
            content_type_for(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("bundle.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("styles.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type_for(Path::new("favicon.svg")), "image/svg+xml");
        assert_eq!(content_type_for(Path::new("font.woff2")), "font/woff2");
        assert_eq!(
            content_type_for(Path::new("artifact.bin")),
            "application/octet-stream"
        );
    }

    #[test]
    fn internal_error_payload_uses_expected_shape() {
        assert_eq!(
            internal_error_payload("boom".to_string()),
            json!({
                "ok": false,
                "error": {
                    "code": "INTERNAL_ERROR",
                    "message": "boom"
                }
            })
        );
    }
}
