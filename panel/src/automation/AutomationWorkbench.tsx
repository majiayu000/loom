import { useState } from "react";
import { actions, defaults, requestFor } from "./actions";
import "./automation.css";

interface Result { ok: boolean; data?: Record<string, unknown>; error?: { message?: string; code?: string }; }
interface Props {
  execute: (request: Record<string, unknown>, root?: string) => Promise<Result>;
  readOnly?: boolean;
  desktop?: boolean;
  unavailable?: boolean;
  initialAction?: string;
}

export function AutomationWorkbench({ execute, readOnly = false, desktop = false, unavailable = false, initialAction = "author_rewrite" }: Props) {
  const initial = actions.find(a => a.id === initialAction) ?? actions[0];
  const [actionId, setActionId] = useState(initial.id);
  const action = actions.find(a => a.id === actionId) ?? actions[0];
  const [values, setValues] = useState(() => defaults(initial));
  const [root, setRoot] = useState("");
  const [busy, setBusy] = useState(false);
  const [reviewed, setReviewed] = useState(false);
  const [previewed, setPreviewed] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState("");
  const [lastPreview, setLastPreview] = useState(false);
  const groups = [...new Set(actions.map(a => a.group))];

  function resetReview() { setReviewed(false); setPreviewed(false); setResult(null); setError(""); }
  function choose(id: string) {
    const next = actions.find(a => a.id === id);
    if (!next) return;
    setActionId(id); setValues(defaults(next)); resetReview();
  }
  async function run(preview: boolean) {
    if (busy || readOnly || unavailable || (!preview && action.review && (!reviewed || (action.preview && !previewed)))) return;
    setBusy(true); setError(""); setResult(null); setLastPreview(preview);
    try {
      const response = await execute(requestFor(action, values, preview), root || undefined);
      setResult(response);
      if (!response.ok) { setError(response.error?.message ?? "操作失败，请检查返回结果。"); setPreviewed(false); }
      else if (preview) setPreviewed(true);
      if (!preview) setReviewed(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPreviewed(false);
    } finally { setBusy(false); }
  }

  return <section className="automation-workbench" aria-labelledby="automation-heading">
    <header className="automation-heading"><div><span className="automation-kicker">LOOM / WORKBENCH</span><h1 id="automation-heading">技能工作台</h1><p>把想法写成技能，验证表现，再带到下一个工作环境。</p></div><span className="automation-local">本机执行</span></header>
    {unavailable ? <p role="status">请在桌面 App 中打开本机工作台，或使用本机 Loom Panel。</p> : <>
      {readOnly && <p role="status">当前 Registry 不可写。请先恢复连接或完成初始化。</p>}
      <div className="automation-layout">
        <nav aria-label="工作台操作" className="automation-nav">{groups.map(group => <div key={group}><h2>{group}</h2>{actions.filter(a => a.group === group).map(a => <button key={a.id} type="button" disabled={busy} aria-current={a.id === actionId ? "page" : undefined} onClick={() => choose(a.id)}>{a.label}<span aria-hidden="true">↗</span></button>)}</div>)}</nav>
        <div className="automation-main">
          <form className="automation-card" onSubmit={event => { event.preventDefault(); void run(!!action.preview); }}>
            <div className="automation-card-heading"><span className="automation-kicker">{action.group}</span><h2>{action.label}</h2><p>{action.description}</p></div>
            <fieldset disabled={busy || readOnly} className="automation-fields">
              {desktop && <label>Registry 目录<input value={root} onChange={e => { setRoot(e.target.value); resetReview(); }} placeholder="留空使用默认仓库" /></label>}
              {action.fields.map(field => <div className="automation-field" key={field.key}><label htmlFor={`${action.id}-${field.key}`}>{field.label}</label>{field.optional && <span className="automation-optional">可选</span>}{field.options ? <select id={`${action.id}-${field.key}`} value={values[field.key] ?? ""} onChange={e => { setValues({ ...values, [field.key]: e.target.value }); resetReview(); }}>{field.options.map(option => <option key={option}>{option}</option>)}</select> : field.multiline ? <textarea id={`${action.id}-${field.key}`} rows={4} required={!field.optional} value={values[field.key] ?? ""} onChange={e => { setValues({ ...values, [field.key]: e.target.value }); resetReview(); }} /> : <input id={`${action.id}-${field.key}`} required={!field.optional} value={values[field.key] ?? ""} onChange={e => { setValues({ ...values, [field.key]: e.target.value }); resetReview(); }} />}{field.hint && <small>{field.hint}</small>}</div>)}
              <p className="automation-effect">{action.effect}</p>
              {action.review && <label className="automation-review"><input type="checkbox" checked={reviewed} disabled={!!action.preview && !previewed} onChange={e => setReviewed(e.target.checked)} /><span>我已审阅{action.preview ? "上方参数和预览结果" : "相关计划或补丁"}，确认本次操作。</span></label>}
              <div className="automation-buttons">
                {action.preview ? <><button type="submit" className="automation-secondary">{busy ? "处理中…" : "预览"}</button><button type="button" disabled={!previewed || !reviewed} onClick={() => void run(false)}>确认执行</button></> : <button type="submit" disabled={!!action.review && !reviewed}>{busy ? "处理中…" : action.label}</button>}
              </div>
            </fieldset>
          </form>
          <section className="automation-card automation-result" aria-label="操作结果" aria-busy={busy}>
            <div className="automation-result-title"><h2>{lastPreview ? "预览结果" : "操作结果"}</h2><span>{busy ? "进行中" : error ? "未完成" : result?.ok ? "已完成" : "等待操作"}</span></div>
            {busy && <p role="status">正在处理。模型调用和远程拉取可能需要一些时间。</p>}
            {error && <p role="alert" className="automation-error">{error}</p>}
            {result ? <textarea readOnly aria-label="完整操作报告（只读）" value={JSON.stringify(result.data ?? result, null, 2)} /> : !busy && !error && <p className="automation-empty">运行后会显示实际报告、计划和产物位置。</p>}
          </section>
        </div>
      </div>
    </>}
  </section>;
}
