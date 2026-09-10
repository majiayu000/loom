import { useCallback, useState } from "react";
import { SkillMAuditHistory } from "../pages/SkillMAuditHistory";
import type { OpsPayload } from "../lib/api/client";
import { isDesktop, native } from "./client";
import "./local-tools.css";

export function LocalActivity() {
  const [root, setRoot] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const load = useCallback(async (options?: { limit?: number; offset?: number }, signal?: AbortSignal) => {
    signal?.throwIfAborted();
    const result = await native<OpsPayload>("local_operations", { root: selected || null, offset: options?.offset ?? 0 });
    signal?.throwIfAborted();
    return result;
  }, [selected]);
  return <section className="team-content local-tools">
    <span className="eyebrow">ON THIS DEVICE</span><h1>本机活动记录</h1>
    <p className="lead">追踪这台设备上的安装、更新和恢复。这里不代表整个团队的活动。</p>
    {!isDesktop() ? <p>请在桌面 App 中查看本机记录。团队版本的发布信息仍在技能详情中查看。</p> : <>
      <form className="team-card" onSubmit={e => { e.preventDefault(); setSelected(root); setRefresh(v => v + 1); }}>
        <label>Registry 目录（留空使用默认）<input value={root} onChange={e => { setRoot(e.target.value); setSelected(null); }} /></label>
        <button type="submit">读取本机记录</button>
      </form>
      {selected !== null && <SkillMAuditHistory key={selected} live refreshKey={String(refresh)} loadOperations={load} sourceLabel="本机记录 · 只读，不上传团队" />}
    </>}
  </section>;
}
