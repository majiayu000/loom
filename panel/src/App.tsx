import { useEffect, useState } from "react";

type Remote = {
  configured?: boolean;
  url?: string;
  ahead?: number;
  behind?: number;
  pending_ops?: number;
  sync_state?: string;
};

export function App() {
  const [skills, setSkills] = useState<string[]>([]);
  const [remote, setRemote] = useState<Remote>({});

  async function refresh() {
    const [skillsRes, remoteRes] = await Promise.all([
      fetch("/api/skills").then((r) => r.json()),
      fetch("/api/remote/status").then((r) => r.json()),
    ]);
    setSkills(skillsRes.skills ?? []);
    setRemote(remoteRes.remote ?? {});
  }

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <main style={{ fontFamily: "ui-sans-serif, system-ui", margin: "2rem auto", maxWidth: 960 }}>
      <h1 style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <img src="/favicon.svg" alt="Loom icon" width={36} height={36} />
        <span>Loom Panel</span>
      </h1>
      <button onClick={() => void refresh()}>Refresh</button>
      <section>
        <h2>Skills</h2>
        <ul>
          {skills.length === 0 ? <li>No skills</li> : skills.map((s) => <li key={s}>{s}</li>)}
        </ul>
      </section>
      <section>
        <h2>Remote</h2>
        <pre>{JSON.stringify(remote, null, 2)}</pre>
      </section>
    </main>
  );
}
