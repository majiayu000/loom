declare module "bun:test" {
  export const expect: (value: unknown) => {
    toBe: (expected: unknown) => void;
  };
  export const test: (name: string, fn: () => void | Promise<void>) => void;
  export const afterAll: (fn: () => void | Promise<void>) => void;
}
