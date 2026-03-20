import React from "react";

interface TooltipProps {
  x: number;
  y: number;
  children: React.ReactNode;
}

export function Tooltip({ x, y, children }: TooltipProps) {
  return (
    <div
      className="fixed z-50 pointer-events-none bg-elevated border border-text-muted/30 rounded px-3 py-2 text-xs text-text-primary shadow-lg max-w-xs"
      style={{
        left: x + 12,
        top: y - 8,
      }}
    >
      {children}
    </div>
  );
}
