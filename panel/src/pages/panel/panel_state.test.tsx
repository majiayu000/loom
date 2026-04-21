import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { LiveDataBanner } from "../PanelApp";
import { bucket } from "./HistoryPage";
import type { V3OperationRecord } from "../../lib/api/client";

function makeOperation(status: string, ack = false): V3OperationRecord {
  return {
    op_id: "op_123",
    intent: "skill.project",
    status,
    ack,
    payload: {},
    effects: {},
    created_at: "2026-04-09T10:05:00Z",
    updated_at: "2026-04-09T10:05:00Z",
  };
}

test("HistoryPage treats succeeded operations as successful", () => {
  expect(bucket(makeOperation("succeeded", false))).toBe("ok");
});

test("LiveDataBanner renders nothing during live refetch loading", () => {
  const html = renderToStaticMarkup(
    <LiveDataBanner error={null} loading={true} mode="live" />,
  );
  expect(html).toBe("");
});
