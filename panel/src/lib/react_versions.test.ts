import { version as reactVersion } from "react";
import { version as reactDOMVersion } from "react-dom";
import { expect, test } from "vitest";

test("React and React DOM use the same exact runtime version", () => {
  expect(reactDOMVersion).toBe(reactVersion);
});
