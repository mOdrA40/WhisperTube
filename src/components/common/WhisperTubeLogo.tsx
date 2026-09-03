import type { SVGProps } from "react";

interface WhisperTubeLogoProps extends SVGProps<SVGSVGElement> {
  size?: number;
  animated?: boolean;
  variant?: "full" | "icon";
}

export function WhisperTubeLogo({
  size = 36,
  animated = false,
  variant = "icon",
  className = "",
  ...props
}: WhisperTubeLogoProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 128 128"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={`whispertube-logo ${animated ? "logo-active" : ""} ${className}`}
      {...props}
    >
      <defs>
        <radialGradient id="wt-outer-glow" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#FF2E55" stopOpacity="0.45" />
          <stop offset="55%" stopColor="#8B5CF6" stopOpacity="0.25" />
          <stop offset="100%" stopColor="#8B5CF6" stopOpacity="0" />
        </radialGradient>

        <linearGradient id="wt-body-grad" x1="16" y1="16" x2="112" y2="112" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#1C1433" />
          <stop offset="45%" stopColor="#121626" />
          <stop offset="100%" stopColor="#0B0E1B" />
        </linearGradient>

        <linearGradient id="wt-border-grad" x1="14" y1="20" x2="114" y2="108" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#FF385C" />
          <stop offset="35%" stopColor="#D946EF" />
          <stop offset="70%" stopColor="#6366F1" />
          <stop offset="100%" stopColor="#06B6D4" />
        </linearGradient>

        <linearGradient id="wt-wave-1" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#FF6B4A" />
          <stop offset="100%" stopColor="#FF2E55" />
        </linearGradient>

        <linearGradient id="wt-wave-2" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#FF387A" />
          <stop offset="100%" stopColor="#C026D3" />
        </linearGradient>

        <linearGradient id="wt-wave-3" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#C084FC" />
          <stop offset="100%" stopColor="#7C3AED" />
        </linearGradient>

        <linearGradient id="wt-wave-4" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#38BDF8" />
          <stop offset="100%" stopColor="#2563EB" />
        </linearGradient>

        <linearGradient id="wt-wave-5" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#2DD4BF" />
          <stop offset="100%" stopColor="#059669" />
        </linearGradient>

        <filter id="wt-neon-filter" x="-20%" y="-20%" width="140%" height="140%">
          <feDropShadow dx="0" dy="0" stdDeviation="2.5" floodColor="#8B5CF6" floodOpacity="0.6" />
        </filter>
      </defs>

      {/* Ambient background glow */}
      <rect x="4" y="4" width="120" height="120" rx="36" fill="url(#wt-outer-glow)" />

      {/* Outer Tube Shell with subtle border */}
      <rect
        x="15"
        y="23"
        width="98"
        height="82"
        rx="24"
        fill="url(#wt-body-grad)"
        stroke="url(#wt-border-grad)"
        strokeWidth="2.5"
      />

      {/* Subtle top reflection */}
      <path
        d="M23 29 C45 25, 83 25, 105 29 C107 29.5, 108 31, 108 33 C108 35, 20 35, 20 33 C20 31, 21 29.5, 23 29 Z"
        fill="#FFFFFF"
        fillOpacity="0.09"
      />

      {/* Equalizer Soundwave Bars shaped in a dynamic Play form */}
      <rect
        className="wt-bar wt-bar-1"
        x="36"
        y="52"
        width="6.5"
        height="24"
        rx="3.25"
        fill="url(#wt-wave-1)"
        filter="url(#wt-neon-filter)"
      />
      <rect
        className="wt-bar wt-bar-2"
        x="47"
        y="41"
        width="6.5"
        height="46"
        rx="3.25"
        fill="url(#wt-wave-2)"
        filter="url(#wt-neon-filter)"
      />
      <rect
        className="wt-bar wt-bar-3"
        x="58"
        y="33"
        width="7"
        height="62"
        rx="3.5"
        fill="url(#wt-wave-3)"
        filter="url(#wt-neon-filter)"
      />
      <rect
        className="wt-bar wt-bar-4"
        x="70"
        y="43"
        width="6.5"
        height="42"
        rx="3.25"
        fill="url(#wt-wave-4)"
        filter="url(#wt-neon-filter)"
      />
      <rect
        className="wt-bar wt-bar-5"
        x="81"
        y="54"
        width="6"
        height="20"
        rx="3"
        fill="url(#wt-wave-5)"
        filter="url(#wt-neon-filter)"
      />

      {/* Whisper Resonance Tip */}
      <circle
        className="wt-dot"
        cx="94"
        cy="64"
        r="3.2"
        fill="#00F5D4"
        filter="url(#wt-neon-filter)"
      />
    </svg>
  );
}
