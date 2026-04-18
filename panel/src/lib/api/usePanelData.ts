import { useCallback, useEffect, useRef, useState } from "react";
import type { HealthPayload, RemotePayload, V3Payload } from "../../types";
import type { Binding, Op, Skill, Target } from "../types";
import { adaptBinding, adaptPendingOp, adaptProjectionOp, adaptSkill, adaptTarget } from "./adapters";
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
  pendingCount: number;
  refetch: () => void;
}

const EMPTY_COUNTS: V3Counts = {};

const POLL_MS = 10_000;

export function usePanelData(): PanelLiveData {
  const [state, setState] = useState<Omit<PanelLiveData, "refetch">>({
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
    pendingCount: 0,
  });

  const tickRef = useRef(0);

  const runFetch = useCallback(async (signal: AbortSignal) => {
    try {
      const [health, info, skillsPayload, v3, remote, pending] = await Promise.all([
        api.health(signal),
        api.info(signal),
        api.skills(signal),
        api.v3Status(signal),
        api.remoteStatus(signal),
        api.pending(signal),
      ]);
      if (signal.aborted) return;

      const v3Data = v3.ok && v3.data ? v3.data : {};
      const projections = v3Data.projections ?? [];
      const rules = v3Data.rules ?? [];
      const v3Targets = v3Data.targets ?? [];
      const v3Bindings = v3Data.bindings ?? [];

      const targets = v3Targets.map((t) => adaptTarget(t, projections));
      const skillNames = skillsPayload.skills ?? [];
      const skills = skillNames.map((name) => adaptSkill(name, projections, rules));
      const bindings = v3Bindings.map((b) => adaptBinding(b, rules));

      const pendingOps: Op[] = (pending.ops ?? []).map(adaptPendingOp);
      const projectionOps: Op[] = projections.map((p) => adaptProjectionOp(p, v3Targets.find((x) => x.target_id === p.target_id)));
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
        pendingCount: pending.count ?? 0,
      });
    } catch (err) {
      if (signal.aborted) return;
      const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
      setState((cur) => ({ ...cur, loading: false, live: false, error: message }));
    }
  }, []);

  useEffect(() => {
    let active = true;
    const controller = new AbortController();

    const kick = () => {
      if (!active) return;
      runFetch(controller.signal);
    };

    kick();
    const id = window.setInterval(kick, POLL_MS);
    return () => {
      active = false;
      controller.abort();
      window.clearInterval(id);
    };
  }, [runFetch]);

  const refetch = useCallback(() => {
    tickRef.current += 1;
    const controller = new AbortController();
    runFetch(controller.signal);
  }, [runFetch]);

  return { ...state, refetch };
}
