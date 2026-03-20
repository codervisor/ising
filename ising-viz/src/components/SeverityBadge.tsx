import React from "react";
import { SIGNAL_COLORS } from "../utils/colors";
import type { SignalType } from "../types";

interface SeverityBadgeProps {
  severity: number;
  signalType: SignalType;
}

export function SeverityBadge({ severity, signalType }: SeverityBadgeProps) {
  const color = SIGNAL_COLORS[signalType];
  return (
    <span
      className="inline-flex items-center px-1.5 py-0.5 rounded text-xs font-mono font-medium"
      style={{
        backgroundColor: `${color}20`,
        color,
      }}
    >
      {severity.toFixed(2)}
    </span>
  );
}
