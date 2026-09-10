import { useEffect, useRef, useState } from "react";
import type { SkillInspectPayload, SkillDiagnosePayload } from "../lib/api/client";
import { SkillInspectSections } from "../pages/panel/SkillDetailPage";
import { SkillDiagnosePanel } from "../pages/panel/SkillDiagnosePanel";
import { native, requireSuccess, type Envelope } from "./client";
import "./local-tools.css";

interface Revision {
  commit: string;
  short_commit: string;
  message: string;
  committed_at: string;
}

export function LocalDetail({ root, skill, inspection }: {
  root: string;
  skill: string;
  inspection: Record<string, unknown> | null;
}) {
  const [tab, setTab] = useState("inspect");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [diagnose, setDiagnose] = useState<SkillDiagnosePayload | null>(null);
  const [revisions, setRevisions] = useState<Revision[] | null>(null);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [diff, setDiff] = useState<string | null>(null);
  const generation = useRef(0);
  useEffect(() => () => { generation.current++; }, []);

  const read = async (action: () => Promise<void>) => {
    const current = generation.current;
    setBusy(true);
    setError(null);
    try { await action(); }
    catch (err) { if (current === generation.current) setError(String(err instanceof Error ? err.message : err)); }
    finally { if (current === generation.current) setBusy(false); }
  };
  const load = async (command: string, extra: Record<string, unknown> = {}) => {
    const current = generation.current;
    const data = requireSuccess(await native<Envelope>(command, { root: root || null, skill, ...extra }));
    if (current !== generation.current) throw new Error("已切换技能");
    return data;
  };

  return <section className="local-tools" aria-label="本机技能管理">
    <p className="subtle">这些是所选本机仓库的文件与配置证据，不代表团队成员的设备状态；实际使用前仍需检查 Agent 新会话。</p>
    <nav className="local-tabs" aria-label="本机技能详情">
      {[["inspect", "详情与安装位置"], ["diagnose", "诊断"], ["versions", "历史与差异"]].map(([key, label]) =>
        <button type="button" key={key} aria-pressed={tab === key} onClick={() => { generation.current++; setBusy(false); setTab(key); setError(null); }}>{label}</button>)}
    </nav>
    {error && <p role="alert">{error}</p>}
    {tab === "inspect" && (inspection
      ? <SkillInspectSections inspect={inspection as unknown as SkillInspectPayload} />
      : <p role="status">尚未取得技能详情；若读取失败，请返回列表重试。</p>)}
    {tab === "diagnose" && <>
      <button type="button" disabled={busy} onClick={() => void read(async () => {
        setDiagnose(null);
        setDiagnose(await load("diagnose_skill") as unknown as SkillDiagnosePayload);
      })}>{busy ? "正在检查…" : "检查本机技能"}</button>
      <SkillDiagnosePanel loading={busy} error={null} diagnose={diagnose} />
    </>}
    {tab === "versions" && <>
      <p>比较本机 Git 修订，不改变团队推荐版本，也不会写入本机文件。这里显示最近 30 条修订。</p>
      <button type="button" disabled={busy} onClick={() => void read(async () => {
        setDiff(null);
        setRevisions(null);
        const data = await load("history_skill");
        const items = data.items as Revision[];
        setRevisions(items);
        setFrom(items[1]?.commit ?? items[0]?.commit ?? "");
        setTo(items[0]?.commit ?? "");
      })}>{busy ? "正在读取…" : "读取本机历史"}</button>
      {revisions && !revisions.length && <p>当前技能尚无已提交的修订。</p>}
      {revisions && revisions.length > 0 && <>
        <div className="revision-selectors">
          <label>旧修订<select value={from} disabled={busy} onChange={e => { setFrom(e.target.value); setDiff(null); }}>
            {revisions.map(r => <option key={r.commit} value={r.commit}>{r.short_commit} · {r.message}</option>)}
          </select></label>
          <label>新修订<select value={to} disabled={busy} onChange={e => { setTo(e.target.value); setDiff(null); }}>
            {revisions.map(r => <option key={r.commit} value={r.commit}>{r.short_commit} · {r.message}</option>)}
          </select></label>
        </div>
        <button type="button" disabled={busy || !from || !to || from === to} onClick={() => void read(async () => {
          setDiff(null);
          const data = await load("diff_skill", { from, to });
          if (typeof data.diff !== "string") throw new Error("引擎响应缺少版本差异");
          setDiff(data.diff);
        })}>查看差异</button>
        <ol className="revision-list">{revisions.map(r => <li key={r.commit}><code>{r.short_commit}</code> {r.message} <small>{r.committed_at}</small></li>)}</ol>
      </>}
      {diff !== null && <section aria-label="本机版本差异"><pre className="local-diff">{diff || "这两个修订中的技能内容相同。"}</pre></section>}
    </>}
  </section>;
}
