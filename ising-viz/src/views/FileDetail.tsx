import React from "react";
import { useAppState } from "../state/context";
import { MetricBar } from "../components/MetricBar";
import { SeverityBadge } from "../components/SeverityBadge";
import { SIGNAL_COLORS } from "../utils/colors";
import type { VizExport, SignalType } from "../types";
import type { DerivedData } from "../data/derived";

const SIGNAL_ICONS: Record<SignalType, string> = {
  ticking_bomb: "\u{1F4A3}",
  fragile_boundary: "\u26A1",
  ghost_coupling: "\u{1F47B}",
  over_engineering: "\u{1F527}",
  stable_core: "\u{1F6E1}",
};

interface FileDetailProps {
  data: VizExport;
  derived: DerivedData;
  maxValues: Record<string, number>;
}

export function FileDetail({ data, derived, maxValues }: FileDetailProps) {
  const state = useAppState();

  if (!state.selectedNode) {
    return (
      <div className="p-4 text-text-muted text-xs text-center">
        Click a file to see details
      </div>
    );
  }

  const node = derived.nodeMap.get(state.selectedNode);
  if (!node) {
    return (
      <div className="p-4 text-text-muted text-xs">
        Node not found: {state.selectedNode}
      </div>
    );
  }

  const signals = derived.signalIndex.byNode.get(node.id) || [];
  const edges = derived.edgeIndex.get(node.id) || [];

  const structuralOut = edges.filter(
    (e) => e.layer === "structural" && e.source === node.id
  ).length;
  const structuralIn = edges.filter(
    (e) => e.layer === "structural" && e.target === node.id
  ).length;
  const temporalCount = edges.filter(
    (e) => e.layer === "change" && e.weight > 0.3
  ).length;
  const defectCount = edges.filter((e) => e.layer === "defect").length;

  return (
    <div className="p-3 space-y-3 text-xs">
      {/* Section A: Identity */}
      <div>
        <div className="font-medium text-text-primary text-sm break-all">
          {node.id}
        </div>
        <div className="text-text-muted mt-1">
          {node.module}/ &middot; {node.language || "unknown"} &middot; LOC{" "}
          {node.loc} &middot; Fan-in {node.fan_in} &middot; Fan-out{" "}
          {node.fan_out} &middot; Bugs {node.bug_count}
        </div>
      </div>

      {/* Section B: Risk Metrics */}
      <div className="space-y-1.5">
        <MetricBar
          label="Hotspot"
          value={node.hotspot}
          max={1}
          color="#f59e0b"
        />
        <MetricBar
          label="Complexity"
          value={node.complexity}
          max={Math.max(maxValues.complexity, 50)}
          color="#3b82f6"
        />
        <MetricBar
          label="Churn Rate"
          value={node.churn_rate}
          max={Math.max(maxValues.churn_rate, 3)}
          color="#f59e0b"
        />
        <MetricBar
          label="Defect Density"
          value={node.defect_density}
          max={Math.max(maxValues.defect_density, 0.15)}
          color="#ef4444"
        />
        <MetricBar
          label="Change Freq"
          value={node.change_freq}
          max={maxValues.change_freq || 1}
          color="#8b5cf6"
        />
        <MetricBar
          label="Sum Coupling"
          value={node.sum_coupling}
          max={Math.max(maxValues.sum_coupling, 1)}
          color="#06b6d4"
        />
      </div>

      {/* Section C: Active Signals */}
      {signals.length > 0 && (
        <div className="space-y-1">
          <div className="text-text-muted font-medium">Signals</div>
          {signals.map((s, i) => (
            <div
              key={i}
              className="flex items-center justify-between text-xs"
            >
              <span>
                <span className="mr-1">
                  {SIGNAL_ICONS[s.type] || "!"}
                </span>
                {s.node_b ? (
                  <span>
                    &rarr; {s.node_b.split("/").pop()}
                  </span>
                ) : (
                  <span className="text-text-muted">node-level</span>
                )}
              </span>
              <SeverityBadge severity={s.severity} signalType={s.type} />
            </div>
          ))}
        </div>
      )}

      {/* Section D: Dependency Summary */}
      <div className="space-y-0.5">
        <div className="text-text-muted font-medium">Dependencies</div>
        <div>
          Structural: &rarr; {structuralOut} files &larr; {structuralIn} files
        </div>
        <div>Temporal: &harr; {temporalCount} files (coupling &gt; 0.3)</div>
        <div>Defect: &rarr; {defectCount} file(s)</div>
      </div>
    </div>
  );
}
