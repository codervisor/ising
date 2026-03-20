import React, { useMemo } from "react";
import { useAppState, useAppDispatch } from "../state/context";
import { SeverityBadge } from "../components/SeverityBadge";
import { SIGNAL_COLORS } from "../utils/colors";
import { fileName } from "../utils/format";
import type { VizExport, VizSignal, SignalType } from "../types";
import type { DerivedData } from "../data/derived";

const SIGNAL_ICONS: Record<SignalType, string> = {
  ticking_bomb: "\u{1F4A3}",
  fragile_boundary: "\u26A1",
  ghost_coupling: "\u{1F47B}",
  over_engineering: "\u{1F527}",
  stable_core: "\u{1F6E1}",
};

const SIGNAL_LABELS: Record<SignalType, string> = {
  ticking_bomb: "Ticking Bomb",
  fragile_boundary: "Fragile Boundary",
  ghost_coupling: "Ghost Coupling",
  over_engineering: "Over Engineering",
  stable_core: "Stable Core",
};

const SIGNAL_ORDER: SignalType[] = [
  "ticking_bomb",
  "fragile_boundary",
  "ghost_coupling",
  "over_engineering",
  "stable_core",
];

interface SignalFeedProps {
  data: VizExport;
  derived: DerivedData;
}

export function SignalFeed({ data, derived }: SignalFeedProps) {
  const state = useAppState();
  const dispatch = useAppDispatch();

  const filteredSignals = useMemo(() => {
    return data.signals
      .filter(
        (s) =>
          state.signalTypeFilter.has(s.type) &&
          s.severity >= state.signalMinSeverity
      )
      .filter((s) => {
        if (!state.searchQuery) return true;
        const q = state.searchQuery.toLowerCase();
        return (
          s.node_a.toLowerCase().includes(q) ||
          (s.node_b?.toLowerCase().includes(q) ?? false)
        );
      });
  }, [data.signals, state.signalTypeFilter, state.signalMinSeverity, state.searchQuery]);

  const grouped = useMemo(() => {
    const groups = new Map<SignalType, VizSignal[]>();
    for (const type of SIGNAL_ORDER) {
      groups.set(type, []);
    }
    for (const signal of filteredSignals) {
      const list = groups.get(signal.type) || [];
      list.push(signal);
      groups.set(signal.type, list);
    }
    return groups;
  }, [filteredSignals]);

  return (
    <div className="flex flex-col h-full">
      {/* Filters */}
      <div className="p-2 border-b border-elevated space-y-2">
        <div className="flex flex-wrap gap-1">
          {SIGNAL_ORDER.map((type) => (
            <button
              key={type}
              onClick={() => dispatch({ type: "TOGGLE_SIGNAL_TYPE", signalType: type })}
              className={`px-1.5 py-0.5 rounded text-[10px] transition-colors ${
                state.signalTypeFilter.has(type)
                  ? "text-white"
                  : "text-text-muted opacity-40"
              }`}
              style={{
                backgroundColor: state.signalTypeFilter.has(type)
                  ? `${SIGNAL_COLORS[type]}40`
                  : "transparent",
              }}
            >
              {SIGNAL_ICONS[type]} {SIGNAL_LABELS[type]}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-2 text-[10px] text-text-muted">
          <span>Min severity:</span>
          <input
            type="range"
            min="0"
            max="1"
            step="0.05"
            value={state.signalMinSeverity}
            onChange={(e) =>
              dispatch({
                type: "SET_SIGNAL_MIN_SEVERITY",
                severity: parseFloat(e.target.value),
              })
            }
            className="flex-1 h-1 accent-text-secondary"
          />
          <span className="w-8 text-right">{state.signalMinSeverity.toFixed(2)}</span>
        </div>
      </div>

      {/* Signal list */}
      <div className="flex-1 overflow-y-auto">
        {SIGNAL_ORDER.map((type) => {
          const signals = grouped.get(type) || [];
          if (signals.length === 0) return null;

          return (
            <div key={type}>
              <div
                className="px-3 py-1.5 text-[10px] font-medium flex items-center gap-1.5 sticky top-0 bg-surface/95 backdrop-blur"
                style={{ color: SIGNAL_COLORS[type] }}
              >
                <span>{SIGNAL_ICONS[type]}</span>
                <span>{SIGNAL_LABELS[type]}</span>
                <span className="text-text-muted ml-auto">{signals.length}</span>
              </div>
              {signals.map((signal, i) => {
                const isActive =
                  state.selectedNode === signal.node_a ||
                  state.selectedNode === signal.node_b;

                return (
                  <div
                    key={`${signal.node_a}-${signal.node_b}-${i}`}
                    className={`px-3 py-2 border-b border-elevated/50 cursor-pointer hover:bg-elevated/50 transition-colors ${
                      isActive ? "bg-elevated/70 border-l-2" : ""
                    }`}
                    style={isActive ? { borderLeftColor: SIGNAL_COLORS[signal.type] } : undefined}
                    onClick={() => {
                      dispatch({ type: "SELECT_NODE", nodeId: signal.node_a });
                      dispatch({ type: "SET_VIEW", view: "blast" });
                    }}
                  >
                    <div className="flex items-center justify-between">
                      <span className="text-xs text-text-primary truncate">
                        {fileName(signal.node_a)}
                        {signal.node_b && (
                          <span className="text-text-muted">
                            {" "}
                            &rarr; {fileName(signal.node_b)}
                          </span>
                        )}
                      </span>
                      <SeverityBadge
                        severity={signal.severity}
                        signalType={signal.type}
                      />
                    </div>
                    {signal.detail && (
                      <div className="text-[10px] text-text-muted mt-0.5 line-clamp-2">
                        {signal.detail}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}

        {filteredSignals.length === 0 && (
          <div className="p-4 text-text-muted text-xs text-center">
            No signals match current filters
          </div>
        )}
      </div>
    </div>
  );
}
