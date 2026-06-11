import type { MutationState } from "../../lib/useMutation";

type MutationBannerTone = "ok" | "err" | "warn" | "muted";
type MutationBannerVariant = "inline" | "bar";
type MutationBannerSpacing = "none" | "top" | "bottom";

interface MutationBannerProps {
  state?: Pick<MutationState, "error" | "success"> | null;
  error?: string | null;
  success?: string | null;
  message?: string | null;
  tone?: MutationBannerTone;
  variant?: MutationBannerVariant;
  spacing?: MutationBannerSpacing;
  className?: string;
  prefixSuccess?: boolean;
}

export function MutationBanner({
  state,
  error = state?.error ?? null,
  success = state?.success ?? null,
  message,
  tone,
  variant = "inline",
  spacing = "none",
  className,
  prefixSuccess = true,
}: MutationBannerProps) {
  const content = message ?? error ?? (success ? `${prefixSuccess ? "✓ " : ""}${success}` : null);
  if (!content) return null;

  const resolvedTone = tone ?? (error ? "err" : "ok");
  const classes = ["mutation-note", className].filter(Boolean).join(" ");

  return (
    <div className={classes} data-tone={resolvedTone} data-variant={variant} data-spacing={spacing}>
      {content}
    </div>
  );
}
