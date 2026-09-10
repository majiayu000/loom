import { useState } from "react";

export interface LocalSkill {
  skill_id: string;
  description?: string;
  source_path?: string;
  source_status?: string;
  trust?: string;
  warnings?: string[];
}

const group = (skill: LocalSkill) => skill.source_status === "present" ? "present" : skill.source_status === "missing" ? "missing" : "attention";
const labels = { present: "已导入", missing: "尚未导入当前仓库", attention: "需要检查" };
const pageSize = 20;

export function LocalSkillList({ skills, busy, onSelect }: {
  skills: LocalSkill[];
  busy: boolean;
  onSelect: (skill: LocalSkill) => void;
}) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [page, setPage] = useState(0);
  const needle = query.trim().toLocaleLowerCase();
  const filtered = skills.filter(s => (filter === "all" || group(s) === filter)
    && `${s.skill_id} ${s.description ?? ""} ${s.source_path ?? ""}`.toLocaleLowerCase().includes(needle));
  const pages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const current = Math.min(page, pages - 1);
  return <section className="local-library" aria-label="本机技能列表">
    <div className="local-library-toolbar">
      <label className="local-search"><span>搜索技能</span><input type="search" placeholder="搜索名称、描述或路径…" value={query} onChange={e => { setQuery(e.target.value); setPage(0); }} /></label>
      <span className="local-total">{skills.length} 个条目</span>
    </div>
    <nav className="local-filters" aria-label="技能状态筛选">
      {[["all", "全部"], ["present", "已导入"], ["missing", "未导入"], ["attention", "需检查"]].map(([key, label]) =>
        <button type="button" key={key} aria-pressed={filter === key} onClick={() => { setFilter(key); setPage(0); }}>{label}<span>{key === "all" ? skills.length : skills.filter(s => group(s) === key).length}</span></button>)}
    </nav>
    <div className="local-list-heading" aria-hidden="true"><span>技能 / 描述</span><span>仓库状态</span><span /></div>
    <div className="local-skill-rows">
      {filtered.slice(current * pageSize, (current + 1) * pageSize).map(s => <button type="button" className="local-skill-row" disabled={busy} key={s.skill_id} onClick={() => onSelect(s)}>
        <span className="local-skill-copy"><strong>{s.skill_id}</strong><span>{s.description || (group(s) === "missing" ? "在 Agent 中发现，导入后可由 Loom 管理。" : "暂无描述，打开详情查看文件与诊断。")}</span></span>
        <span className={`local-status local-status-${group(s)}`} title={s.source_status}>{labels[group(s)]}</span>
        <span className="local-row-arrow" aria-hidden="true">↗</span>
      </button>)}
    </div>
    {!filtered.length && <div className="empty-state"><h2>{skills.length ? "没有匹配的技能" : "仓库中还没有技能"}</h2><p>{skills.length ? "换个关键词或状态试试。" : "展开“导入已有技能目录”，添加你的第一个技能。"}</p></div>}
    <footer className="local-pagination"><span role="status">{filtered.length ? `${current * pageSize + 1}–${Math.min((current + 1) * pageSize, filtered.length)}` : "0"} / {filtered.length} 个条目</span><div><button type="button" disabled={current === 0} onClick={() => setPage(current - 1)}>上一页</button><span>{current + 1} / {pages}</span><button type="button" disabled={current === pages - 1} onClick={() => setPage(current + 1)}>下一页</button></div></footer>
  </section>;
}
