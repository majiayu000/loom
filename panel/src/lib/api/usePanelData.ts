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

export interface PanelLiveData {
  live: boolean;
  loading: boolean;
  error: string | null;
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

export function usePanelData(): PanelLiveData {
  const [state, setState] = useState<LiveState>(INITIAL_STATE);

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

      if (!v3.ok) {
        throw new ApiError(
          "/api/v3/status",
          200,
          v3.error?.message ?? v3.error?.code ?? "GET /api/v3/status returned ok=false",
        );
      }
      if (!v3.data) {
        throw new ApiError("/api/v3/status", 200, "GET /api/v3/status returned no data");
      }
      if (remote.error) {
        throw new ApiError(
          "/api/remote/status",
          200,
          remote.error.message ?? remote.error.code ?? "GET /api/remote/status returned an error",
        );
      }

      const v3Data = v3.data;
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

      setState({
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
      });
    } catch (err) {
      if (controller.signal.aborted || generation !== generationRef.current) return;
      const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
      setState((cur) => ({ ...cur, loading: false, live: false, error: message }));
    }
  }, []);

  useEffect(() => {
    runFetch();
    const id = window.setInterval(runFetch, POLL_MS);
    return () => {
      window.clearInterval(id);
      controllerRef.current?.abort();
      controllerRef.current = null;
    };
  }, [runFetch]);

  const refetch = useCallback(() => {
    runFetch();
  }, [runFetch]);

  return { ...state, refetch };
}
