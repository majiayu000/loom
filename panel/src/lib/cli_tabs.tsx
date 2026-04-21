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
    <Prompt /> export LOOM_REGISTRY=/path/to/loom-registry{"\n"}
    <Prompt /> mkdir -p "$LOOM_REGISTRY" && cd "$LOOM_REGISTRY" && git init{"\n"}
    Initialized empty Git repository in <S>$LOOM_REGISTRY/.git/</S>
    {"\n\n"}
    <C># 2. Tell Loom about an agent skills directory</C>
    {"\n"}
    <Prompt /> loom <F>--root</F> "$LOOM_REGISTRY" target add \{"\n"}
    {"    "}
    <F>--agent</F> AGENT <F>--path</F> <S>"$AGENT_SKILLS_DIR"</S> <F>--ownership</F> observed
    {"\n"}
    <OK>✓</OK> target added <E>TARGET_ID</E> → $AGENT_SKILLS_DIR <C>(observed)</C>
    {"\n\n"}
    <C># 3. Add another profile when needed</C>
    {"\n"}
    <Prompt /> loom target add <F>--agent</F> AGENT <F>--profile</F> PROFILE \{"\n"}
    {"    "}
    <F>--path</F> <S>"$PROFILE_SKILLS_DIR"</S> <F>--ownership</F> managed
    {"\n"}
    <OK>✓</OK> target added <E>TARGET_ID</E> → $PROFILE_SKILLS_DIR <C>(managed)</C>
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
    TARGET_COUNT{"\n\n"}
    <C># Doctor: inspect drift + projection health</C>
    {"\n"}
    <Prompt /> loom workspace doctor{"\n"}
    <OK>✓</OK> PROJECTION_COUNT projections clean{"\n"}
    <W>⚠</W> DRIFT_COUNT drifted · <E>SKILL_NAME</E> → TARGET_ID{"\n"}
    <OK>➜</OK> run <E>loom workspace repair</E> or <E>loom skill project</E> to realign
    {"\n\n"}
    <C># Register a binding</C>
    {"\n"}
    <Prompt /> loom workspace binding add \{"\n"}
    {"  "}
    <F>--agent</F> AGENT <F>--profile</F> PROFILE \{"\n"}
    {"  "}
    <F>--matcher-kind</F> path-prefix <F>--matcher-value</F> <S>"$WORKSPACE_PATH"</S> \{"\n"}
    {"  "}
    <F>--target</F> TARGET_ID{"\n"}
    <OK>✓</OK> binding <E>BINDING_ID</E> registered · policy-profile=auto
  </>
);

const Lifecycle = () => (
  <>
    <C># Capture a new version of an existing skill</C>
    {"\n"}
    <Prompt /> loom skill capture SKILL_NAME{"\n"}
    <OK>✓</OK> captured <E>SKILL_NAME#CAPTURE_ID</E> <C>(from target TARGET_ID)</C>
    {"\n\n"}
    <C># Diff the capture against the last release</C>
    {"\n"}
    <Prompt /> loom skill diff SKILL_NAME <F>--from</F> VERSION <F>--to</F> CAPTURE_ID{"\n"}
    <W>~</W> SKILL.md{"\n"}
    {"  "}
    <Del>- for simple function extractions only</Del>
    {"\n"}
    {"  "}
    <Add>+ for both function + module-level refactors</Add>
    {"\n"}
    {"  "}
    <Add>+ pairs well with RELATED_SKILL</Add>
    {"\n\n"}
    <C># Snapshot + release</C>
    {"\n"}
    <Prompt /> loom skill snapshot SKILL_NAME <F>--tag</F> SNAPSHOT_TAG{"\n"}
    <OK>✓</OK> snapshot SNAPSHOT_ID{"\n\n"}
    <Prompt /> loom skill release SKILL_NAME <F>--version</F> VERSION{"\n"}
    <OK>✓</OK> released <E>SKILL_NAME@VERSION</E>
    {"\n"}
    <OK>➜</OK> auto-projected to matching targets
    {"\n\n"}
    <C># Something broke? roll back</C>
    {"\n"}
    <Prompt /> loom skill rollback SKILL_NAME <F>--to</F> VERSION{"\n"}
    <OK>✓</OK> rolled back · replayed matching projections
  </>
);

const Sync = () => (
  <>
    <C># Push the registry to your team remote</C>
    {"\n"}
    <Prompt /> loom sync push{"\n"}
    <OK>✓</OK> pushed local registry changes → origin/main{"\n\n"}
    <C># Pull changes from a teammate</C>
    {"\n"}
    <Prompt /> loom sync pull{"\n"}
    <OK>✓</OK> fetched remote registry changes{"\n"}
    <W>⚠</W> pending projections may need replay <C>(skill state changed)</C>
    {"\n\n"}
    <C># Replay pending ops against your local targets</C>
    {"\n"}
    <Prompt /> loom sync replay{"\n"}
    <OK>✓</OK> replayed <E>OP_ID</E> → TARGET_ID{"\n"}
    <OK>✓</OK> replayed <E>OP_ID</E> → TARGET_ID
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
    OP_ID <W>pending</W> project SKILL_NAME@VERSION → TARGET_ID{"\n"}
    OP_ID <span style={{ color: "var(--err)" }}>failed</span> project SKILL_NAME@VERSION → TARGET_ID{"\n"}
    {"        "}
    <C>reason: target ownership=external; refusing to write</C>
    {"\n\n"}
    <C># Retry pending projections</C>
    {"\n"}
    <Prompt /> loom ops retry <F>--kind</F> project{"\n"}
    <OK>✓</OK> pending project operations retried{"\n\n"}
    <C># Diagnose + auto-repair drifted projections</C>
    {"\n"}
    <Prompt /> loom ops history diagnose{"\n"}
    <OK>✓</OK> ok / failed / pending counts reported{"\n"}
    {"  "}
    drift: <E>SKILL_NAME</E> → TARGET_ID <C>(projection health detail)</C>
    {"\n\n"}
    <Prompt /> loom ops history repair <F>--strategy</F> local{"\n"}
    <OK>✓</OK> repaired drifted projections from local skill store
  </>
);

export const CLI_TAB_CONTENT: Record<CliTabKey, ReactNode> = {
  quickstart: <Quickstart />,
  workspace: <Workspace />,
  lifecycle: <Lifecycle />,
  sync: <Sync />,
  ops: <Ops />,
};
