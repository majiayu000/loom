import { useEffect, useState, type DependencyList } from "react";

export type ApiQueryState<T> =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; payload: T }
  | { kind: "error"; message: string };

interface UseApiQueryOptions {
  enabled?: boolean;
  getErrorMessage?: (error: unknown) => string;
}

export function useApiQuery<T>(
  fetcher: (signal: AbortSignal) => Promise<T>,
  deps: DependencyList,
  options: UseApiQueryOptions = {},
): ApiQueryState<T> {
  const enabled = options.enabled ?? true;
  const getErrorMessage = options.getErrorMessage ?? defaultErrorMessage;
  const [state, setState] = useState<ApiQueryState<T>>({ kind: "idle" });

  useEffect(() => {
    if (!enabled) {
      setState({ kind: "idle" });
      return;
    }

    const controller = new AbortController();
    setState({ kind: "loading" });
    fetcher(controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return;
        setState({ kind: "ready", payload });
      })
      .catch((error) => {
        if (controller.signal.aborted) return;
        setState({ kind: "error", message: getErrorMessage(error) });
      });

    return () => controller.abort();
  }, [enabled, ...deps]);

  return state;
}

function defaultErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
