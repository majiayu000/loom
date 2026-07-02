import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  api,
  type TelemetryAggregate,
  type TelemetryReportPayload,
} from "../../lib/api/client";
import { TelemetryPage } from "./TelemetryPage";

afterEach(() => {
  vi.restoreAllMocks();
});

function aggregate(overrides: Partial<TelemetryAggregate> = {}): TelemetryAggregate {
  return {
    events: 1,
    usage: {
      activations: 1,
      deactivations: 0,
      invocations: 0,
      errors: 0,
      status: "available",
    },
    value: {
      eval_runs: 1,
      passed: 1,
      failed: 0,
      pass_rate: 1,
      baseline_delta_avg: 0.2,
      status: "available",
    },
    cost: {
      tokens_in: 7,
      tokens_out: 3,
      commands: 2,
      duration_ms: 0,
      status: "available",
    },
    drift: {
      stale_eval_days: 4,
      last_successful_eval_at: "2026-07-01T00:00:00Z",
      status: "available",
    },
    risk: {
      safety_events: 1,
      safety_findings: 2,
      dependency_findings: 0,
      status: "available",
    },
    recommendation_feedback: {
      accepted: 1,
      rejected: 0,
      ignored: 0,
      status: "available",
    },
    ...overrides,
  };
}

function report(): TelemetryReportPayload {
  const demo = aggregate();
  return {
    schema_version: 1,
    enabled: true,
    mode: "local-only",
    retention_days: 90,
    events_total: 2,
    matched_events: 1,
    summary: demo,
    skills: { demo },
    panel_read_model: {
      status: "available",
      deferred_ui: false,
      route: "/api/v1/telemetry/report",
    },
  };
}

describe("TelemetryPage", () => {
  it("renders the telemetry report as a dashboard", async () => {
    vi.spyOn(api, "telemetryReportWithWarnings").mockResolvedValue({
      data: report(),
      warnings: ["telemetry event line 9 quarantined"],
    });

    render(<TelemetryPage apiReachable={true} mode="live" refreshKey="tick-1" />);

    expect(await screen.findByRole("heading", { name: "Telemetry" })).toBeInTheDocument();
    expect(await screen.findByText("telemetry event line 9 quarantined")).toBeInTheDocument();
    expect(screen.getByText("enabled")).toBeInTheDocument();
    expect(screen.getByText("2 stored")).toBeInTheDocument();
    expect(screen.getAllByText("demo").length).toBeGreaterThan(0);
    expect(screen.getByText("1 evals · 100%")).toBeInTheDocument();
    expect(screen.getByText("10 tokens · 2 commands")).toBeInTheDocument();
    expect(screen.getAllByText("3 signals").length).toBeGreaterThan(0);
    expect(screen.getAllByText("4d").length).toBeGreaterThan(0);
    expect(screen.getByRole("img", { name: "Usage vs eval delta scatterplot" })).toBeInTheDocument();
    expect(screen.getByText("Usage vs value")).toBeInTheDocument();
    expect(screen.getByText("High-risk active skills")).toBeInTheDocument();
    expect(screen.getByText("High overhead")).toBeInTheDocument();
  });

  it("does not fetch telemetry while the panel API is unreachable", async () => {
    const fetchTelemetry = vi.spyOn(api, "telemetryReportWithWarnings").mockResolvedValue({
      data: report(),
      warnings: [],
    });

    render(<TelemetryPage apiReachable={false} mode="offline-empty" refreshKey={null} />);

    expect(screen.getByText("Telemetry needs the live panel API.")).toBeInTheDocument();
    await waitFor(() => {
      expect(fetchTelemetry).not.toHaveBeenCalled();
    });
  });
});
