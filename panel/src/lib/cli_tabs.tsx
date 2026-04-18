import type { ReactNode } from "react";

export type CliTabKey = "quickstart" | "workspace" | "lifecycle" | "sync" | "ops";

export const CLI_TAB_ORDER: { key: CliTabKey; label: string }[] = [
  { key: "quickstart", label: "quickstart" },
  { key: "workspace", label: "workspace" },
  { key: "lifecycle", label: "lifecycle" },
  { key: "sync", label: "sync & replay" },
  { key: "ops", label: "ops / diagnose" },
];

const Prompt = () => <span className="prompt">$</span>;
const C = ({ children }: { children: ReactNode }) => <span className="comment">{children}</span>;
const F = ({ children }: { children: ReactNode }) => <span className="flag">{children}</span>;
const S = ({ children }: { children: ReactNode }) => <span className="str">{children}</span>;
const OK = ({ children }: { children: ReactNode }) => <span className="ok">{children}</span>;
const W = ({ children }: { children: ReactNode }) => <span className="warn">{children}</span>;
const E = ({ children }: { children: ReactNode }) => <span className="emph">{children}</span>;
const Add = ({ children }: { children: ReactNode }) => <span className="add">{children}</span>;
const Del = ({ children }: { children: ReactNode }) => <span className="del">{children}</span>;

const Quickstart = () => (
  <>
    <C># 1. Set up a registry repo (separate from Loom tool repo)</C>
    {"\n"}
    <Prompt /> mkdir -p ~/.loom-registry && cd ~/.loom-registry && git init{"\n"}
    Initialized empty Git repository in <S>~/.loom-registry/.git/</S>
    {"\n\n"}
    <C># 2. Tell Loom about your Claude skills directory</C>
    {"\n"}
    <Prompt /> loom <F>--root</F> ~/.loom-registry target add \{"\n"}
    {"    "}
    <F>--agent</F> claude <F>--path</F> <S>"$HOME/.claude/skills"</S> <F>--ownership</F> observed
    {"\n"}
    <OK>✓</OK> target added <E>claude/home</E> → ~/.claude/skills <C>(observed · 18 skills present)</C>
    {"\n\n"}
    <C># 3. Add a second profile for work</C>
    {"\n"}
    <Prompt /> loom target add <F>--agent</F> claude <F>--profile</F> work \{"\n"}
    {"    "}
    <F>--path</F> <S>"$HOME/.claude-work/skills"</S> <F>--ownership</F> managed
    {"\n"}
    <OK>✓</OK> target added <E>claude/work</E> → ~/.claude-work/skills <C>(managed · 24 skills present)</C>
    {"\n\n"}
    <C># 4. Open the visual panel</C>
    {"\n"}
    <Prompt /> loom panel{"\n"}
    <OK>➜</OK> panel listening on <E>http://localhost:43117</E>
  </>
);

const Workspace = () => (
  <>
    <C># Inspect current workspace</C>
    {"\n"}
    <Prompt /> loom workspace status <F>--json</F> | jq '.targets | length'{"\n"}
    6{"\n\n"}
    <C># Doctor: inspect drift + projection health</C>
    {"\n"}
    <Prompt /> loom workspace doctor{"\n"}
    <OK>✓</OK> 47 projections clean{"\n"}
    <W>⚠</W> 2 drifted · <E>refactor-patterns</E> → claude/home, codex/home{"\n"}
    <OK>➜</OK> run <E>loom workspace repair</E> or <E>loom skill project</E> to realign
    {"\n\n"}
    <C># Register a binding</C>
    {"\n"}
    <Prompt /> loom workspace binding add \{"\n"}
    {"  "}
    <F>--agent</F> claude <F>--profile</F> work \{"\n"}
    {"  "}
    <F>--matcher-kind</F> path-prefix <F>--matcher-value</F> <S>"/Users/me/work"</S> \{"\n"}
    {"  "}
    <F>--target</F> t-claude-work{"\n"}
    <OK>✓</OK> binding <E>b-06</E> registered · policy-profile=auto
  </>
);

const Lifecycle = () => (
  <>
    <C># Capture a new version of an existing skill</C>
    {"\n"}
    <Prompt /> loom skill capture refactor-patterns{"\n"}
    <OK>✓</OK> captured <E>refactor-patterns#c7</E> <C>(3 files · 847 bytes · from target claude/home)</C>
    {"\n\n"}
    <C># Diff the capture against the last release</C>
    {"\n"}
    <Prompt /> loom skill diff refactor-patterns <F>--from</F> v0.4 <F>--to</F> c7{"\n"}
    <W>~</W> SKILL.md{"\n"}
    {"  "}
    <Del>- for simple function extractions only</Del>
    {"\n"}
    {"  "}
    <Add>+ for both function + module-level refactors</Add>
    {"\n"}
    {"  "}
    <Add>+ pairs well with rust-test-harness</Add>
    {"\n\n"}
    <C># Snapshot + release as v0.4.2</C>
    {"\n"}
    <Prompt /> loom skill snapshot refactor-patterns <F>--tag</F> pre-v0.4.2{"\n"}
    <OK>✓</OK> snapshot sn-8f1a2c{"\n\n"}
    <Prompt /> loom skill release refactor-patterns <F>--version</F> v0.4.2{"\n"}
    <OK>✓</OK> released <E>refactor-patterns@v0.4.2</E>
    {"\n"}
    <OK>➜</OK> auto-projected to 4 targets <C>(3 symlink · 1 materialize)</C>
    {"\n\n"}
    <C># Something broke? roll back</C>
    {"\n"}
    <Prompt /> loom skill rollback refactor-patterns <F>--to</F> v0.4.1{"\n"}
    <OK>✓</OK> rolled back · replayed 4 projections
  </>
);

const Sync = () => (
  <>
    <C># Push the registry to your team remote</C>
    {"\n"}
    <Prompt /> loom sync push{"\n"}
    <OK>✓</OK> pushed 7 commits · <E>2a3f8c1..e91d4b2</E> → origin/main{"\n\n"}
    <C># Pull changes from a teammate</C>
    {"\n"}
    <Prompt /> loom sync pull{"\n"}
    <OK>✓</OK> fetched 3 commits from origin/main{"\n"}
    <W>⚠</W> 2 projections need replay <C>(skill state changed)</C>
    {"\n\n"}
    <C># Replay pending ops against your local targets</C>
    {"\n"}
    <Prompt /> loom sync replay{"\n"}
    <OK>✓</OK> replayed op-4a21 <E>commit-message-writer@v1.2.0</E> → claude/home{"\n"}
    <OK>✓</OK> replayed op-4a22 <E>typed-api-client@v0.5.0</E> {"  "}→ cursor/home
    {"\n\n"}
    <C># Inspect ops history</C>
    {"\n"}
    <Prompt /> loom ops history <F>--last</F> 24h <F>--json</F> | jq '.failed'{"\n"}
    []
  </>
);

const Ops = () => (
  <>
    <C># List recent operations</C>
    {"\n"}
    <Prompt /> loom ops list <F>--last</F> 24h{"\n"}
    op-4a21 <W>pending</W> project refactor-patterns@v0.4.2 → claude/work{"\n"}
    op-4a1e <span style={{ color: "var(--err)" }}>failed</span> project sql-schema-audit@v0.3.0 → windsurf/home{"\n"}
    {"        "}
    <C>reason: target ownership=external; refusing to write</C>
    {"\n\n"}
    <C># Retry pending projections</C>
    {"\n"}
    <Prompt /> loom ops retry <F>--kind</F> project{"\n"}
    <OK>✓</OK> 1 retried, 0 failed{"\n\n"}
    <C># Diagnose + auto-repair drifted projections</C>
    {"\n"}
    <Prompt /> loom ops history diagnose{"\n"}
    <OK>✓</OK> 45 ok · 1 failed · 1 pending{"\n"}
    {"  "}
    drift: <E>refactor-patterns</E> → claude/home <C>(missing symlink target)</C>
    {"\n\n"}
    <Prompt /> loom ops history repair <F>--strategy</F> local{"\n"}
    <OK>✓</OK> repaired 1 projection from local skill store
  </>
);

export const CLI_TAB_CONTENT: Record<CliTabKey, ReactNode> = {
  quickstart: <Quickstart />,
  workspace: <Workspace />,
  lifecycle: <Lifecycle />,
  sync: <Sync />,
  ops: <Ops />,
};
