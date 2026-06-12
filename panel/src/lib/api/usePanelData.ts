import { useCallback, useEffect, useRef, useState } from "react";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import type { HealthPayload, InfoPayload, RemotePayload, RegistryPayload } from "../../types";
import type { Binding, Op, Skill, Target } from "../types";
import {
  adaptBinding,
  adaptPendingOp,
  adaptRegistryOperation,
  adaptSkill,
  adaptSkillSummary,
  adaptTarget,
  buildAdapterIndex,
} from "./adapters";
import { ApiError, api } from "./client";

type RegistryCounts = NonNullable<NonNullable<RegistryPayload["data"]>["counts"]>;
type AgentDir = NonNullable<InfoPayload["agent_dirs"]>[number];

export type PanelDataMode = "live" | "first-run" | "offline-empty" | "offline-stale";

export interface PanelLiveData {
  live: boolean;
  apiReachable: boolean;
  loading: boolean;
  error: string | null;
  mode: PanelDataMode;
  setupRequired: boolean;
  lastUpdated: string | null;
  registryRoot: string | null;
  agentDirs: AgentDir[];
  remote: RemotePayload | null;
  warnings: string[];
  health: HealthPayload | null;
  counts: RegistryCounts;
  skills: Skill[];
  targets: Target[];
  bindings: Binding[];
  ops: Op[];
  /** Raw Registry projections — exposed so consumers like `ProjectionGraph` can
   *  use the backend-reported `method`/`health` instead of fabricating it. */
  projections: RegistryProjection[];
  queuedWriteCount: number;
  refetch: () => void;
}

const EMPTY_COUNTS: RegistryCounts = {};

const POLL_MS = 10_000;

type LiveState = Omit<PanelLiveData, "refetch">;

const INITIAL_STATE: LiveState = {
  live: false,
  apiReachable: false,
  loading: true,
  error: null,
  mode: "offline-empty",
  setupRequired: false,
  lastUpdated: null,
  registryRoot: null,
  agentDirs: [],
  remote: null,
  warnings: [],
  health: null,
  counts: EMPTY_COUNTS,
  skills: [],
  targets: [],
  bindings: [],
  ops: [],
  projections: [],
  queuedWriteCount: 0,
};

function hasLastKnownData(state: LiveState): boolean {
  return (
    state.skills.length > 0 ||
    state.targets.length > 0 ||
    state.bindings.length > 0 ||
    state.ops.length > 0 ||
    state.projections.length > 0 ||
    state.lastUpdated !== null ||
    state.registryRoot !== null ||
    state.remote !== null ||
    state.health !== null
  );
}

function modeForState(state: Omit<LiveState, "mode">): PanelDataMode {
  if (state.setupRequired) return "first-run";
  if (state.live) return "live";
  return hasLastKnownData(state as LiveState) ? "offline-stale" : "offline-empty";
}

function warningStrings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((warning): warning is string => typeof warning === "string" && warning.length > 0);
}

function uniqueWarnings(warnings: string[]): string[] {
  return Array.from(new Set(warnings));
}

export function usePanelData(): PanelLiveData {
  const [state, setState] = useState<LiveState>(INITIAL_STATE);

  const withMode = useCallback(
    (next: Omit<LiveState, "mode">): LiveState => ({ ...next, mode: modeForState(next) }),
    [],
  );

  const markLoading = useCallback(
    (cur: LiveState): LiveState => ({ ...cur, loading: true, error: null, mode: cur.mode }),
    [],
  );

  const markFailure = useCallback(
    (cur: LiveState, message: string, apiReachable: boolean): LiveState =>
      withMode({ ...cur, live: false, apiReachable, setupRequired: false, loading: false, error: message }),
    [withMode],
  );

  const markSuccess = useCallback(
    (next: Omit<LiveState, "mode">): LiveState => withMode(next),
    [withMode],
  );

  // Single in-flight controller. `refetch` aborts the old one before
  // starting a new fetch so stale responses can never overwrite fresher
  // ones (cf. PR #7 review H1: race + AbortController leak).
  const controllerRef = useRef<AbortController | null>(null);
  const generationRef = useRef(0);

  const runFetch = useCallback(async () => {
    controllerRef.current?.abort();
    const controller = new AbortController();
    controllerRef.current = controller;
    const generation = ++generationRef.current;
    let apiReachable = false;

    try {
      const [health, info, workspaceStatus] = await Promise.all([
        api.health(controller.signal),
        api.infoWithWarnings(controller.signal),
        api.workspaceStatusWithWarnings(controller.signal),
      ]);
      if (controller.signal.aborted || generation !== generationRef.current) return;
      apiReachable = true;
      const baseWarnings = uniqueWarnings([...info.warnings, ...workspaceStatus.warnings]);

      if (workspaceStatus.data.registry?.available === false) {
        setState(
          markSuccess({
            live: true,
            apiReachable: true,
            setupRequired: true,
            loading: false,
            error: null,
            lastUpdated: new Date().toISOString(),
            registryRoot: info.data.root ?? null,
            agentDirs: info.data.agent_dirs ?? [],
            remote: null,
            warnings: baseWarnings,
            health,
            counts: EMPTY_COUNTS,
            skills: [],
            targets: [],
            bindings: [],
            ops: [],
            projections: [],
            queuedWriteCount: 0,
          }),
        );
        return;
      }

      const [skillsPayload, registry, remote, pending, activity] = await Promise.all([
        api.skillsWithWarnings(controller.signal),
        api.registryStatusWithWarnings(controller.signal),
        api.remoteStatusWithWarnings(controller.signal),
        api.pendingWithWarnings(controller.signal),
        api.opsWithWarnings({ limit: 30 }, controller.signal),
      ]);
      if (controller.signal.aborted || generation !== generationRef.current) return;

      const registryData = registry.data.data ?? {};
      const projections = registryData.projections ?? [];
      const rules = registryData.rules ?? [];
      const registryTargets = registryData.targets ?? [];
      const registryBindings = registryData.bindings ?? [];

      const index = buildAdapterIndex(registryTargets, projections);
      const skillItems = skillsPayload.data.skills ?? [];
      const skills = skillItems.map((item) =>
        typeof item === "string" ? adaptSkill(item, index, rules) : adaptSkillSummary(item),
      );
      const observedSkillCounts = new Map<string, number>();
      for (const skill of skills) {
        for (const targetId of skill.observedTargetIds ?? []) {
          observedSkillCounts.set(targetId, (observedSkillCounts.get(targetId) ?? 0) + 1);
        }
      }
      const targets = registryTargets.map((t) => adaptTarget(t, index, observedSkillCounts));
      const bindings = registryBindings.map((b) => adaptBinding(b, rules));

      const pendingOps: Op[] = (pending.data.ops ?? []).map(adaptPendingOp);
      const activityOps: Op[] = (activity.data.data?.operations ?? []).map(adaptRegistryOperation);
      const ops = [...pendingOps, ...activityOps].slice(0, 30);
      const warnings = uniqueWarnings([
        ...baseWarnings,
        ...skillsPayload.warnings,
        ...registry.warnings,
        ...remote.warnings,
        ...pending.warnings,
        ...activity.warnings,
        ...warningStrings(remote.data.warnings),
        ...warningStrings(pending.data.warnings),
      ]);

      setState(
        markSuccess({
          live: true,
          apiReachable: true,
          setupRequired: false,
          loading: false,
          error: null,
          lastUpdated: new Date().toISOString(),
          registryRoot: info.data.root ?? null,
          agentDirs: info.data.agent_dirs ?? [],
          remote: remote.data.remote ?? null,
          warnings,
          health,
          counts: registryData.counts ?? EMPTY_COUNTS,
          skills,
          targets,
          bindings,
          ops,
          projections,
          queuedWriteCount: pending.data.count ?? 0,
        }),
      );
    } catch (err) {
      if (controller.signal.aborted || generation !== generationRef.current) return;
      const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
      setState((cur) => markFailure(cur, message, apiReachable));
    }
  }, [markFailure, markSuccess]);

  useEffect(() => {
    setState((cur) => markLoading(cur));
    runFetch();
    const id = window.setInterval(runFetch, POLL_MS);
    return () => {
      window.clearInterval(id);
      controllerRef.current?.abort();
      controllerRef.current = null;
    };
  }, [markLoading, runFetch]);

  const refetch = useCallback(() => {
    setState((cur) => markLoading(cur));
    runFetch();
  }, [markLoading, runFetch]);

  return { ...state, refetch };
}
