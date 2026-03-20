import React from "react";
import { useAppState, useAppDispatch } from "../state/context";
import type { ColorMode, ViewType } from "../types";

const COLOR_MODE_LABELS: Record<ColorMode, string> = {
  hotspot: "Hotspot",
  defect: "Defect",
  churn: "Churn",
  module: "Module",
  coupling: "Coupling",
  age: "Age",
};

export function Header() {
  const state = useAppState();
  const dispatch = useAppDispatch();

  return (
    <header className="h-12 bg-surface border-b border-elevated flex items-center px-4 gap-4 shrink-0">
      <span className="font-sans font-semibold text-text-primary text-sm tracking-wide">
        ISING
      </span>

      <div className="flex gap-1 ml-4">
        {(["treemap", "blast"] as ViewType[]).map((v) => (
          <button
            key={v}
            onClick={() => dispatch({ type: "SET_VIEW", view: v })}
            className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
              state.activeView === v
                ? "bg-elevated text-text-primary"
                : "text-text-muted hover:text-text-secondary"
            }`}
          >
            {v === "treemap" ? "Treemap" : "Blast Radius"}
          </button>
        ))}
      </div>

      {state.activeView === "treemap" && (
        <div className="flex gap-1 ml-4">
          {(Object.keys(COLOR_MODE_LABELS) as ColorMode[]).map((mode) => (
            <button
              key={mode}
              onClick={() => dispatch({ type: "SET_COLOR_MODE", mode })}
              className={`px-2 py-1 rounded text-xs transition-colors ${
                state.treemapColorMode === mode
                  ? "bg-elevated text-text-primary"
                  : "text-text-muted hover:text-text-secondary"
              }`}
            >
              {COLOR_MODE_LABELS[mode]}
            </button>
          ))}
          <button
            onClick={() => dispatch({ type: "TOGGLE_SIGNAL_OVERLAY" })}
            className={`px-2 py-1 rounded text-xs ml-2 transition-colors ${
              state.signalOverlay
                ? "bg-elevated text-text-primary"
                : "text-text-muted hover:text-text-secondary"
            }`}
          >
            Signals
          </button>
        </div>
      )}

      {state.activeView === "blast" && (
        <div className="flex items-center gap-2 ml-4">
          <span className="text-xs text-text-muted">Depth:</span>
          {([1, 2, 3] as const).map((d) => (
            <button
              key={d}
              onClick={() => dispatch({ type: "SET_BLAST_DEPTH", depth: d })}
              className={`w-6 h-6 rounded text-xs transition-colors ${
                state.blastDepth === d
                  ? "bg-elevated text-text-primary"
                  : "text-text-muted hover:text-text-secondary"
              }`}
            >
              {d}
            </button>
          ))}
          <div className="flex gap-1 ml-4">
            {(["structural", "change", "defect"] as const).map((layer) => (
              <button
                key={layer}
                onClick={() => dispatch({ type: "TOGGLE_LAYER", layer })}
                className={`px-2 py-1 rounded text-xs transition-colors ${
                  state.activeLayers[layer]
                    ? "bg-elevated text-text-primary"
                    : "text-text-muted hover:text-text-secondary"
                }`}
              >
                {layer}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="ml-auto">
        <input
          type="text"
          placeholder="Search files..."
          value={state.searchQuery}
          onChange={(e) =>
            dispatch({ type: "SET_SEARCH", query: e.target.value })
          }
          className="bg-elevated border border-text-muted/20 rounded px-3 py-1 text-xs text-text-primary placeholder-text-muted w-48 focus:outline-none focus:border-text-secondary"
        />
      </div>
    </header>
  );
}
