import type { SVGProps } from "react";

interface WhisperTubeLogoProps extends SVGProps<SVGSVGElement> {
  size?: number;
  animated?: boolean;
  variant?: "full" | "icon";
}

export function WhisperTubeLogo({ size = 36, animated = false, variant = "icon", className = "", ...props }: WhisperTubeLogoProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={`whispertube-logo ${animated ? "logo-active" : ""} logo-${variant} ${className}`}
      aria-hidden="true" {...props}>
      <rect x="2" y="2" width="28" height="28" rx="9" fill="currentColor" fillOpacity="0.1" />
      <path d="M9 13v6m4-10v14m6-12v10m4-6v2" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" />
    </svg>
  );
}
