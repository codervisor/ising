import React from "react";

interface MetricBarProps {
  label: string;
  value: number;
  max: number;
  color: string;
}

export function MetricBar({ label, value, max, color }: MetricBarProps) {
  const pct = max > 0 ? Math.min((value / max) * 100, 100) : 0;
  return (
    <div className="flex items-center gap-2 text-xs">
      <span className="text-text-secondary w-28 truncate">{label}</span>
      <div className="flex-1 h-2 bg-surface rounded overflow-hidden">
        <div
          className="h-full rounded transition-all duration-300"
          style={{ width: `${pct}%`, backgroundColor: color }}
        />
      </div>
      <span className="text-text-muted w-12 text-right font-mono">
        {value < 1 ? value.toFixed(2) : value.toFixed(0)}
      </span>
    </div>
  );
}
