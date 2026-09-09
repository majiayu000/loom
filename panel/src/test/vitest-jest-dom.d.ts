import "vitest";
import type { TestingLibraryMatchers } from "@testing-library/jest-dom/matchers";

// Vitest 5 moved custom matcher typing onto Matchers<R, T>.
// @testing-library/jest-dom still augments the older one-parameter Assertion<T>,
// which no longer merges under Vitest 5.
declare module "vitest" {
  interface Matchers<R = void, T = unknown>
    // biome-ignore lint/suspicious/noExplicitAny: mirrors jest-dom's published vitest augmentation
    extends TestingLibraryMatchers<any, R> {}
}
