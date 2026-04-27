import { useCallback, useEffect, useRef, useState } from "react";
import type { V3Projection } from "../../generated/V3Projection";
import type { HealthPayload, RemotePayload, V3Payload } from "../../types";
import type { Binding, Op, Skill, Target } from "../types";
import {
  adaptBinding,
  adaptPendingOp,
  adaptProjectionOp,
  adaptSkill,
  adaptTarget,
  buildAdapterIndex,
} from "./adapters";
import { ApiError, api } from "./client";

type V3Counts = NonNullable<NonNullable<V3Payload["data"]>["counts"]>;

export type PanelDataMode = "live" | "offline-empty" | "offline-stale";

export interface PanelLiveData {
  live: boolean;
  loading: boolean;
  error: string | null;
  mode: PanelDataMode;
  lastUpdated: string | null;
  registryRoot: string | null;
  remote: RemotePayload | null;
  health: HealthPayload | null;
  counts: V3Counts;
  skills: Skill[];
  targets: Target[];
  bindings: Binding[];
  ops: Op[];
  /** Raw V3 projections — exposed so consumers like `ProjectionGraph` can
   *  use the backend-reported `method`/`health` instead of fabricating it. */
  projections: V3Projection[];
  pendingCount: number;
  refetch: () => void;
}

const EMPTY_COUNTS: V3Counts = {};

const POLL_MS = 10_000;

type LiveState = Omit<PanelLiveData, "refetch">;

const INITIAL_STATE: LiveState = {
  live: false,
  loading: true,
  error: null,
  mode: "offline-empty",
  lastUpdated: null,
  registryRoot: null,
  remote: null,
  health: null,
  counts: EMPTY_COUNTS,
  skills: [],
  targets: [],
  bindings: [],
  ops: [],
  projections: [],
  pendingCount: 0,
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
  if (state.live) return "live";
  return hasLastKnownData(state as LiveState) ? "offline-stale" : "offline-empty";
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
    (cur: LiveState, message: string): LiveState => withMode({ ...cur, live: false, loading: false, error: message }),
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

    try {
      const [health, info, skillsPayload, v3, remote, pending] = await Promise.all([
        api.health(controller.signal),
        api.info(controller.signal),
        api.skills(controller.signal),
        api.v3Status(controller.signal),
        api.remoteStatus(controller.signal),
        api.pending(controller.signal),
      ]);
      if (controller.signal.aborted || generation !== generationRef.current) return;

      const v3Data = v3.data ?? {};
      const projections = v3Data.projections ?? [];
      const rules = v3Data.rules ?? [];
      const v3Targets = v3Data.targets ?? [];
      const v3Bindings = v3Data.bindings ?? [];

      const index = buildAdapterIndex(v3Targets, projections);
      const targets = v3Targets.map((t) => adaptTarget(t, index));
      const skillNames = skillsPayload.skills ?? [];
      const skills = skillNames.map((name) => adaptSkill(name, index, rules));
      const bindings = v3Bindings.map((b) => adaptBinding(b, rules));

      const pendingOps: Op[] = (pending.ops ?? []).map(adaptPendingOp);
      const projectionOps: Op[] = projections.map((p) => adaptProjectionOp(p, index));
      const ops = [...pendingOps, ...projectionOps].slice(0, 30);

      setState(
        markSuccess({
          live: true,
          loading: false,
          error: null,
          lastUpdated: new Date().toISOString(),
          registryRoot: info.root ?? null,
          remote: remote.remote ?? null,
          health,
          counts: v3Data.counts ?? EMPTY_COUNTS,
          skills,
          targets,
          bindings,
          ops,
          projections,
          pendingCount: pending.count ?? 0,
        }),
      );
    } catch (err) {
      if (controller.signal.aborted || generation !== generationRef.current) return;
      const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
      setState((cur) => markFailure(cur, message));
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
