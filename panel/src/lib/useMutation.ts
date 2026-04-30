import { useState } from "react";

export interface MutationState {
  busy: boolean;
  error: string | null;
  success: string | null;
}

export function useMutation() {
  const [state, setState] = useState<MutationState>({ busy: false, error: null, success: null });

  async function run(label: string, fn: () => Promise<unknown>, onDone?: (result: unknown) => void) {
    setState({ busy: true, error: null, success: null });
    try {
      const result = await fn();
      setState({ busy: false, error: null, success: label });
      onDone?.(result);
      window.setTimeout(() => setState((s) => (s.success === label ? { ...s, success: null } : s)), 3000);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setState({ busy: false, error: `${label}: ${message}`, success: null });
    }
  }

  return { ...state, run };
}
