import * as d3 from "d3";
import type { ColorMode, SignalType, VizNode } from "../types";

export const SIGNAL_COLORS: Record<SignalType, string> = {
  ticking_bomb: "#dc2626",
  fragile_boundary: "#ef4444",
  ghost_coupling: "#f59e0b",
  over_engineering: "#6b7280",
  stable_core: "#10b981",
};

export const LAYER_COLORS = {
  structural: "#94a3b8",
  change: "#f59e0b",
  defect: "#ef4444",
};

export const LAYER_DASH = {
  structural: "none",
  change: "6,3",
  defect: "3,3",
};

export const LAYER_WIDTH_FACTOR = {
  structural: 1.5,
  change: 3,
  defect: 4,
};

export function getNodeColor(
  node: VizNode,
  mode: ColorMode,
  moduleColorMap: Map<string, string>,
  maxValues: Record<string, number>
): string {
  switch (mode) {
    case "hotspot":
      return d3.interpolateYlOrRd(node.hotspot / Math.max(maxValues.hotspot, 0.01));
    case "defect":
      return d3.interpolateYlOrRd(
        node.defect_density / Math.max(maxValues.defect_density, 0.01)
      );
    case "churn":
      return d3.interpolateYlOrRd(
        node.churn_rate / Math.max(maxValues.churn_rate, 0.01)
      );
    case "module":
      return moduleColorMap.get(node.module) || "#6b7280";
    case "coupling":
      return d3.interpolatePuBuGn(
        node.sum_coupling / Math.max(maxValues.sum_coupling, 0.01)
      );
    case "age": {
      if (!node.last_changed) return "#10b981";
      const daysSince =
        (Date.now() - new Date(node.last_changed).getTime()) /
        (1000 * 60 * 60 * 24);
      const maxDays = Math.max(maxValues.max_age_days, 1);
      // Older = greener (stable), newer = redder
      return d3.interpolateRdYlGn(daysSince / maxDays);
    }
    default:
      return "#64748b";
  }
}

export function computeMaxValues(nodes: VizNode[]): Record<string, number> {
  const result: Record<string, number> = {
    hotspot: 0,
    defect_density: 0,
    churn_rate: 0,
    sum_coupling: 0,
    max_age_days: 0,
    loc: 0,
    complexity: 0,
  };
  const now = Date.now();
  for (const n of nodes) {
    result.hotspot = Math.max(result.hotspot, n.hotspot);
    result.defect_density = Math.max(result.defect_density, n.defect_density);
    result.churn_rate = Math.max(result.churn_rate, n.churn_rate);
    result.sum_coupling = Math.max(result.sum_coupling, n.sum_coupling);
    result.loc = Math.max(result.loc, n.loc);
    result.complexity = Math.max(result.complexity, n.complexity);
    if (n.last_changed) {
      const days =
        (now - new Date(n.last_changed).getTime()) / (1000 * 60 * 60 * 24);
      result.max_age_days = Math.max(result.max_age_days, days);
    }
  }
  return result;
}
