import type { SVGProps } from "react";

interface LoomMarkProps extends SVGProps<SVGSVGElement> {
  size?: number;
}

export function LoomMark({ size = 24, ...props }: LoomMarkProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" {...props}>
      <line x1="6" y1="3" x2="6" y2="21" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <line x1="12" y1="3" x2="12" y2="21" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <line x1="18" y1="3" x2="18" y2="21" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <path
        d="M 3 12 L 12 7.5 L 21 12 L 12 16.5 Z"
        fill="#d97736"
        stroke="#d97736"
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
      <circle cx="12" cy="12" r="1.4" fill="#0e0d0b" />
    </svg>
  );
}

export function LoomFooterMark({ size = 22 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
    >
      <line x1="5" y1="3" x2="5" y2="21" />
      <line x1="9" y1="3" x2="9" y2="21" />
      <line x1="13" y1="3" x2="13" y2="21" />
      <line x1="17" y1="3" x2="17" y2="21" />
      <line x1="3" y1="9" x2="21" y2="9" stroke="#d97736" strokeWidth="2" />
      <line x1="3" y1="15" x2="21" y2="15" stroke="#d97736" strokeWidth="2" />
    </svg>
  );
}
