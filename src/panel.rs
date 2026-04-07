use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{Json, Router, extract::State, response::Html, response::IntoResponse, routing::get};
use serde_json::json;

use crate::commands::{list_skills, remote_status_payload};
use crate::state::AppContext;

#[derive(Clone)]
struct PanelState {
    ctx: Arc<AppContext>,
}

pub async fn run_panel(ctx: AppContext, port: u16) -> Result<()> {
    let state = PanelState { ctx: Arc::new(ctx) };

    let app = Router::new()
        .route("/", get(index))
        .route("/favicon.svg", get(favicon))
        .route("/api/health", get(health))
        .route("/api/info", get(info))
        .route("/api/skills", get(skills))
        .route("/api/targets", get(targets))
        .route("/api/remote/status", get(remote_status))
        .route("/api/pending", get(pending))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("panel listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

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

      .skill-detail {
        min-height: 280px;
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
      }

      @media (max-width: 620px) {
        .nav { grid-template-columns: repeat(2, minmax(0, 1fr)); }
        .grid.overview { grid-template-columns: 1fr; }
        .command-item { grid-template-columns: 1fr; }
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
        targets: { skills: {} },
        remote: null,
        remoteWarnings: [],
        pending: { count: 0, ops: [] },
        query: '',
        route: 'overview',
        selectedSkill: null,
        loadedAt: null,
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

      async function copyText(text) {
        try {
          await navigator.clipboard.writeText(text);
          showToast('Copied command');
        } catch {
          showToast('Copy failed');
        }
      }

      function routeFromHash() {
        const raw = (window.location.hash || '#overview').replace('#', '').toLowerCase();
        return ROUTES.includes(raw) ? raw : 'overview';
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
          `loom --json --root "${root}" workspace init --from-agent both --target both`,
          `loom --json --root "${root}" workspace status`,
          `loom --json --root "${root}" sync pull`,
          `loom --json --root "${root}" sync push`,
        ];
      }

      function renderTopChips() {
        const remote = state.remote || {};
        const html = [
          `<span class='chip'>Health: ${state.health?.ok ? 'OK' : 'UNKNOWN'}</span>`,
          `<span class='chip sync ${syncClass(remote.sync_state)}'>Sync: ${esc(remote.sync_state || 'UNKNOWN')}</span>`,
          `<span class='chip'>Skills: ${esc(state.skills.length)}</span>`,
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
        const linkedCount = state.skills.filter((name) => !!state.targets?.skills?.[name]).length;
        return `
          <h2 class='view-title'>Overview</h2>
          <p class='view-sub'>Runtime summary and quick commands • ${esc(ts(state.loadedAt))}</p>
          <div class='grid overview'>
            <div class='card'><div class='metric-k'>Skills</div><div class='metric-v'>${state.skills.length}</div></div>
            <div class='card'><div class='metric-k'>Linked</div><div class='metric-v'>${linkedCount}</div></div>
            <div class='card'><div class='metric-k'>Pending Ops</div><div class='metric-v'>${esc(state.pending?.count || 0)}</div></div>
            <div class='card'><div class='metric-k'>Ahead / Behind</div><div class='metric-v mono'>${esc(remote.ahead ?? 0)} / ${esc(remote.behind ?? 0)}</div></div>
          </div>
          <div class='card' style='margin-top:12px;'>
            <div class='metric-k'>Remote URL</div>
            <div class='mono' style='margin-top:6px;'>${esc(state.info?.remote_url || '-')}</div>
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

      function renderSkills(root) {
        const query = state.query.trim().toLowerCase();
        const filtered = state.skills.filter((name) => name.toLowerCase().includes(query));

        if (!state.selectedSkill || !state.skills.includes(state.selectedSkill)) {
          state.selectedSkill = filtered[0] || null;
        }

        const left = filtered.length === 0
          ? `<div class='event'>No skill matches your search.</div>`
          : `<div class='skills-grid'>${filtered.map((name) => {
              const linked = state.targets?.skills?.[name];
              const chips = [];
              if (linked?.claude_path) chips.push(`<span class='tiny-chip linked'>Claude</span>`);
              if (linked?.codex_path) chips.push(`<span class='tiny-chip linked'>Codex</span>`);
              if (!linked) chips.push(`<span class='tiny-chip unlinked'>Unlinked</span>`);
              if (linked?.method) chips.push(`<span class='tiny-chip'>${esc(linked.method)}</span>`);
              return `
                <article class='skill-card ${state.selectedSkill === name ? 'active' : ''}' data-skill='${esc(name)}'>
                  <h3 class='skill-name'>${esc(name)}</h3>
                  <div class='chip-wrap'>${chips.join('')}</div>
                </article>
              `;
            }).join('')}</div>`;

        let detail = `<div class='event'>Select a skill to inspect command shortcuts.</div>`;
        if (state.selectedSkill) {
          const linked = state.targets?.skills?.[state.selectedSkill] || {};
          const saveCmd = `loom --json --root "${root}" skill save ${state.selectedSkill}`;
          const snapCmd = `loom --json --root "${root}" skill snapshot ${state.selectedSkill}`;
          const relCmd = `loom --json --root "${root}" skill release ${state.selectedSkill} v1.0.0`;
          const diffCmd = `loom --json --root "${root}" skill diff ${state.selectedSkill} HEAD~1 HEAD`;
          detail = `
            <div class='card skill-detail'>
              <h3 style='margin:0 0 8px;'>${esc(state.selectedSkill)}</h3>
              <div class='event mono'>
                <div>Method: ${esc(linked.method || 'n/a')}</div>
                <div>Claude: ${esc(linked.claude_path || '-')}</div>
                <div>Codex: ${esc(linked.codex_path || '-')}</div>
              </div>
              <div class='commands'>
                ${[saveCmd, snapCmd, relCmd, diffCmd].map((cmd) => `
                  <div class='command-item mono'>
                    <code>${esc(cmd)}</code>
                    <button class='cmd-btn copy-btn' data-cmd='${esc(cmd)}'>Copy</button>
                  </div>
                `).join('')}
              </div>
            </div>
          `;
        }

        return `
          <h2 class='view-title'>Skills</h2>
          <p class='view-sub'>Search and inspect managed skills with agent-ready commands.</p>
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
          <div class='timeline'>${events}</div>
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
        const commands = [
          `loom --json --root "${root}" sync status`,
          `loom --json --root "${root}" workspace doctor`,
          `loom --json --root "${root}" workspace init --wizard`,
        ];

        return `
          <h2 class='view-title'>Settings</h2>
          <p class='view-sub'>Environment bindings and operator references.</p>
          <div class='grid' style='grid-template-columns:1fr 1fr;'>
            <div class='card mono'>
              <div>Root: ${esc(info.root || '-')}</div>
              <div>State dir: ${esc(info.state_dir || '-')}</div>
              <div>Targets file: ${esc(info.targets_file || '-')}</div>
            </div>
            <div class='card mono'>
              <div>Claude dir: ${esc(info.claude_dir || '-')}</div>
              <div>Codex dir: ${esc(info.codex_dir || '-')}</div>
              <div>Remote URL: ${esc(info.remote_url || '-')}</div>
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
        if (state.route === 'skills') return renderSkills(root);
        if (state.route === 'ops') return renderOps();
        if (state.route === 'settings') return renderSettings(root);
        return renderOverview(root);
      }

      function bindDynamicEvents() {
        document.getElementById('skill-query')?.addEventListener('input', (e) => {
          state.query = e.target.value;
          render();
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
        const [h, i, s, t, r, p] = await Promise.all([
          fetch('/api/health').then((x) => x.json()),
          fetch('/api/info').then((x) => x.json()),
          fetch('/api/skills').then((x) => x.json()),
          fetch('/api/targets').then((x) => x.json()),
          fetch('/api/remote/status').then((x) => x.json()),
          fetch('/api/pending').then((x) => x.json()),
        ]);

        state.health = h;
        state.info = i;
        state.skills = s.skills || [];
        state.targets = t.targets || { skills: {} };
        state.remote = r.remote || {};
        state.remoteWarnings = r.warnings || [];
        state.pending = p || { count: 0, ops: [] };
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

      state.route = routeFromHash();
      void load();
    </script>
  </body>
</html>
"#,
    )
}

async fn favicon() -> impl IntoResponse {
    (
        [
            ("content-type", "image/svg+xml; charset=utf-8"),
            ("cache-control", "public, max-age=86400"),
        ],
        include_str!("../assets/loom-icon.svg"),
    )
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true, "service": "loom-panel"}))
}

async fn info(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let (claude_dir, codex_dir) = resolve_target_dirs();
    let remote_url = crate::gitops::remote_url(&state.ctx)
        .ok()
        .flatten()
        .unwrap_or_default();

    Json(json!({
        "root": state.ctx.root.display().to_string(),
        "state_dir": state.ctx.state_dir.display().to_string(),
        "targets_file": state.ctx.targets_file.display().to_string(),
        "claude_dir": claude_dir,
        "codex_dir": codex_dir,
        "remote_url": remote_url,
    }))
}

async fn skills(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let skills = list_skills(&state.ctx).unwrap_or_default();
    Json(json!({"skills": skills}))
}

async fn targets(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let targets = state.ctx.load_targets().unwrap_or_default();
    Json(json!({"targets": targets}))
}

async fn remote_status(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match remote_status_payload(&state.ctx) {
        Ok((remote, meta)) => Json(json!({"remote": remote, "warnings": meta.warnings})),
        Err(err) => Json(json!({"error": err.message, "code": err.code.as_str()})),
    }
}

async fn pending(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let ops = state.ctx.read_pending().unwrap_or_default();
    Json(json!({"count": ops.len(), "ops": ops}))
}

fn resolve_target_dirs() -> (String, String) {
    let home = env::var("HOME").unwrap_or_else(|_| "~".to_string());
    let claude =
        env::var("CLAUDE_SKILLS_DIR").unwrap_or_else(|_| format!("{}/.claude/skills", home));
    let codex =
        env::var("CODEX_SKILLS_DIR").unwrap_or_else(|_| format!("{}/.codex/skills", home));
    (claude, codex)
}
