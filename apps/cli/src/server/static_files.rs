//! Static file serving with Single Page Application (SPA) history fallback.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, Response, StatusCode};
use axum::response::IntoResponse;
use tower::ServiceExt;
use tower_http::services::ServeDir;

const FALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>AgentWorth - Your agents left receipts</title>
  <style>
    :root {
      --bg: #09090b;
      --card-bg: #121215;
      --card-border: #27272a;
      --text: #f4f4f5;
      --text-muted: #a1a1aa;
      --cyan: #06b6d4;
      --green: #22c55e;
      --yellow: #eab308;
      --red: #ef4444;
      --magenta: #d946ef;
      --mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      background: var(--bg);
      color: var(--text);
      font-family: var(--mono);
      padding: 24px;
      line-height: 1.5;
    }
    header {
      border-bottom: 1px solid var(--card-border);
      padding-bottom: 16px;
      margin-bottom: 24px;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    h1 { font-size: 1.5rem; color: var(--cyan); }
    .tagline { color: var(--text-muted); font-size: 0.9rem; margin-top: 4px; }
    .btn {
      background: #18181b;
      color: var(--text);
      border: 1px solid var(--card-border);
      padding: 8px 16px;
      border-radius: 6px;
      cursor: pointer;
      font-family: var(--mono);
      font-size: 0.85rem;
    }
    .btn:hover { background: #27272a; border-color: var(--cyan); }
    .btn-primary { background: var(--cyan); color: #000; font-weight: bold; border: none; }
    .btn-primary:hover { background: #22d3ee; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 16px; margin-bottom: 24px; }
    .card {
      background: var(--card-bg);
      border: 1px solid var(--card-border);
      border-radius: 8px;
      padding: 16px;
    }
    .card-title { color: var(--text-muted); font-size: 0.8rem; text-transform: uppercase; margin-bottom: 8px; }
    .card-value { font-size: 1.5rem; font-weight: bold; }
    .search-bar {
      display: flex;
      gap: 12px;
      margin-bottom: 16px;
    }
    input, select {
      background: var(--card-bg);
      border: 1px solid var(--card-border);
      color: var(--text);
      padding: 8px 12px;
      border-radius: 6px;
      font-family: var(--mono);
      font-size: 0.9rem;
    }
    input { flex: 1; }
    table { width: 100%; border-collapse: collapse; margin-top: 8px; font-size: 0.85rem; }
    th { text-align: left; padding: 10px; border-bottom: 1px solid var(--card-border); color: var(--cyan); }
    td { padding: 10px; border-bottom: 1px solid #18181b; }
    tr:hover td { background: #18181b; }
    .badge {
      display: inline-block;
      padding: 2px 6px;
      border-radius: 4px;
      font-size: 0.75rem;
      background: #27272a;
    }
    .badge-green { background: #052e16; color: var(--green); }
    .badge-cyan { background: #083344; color: var(--cyan); }
    .badge-yellow { background: #422006; color: var(--yellow); }
    .badge-red { background: #450a0a; color: var(--red); }
    .archaeology-box {
      border: 1px dashed var(--yellow);
      background: rgba(234, 179, 8, 0.05);
      border-radius: 8px;
      padding: 16px;
      margin-bottom: 24px;
    }
    .arch-title { color: var(--yellow); font-size: 1.1rem; margin-bottom: 8px; }
    #modal {
      display: none;
      position: fixed;
      inset: 0;
      background: rgba(0, 0, 0, 0.8);
      padding: 24px;
      overflow-y: auto;
      z-index: 100;
    }
    .modal-content {
      background: var(--card-bg);
      border: 1px solid var(--card-border);
      max-width: 900px;
      margin: 0 auto;
      border-radius: 8px;
      padding: 24px;
    }
    .timeline-event {
      border-left: 2px solid var(--card-border);
      padding-left: 16px;
      margin-left: 8px;
      margin-bottom: 16px;
    }
  </style>
</head>
<body>
  <header>
    <div>
      <h1>AGENTWORTH</h1>
      <div class="tagline">Your agents left receipts. Discover and normalize AI histories locally.</div>
    </div>
    <button class="btn btn-primary" onclick="triggerScan()">⚡ Rescan Agents</button>
  </header>

  <div id="stats-grid" class="grid">
    <div class="card"><div class="card-title">Total Sessions</div><div id="stat-sessions" class="card-value">-</div></div>
    <div class="card"><div class="card-title">Total Tokens</div><div id="stat-tokens" class="card-value" style="color: var(--magenta)">-</div></div>
    <div class="card"><div class="card-title">Total Events</div><div id="stat-events" class="card-value">-</div></div>
    <div class="card"><div class="card-title">Primary Adapter</div><div id="stat-adapter" class="card-value" style="color: var(--green)">-</div></div>
  </div>

  <div id="archaeology-container" class="archaeology-box">
    <div class="arch-title">🏛️ AGENT ARCHAEOLOGY HIGHLIGHTS</div>
    <div id="arch-content" style="font-size: 0.9rem;">Loading discoveries...</div>
  </div>

  <div class="card">
    <div class="search-bar">
      <input type="text" id="search-input" placeholder="Search sessions, prompts, repositories..." onkeyup="handleSearch(event)">
      <select id="adapter-select" onchange="loadTraces()">
        <option value="">All Adapters</option>
        <option value="claude_code">Claude Code</option>
        <option value="codex">Codex</option>
        <option value="gemini">Gemini</option>
        <option value="opencode">OpenCode</option>
      </select>
      <select id="order-select" onchange="loadTraces()">
        <option value="started_at_desc">Newest First</option>
        <option value="started_at_asc">Oldest First</option>
        <option value="tokens_desc">Most Tokens</option>
        <option value="duration_desc">Longest Duration</option>
      </select>
    </div>

    <table id="traces-table">
      <thead>
        <tr>
          <th>SESSION ID</th>
          <th>ADAPTER</th>
          <th>STARTED</th>
          <th>DURATION</th>
          <th>TOKENS</th>
          <th>MODELS</th>
          <th>ACTIONS</th>
        </tr>
      </thead>
      <tbody id="traces-body">
        <tr><td colspan="7">Loading sessions...</td></tr>
      </tbody>
    </table>
  </div>

  <div id="modal" onclick="closeModal(event)">
    <div class="modal-content" onclick="event.stopPropagation()">
      <div style="display:flex; justify-content:space-between; margin-bottom:16px;">
        <h2 id="modal-title" style="color:var(--cyan); font-size:1.2rem;">Trace Details</h2>
        <button class="btn" onclick="closeModal()">Close ✕</button>
      </div>
      <div id="modal-body">Loading...</div>
    </div>
  </div>

  <script>
    async function loadStats() {
      try {
        const res = await fetch('/api/stats');
        const data = await res.json();
        document.getElementById('stat-sessions').innerText = data.total_sessions || 0;
        document.getElementById('stat-tokens').innerText = (data.token_usage?.total_tokens || 0).toLocaleString();
        document.getElementById('stat-events').innerText = (data.total_events || 0).toLocaleString();
        const adapters = Object.keys(data.sessions_by_adapter || {});
        document.getElementById('stat-adapter').innerText = adapters[0] || 'none';
      } catch (e) {
        console.error(e);
      }
    }

    async function loadArchaeology() {
      try {
        const res = await fetch('/api/archaeology');
        const data = await res.json();
        let html = '';
        if (data.most_expensive_unsolved) {
          const u = data.most_expensive_unsolved;
          html += `<div><strong>Most Expensive Unsolved:</strong> "${u.prompt}" &mdash; <span style="color:var(--yellow);">${(u.total_tokens || 0).toLocaleString()} tokens</span> (${u.adapter})</div>`;
        }
        if (data.longest_recovery_loop) {
          const r = data.longest_recovery_loop;
          html += `<div style="margin-top:6px;"><strong>Longest Autonomous Recovery Loop:</strong> ${r.steps_to_recover} steps (${r.duration_seconds ? r.duration_seconds.toFixed(1) + 's' : 'unknown duration'}) [${r.adapter}]</div>`;
        }
        if (data.token_carbon_dating) {
          const c = data.token_carbon_dating;
          html += `<div style="margin-top:6px; color:var(--text-muted);">Carbon Dating: Active across ${c.total_days_active} days &bull; Avg ${c.average_tokens_per_session.toLocaleString()} tokens/session</div>`;
        }
        document.getElementById('arch-content').innerHTML = html || 'No archaeology highlights found yet.';
      } catch (e) {
        document.getElementById('arch-content').innerText = 'Archaeology engine ready.';
      }
    }

    async function loadTraces() {
      const search = document.getElementById('search-input').value;
      const adapter = document.getElementById('adapter-select').value;
      const orderBy = document.getElementById('order-select').value;

      const params = new URLSearchParams();
      if (search) params.set('search', search);
      if (adapter) params.set('adapter', adapter);
      if (orderBy) params.set('order_by', orderBy);

      const res = await fetch('/api/traces?' + params.toString());
      const traces = await res.json();
      const tbody = document.getElementById('traces-body');

      if (!traces || traces.length === 0) {
        tbody.innerHTML = '<tr><td colspan="7" style="color:var(--text-muted);">No traces match the filter.</td></tr>';
        return;
      }

      tbody.innerHTML = traces.map(t => `
        <tr>
          <td><code style="color:var(--cyan); cursor:pointer;" onclick="inspectTrace('${t.session_id}')">${t.session_id.substring(0, 16)}...</code></td>
          <td><span class="badge badge-cyan">${t.adapter}</span></td>
          <td style="color:var(--text-muted);">${new Date(t.started_at).toLocaleString()}</td>
          <td>${t.duration_seconds ? (t.duration_seconds >= 60 ? (t.duration_seconds/60).toFixed(1) + 'm' : t.duration_seconds.toFixed(0) + 's') : '-'}</td>
          <td style="color:var(--magenta);">${t.total_tokens.toLocaleString()}</td>
          <td style="color:var(--text-muted);">${(t.models_used || []).join(', ') || '-'}</td>
          <td>
            <button class="btn" style="padding:4px 8px; font-size:0.75rem;" onclick="inspectTrace('${t.session_id}')">Inspect</button>
            <button class="btn" style="padding:4px 8px; font-size:0.75rem;" onclick="exportTrace('${t.session_id}')">Export</button>
          </td>
        </tr>
      `).join('');
    }

    async function inspectTrace(id) {
      document.getElementById('modal').style.display = 'block';
      document.getElementById('modal-title').innerText = 'Trace: ' + id;
      document.getElementById('modal-body').innerHTML = 'Loading full trace timeline...';

      try {
        const res = await fetch('/api/traces/' + id);
        const data = await res.json();
        const score = data.score;
        const trace = data.trace;

        let html = `
          <div style="margin-bottom:16px; padding:12px; background:#18181b; border-radius:6px;">
            <div style="display:flex; justify-content:space-between; margin-bottom:8px;">
              <div><strong>Adapter:</strong> ${trace.adapter} &bull; <strong>Tokens:</strong> ${trace.stats.token_usage.total.toLocaleString()}</div>
              <div><strong style="color:var(--yellow);">Score: ${(score.composite_score * 100).toFixed(1)} / 100</strong></div>
            </div>
            <div style="font-size:0.8rem; color:var(--text-muted);">
              Outcome: ${(score.outcome_score*100).toFixed(0)}% &bull;
              Verifiability: ${(score.verifiability_score*100).toFixed(0)}% &bull;
              Recovery: ${(score.recovery_score*100).toFixed(0)}%
            </div>
          </div>
          <h3 style="margin-bottom:12px; font-size:1rem; color:var(--cyan);">Events Timeline (${trace.events.length} events)</h3>
        `;

        html += trace.events.map(ev => {
          const type = Object.keys(ev.payload)[0];
          return `
            <div class="timeline-event">
              <div style="font-size:0.75rem; color:var(--text-muted);">[#${ev.sequence}] ${new Date(ev.timestamp).toLocaleTimeString()} &bull; <span style="color:var(--cyan);">${type}</span></div>
              <pre style="margin-top:4px; font-size:0.8rem; white-space:pre-wrap; max-height:200px; overflow-y:auto; color:var(--text);">${JSON.stringify(ev.payload[type], null, 2)}</pre>
            </div>
          `;
        }).join('');

        document.getElementById('modal-body').innerHTML = html;
      } catch (e) {
        document.getElementById('modal-body').innerHTML = '<div style="color:var(--red);">Failed to load trace details.</div>';
      }
    }

    async function exportTrace(id) {
      const format = prompt('Export format: "json" or "atif"?', 'json') || 'json';
      const redact = confirm('Redact secrets, API keys, and credentials?');
      const res = await fetch('/api/export/' + id, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ format, redact })
      });
      const data = await res.json();
      const blob = new Blob([data.content], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `agentworth-${id}.${format === 'atif' ? 'atif.json' : 'json'}`;
      a.click();
    }

    async function triggerScan() {
      const btn = event.target;
      btn.innerText = 'Scanning...';
      btn.disabled = true;
      try {
        const res = await fetch('/api/scan', { method: 'POST' });
        const data = await res.json();
        alert(`Scan finished: ${data.scanned_sessions} scanned, ${data.skipped_unchanged} skipped, ${data.total_indexed_sessions} total in index.`);
        loadStats();
        loadArchaeology();
        loadTraces();
      } catch (e) {
        alert('Scan failed: ' + e);
      } finally {
        btn.innerText = '⚡ Rescan Agents';
        btn.disabled = false;
      }
    }

    function handleSearch(e) {
      if (e.key === 'Enter') loadTraces();
    }

    function closeModal() {
      document.getElementById('modal').style.display = 'none';
    }

    loadStats();
    loadArchaeology();
    loadTraces();
  </script>
</body>
</html>"#;

/// Fallback handler when serving SPA static files or embedded fallback.
pub async fn serve_static_or_spa(
    dist_dir: Option<PathBuf>,
    req: Request<Body>,
) -> impl IntoResponse {
    // If custom or standard dist_dir exists, attempt to serve from disk
    if let Some(ref dist) = dist_dir {
        if dist.exists() && dist.is_dir() {
            let path_uri = req.uri().path().trim_start_matches('/');
            let candidate_file = dist.join(path_uri);

            // 1. If exact file exists and is not a directory, serve it
            if !path_uri.is_empty() && candidate_file.exists() && candidate_file.is_file() {
                let serve_dir = ServeDir::new(dist);
                let res = serve_dir.oneshot(req).await.into_response();
                if res.status() != StatusCode::NOT_FOUND {
                    return res;
                }
            }

            // 2. Otherwise serve index.html as SPA history fallback
            let index_html = dist.join("index.html");
            if index_html.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&index_html).await {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                        .body(Body::from(content))
                        .unwrap_or_else(|_| {
                            Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::empty())
                                .unwrap()
                        });
                }
            }
        }
    }

    // Default: Return embedded fallback SPA HTML
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )
        .body(Body::from(FALLBACK_HTML))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}
