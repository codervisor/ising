import React, { useMemo, useRef, useState, useCallback } from "react";
import * as d3 from "d3";
import { useAppState, useAppDispatch } from "../state/context";
import { getNodeColor, computeMaxValues, SIGNAL_COLORS } from "../utils/colors";
import { fileName } from "../utils/format";
import { Tooltip } from "../components/Tooltip";
import type { VizNode, VizExport, SignalType } from "../types";
import type { DerivedData } from "../data/derived";

interface TreemapProps {
  data: VizExport;
  derived: DerivedData;
  width: number;
  height: number;
}

const SIGNAL_ICONS: Record<SignalType, string> = {
  ticking_bomb: "\u{1F4A3}",
  fragile_boundary: "\u26A1",
  ghost_coupling: "\u{1F47B}",
  over_engineering: "\u{1F527}",
  stable_core: "\u{1F6E1}",
};

interface HierarchyNode {
  name: string;
  module?: string;
  nodeData?: VizNode;
  value?: number;
  children?: HierarchyNode[];
}

export function Treemap({ data, derived, width, height }: TreemapProps) {
  const state = useAppState();
  const dispatch = useAppDispatch();
  const [tooltip, setTooltip] = useState<{
    x: number;
    y: number;
    node: VizNode;
  } | null>(null);

  const maxValues = useMemo(() => computeMaxValues(data.nodes), [data.nodes]);

  const hierarchyData = useMemo((): HierarchyNode => {
    const { treemapHierarchy } = derived;
    if (state.treemapZoomModule) {
      const mod = treemapHierarchy.children.find(
        (c) => c.name === state.treemapZoomModule
      );
      if (mod) {
        return {
          name: mod.name,
          children: mod.children.map((n) => ({
            name: n.id,
            module: mod.name,
            nodeData: n,
            value: Math.max(n.loc, 1),
          })),
        };
      }
    }
    return {
      name: "root",
      children: treemapHierarchy.children.map((mod) => ({
        name: mod.name,
        children: mod.children.map((n) => ({
          name: n.id,
          module: mod.name,
          nodeData: n,
          value: Math.max(n.loc, 1),
        })),
      })),
    };
  }, [derived, state.treemapZoomModule]);

  const root = useMemo(() => {
    const hierarchy = d3
      .hierarchy(hierarchyData)
      .sum((d) => (d as HierarchyNode).value || 0)
      .sort((a, b) => (b.value || 0) - (a.value || 0));

    d3.treemap<HierarchyNode>()
      .size([width, height])
      .paddingOuter(3)
      .paddingTop(18)
      .paddingInner(1)
      .round(true)(hierarchy);

    return hierarchy;
  }, [hierarchyData, width, height]);

  const matchesSearch = useCallback(
    (id: string) => {
      if (!state.searchQuery) return true;
      return id.toLowerCase().includes(state.searchQuery.toLowerCase());
    },
    [state.searchQuery]
  );

  const leaves = root.leaves();

  return (
    <div className="relative">
      <svg width={width} height={height}>
        {/* Module groups (inner nodes at depth 1) */}
        {root.children?.map((mod) => {
          const d = mod as d3.HierarchyRectangularNode<HierarchyNode>;
          return (
            <g key={d.data.name}>
              <rect
                x={d.x0}
                y={d.y0}
                width={d.x1 - d.x0}
                height={d.y1 - d.y0}
                fill="none"
                stroke="#334155"
                strokeWidth={1}
                className="cursor-pointer"
                onClick={() =>
                  dispatch({
                    type: "ZOOM_MODULE",
                    module:
                      state.treemapZoomModule === d.data.name
                        ? null
                        : d.data.name,
                  })
                }
              />
              <text
                x={d.x0 + 4}
                y={d.y0 + 13}
                className="fill-text-secondary text-[10px] font-sans font-medium pointer-events-none"
              >
                {d.data.name}
              </text>
            </g>
          );
        })}

        {/* File rectangles (leaves) */}
        {leaves.map((leaf) => {
          const d = leaf as d3.HierarchyRectangularNode<HierarchyNode>;
          const nodeData = d.data.nodeData;
          if (!nodeData) return null;

          const w = d.x1 - d.x0;
          const h = d.y1 - d.y0;
          if (w < 1 || h < 1) return null;

          const fillColor = getNodeColor(
            nodeData,
            state.treemapColorMode,
            derived.moduleColorMap,
            maxValues
          );
          const isSelected = state.selectedNode === nodeData.id;
          const isHovered = state.hoveredNode === nodeData.id;
          const matches = matchesSearch(nodeData.id);
          const opacity = matches ? 1 : 0.15;

          const signals = derived.signalIndex.byNode.get(nodeData.id);
          const showBadge =
            state.signalOverlay && signals && signals.length > 0 && w > 20 && h > 20;

          return (
            <g key={nodeData.id}>
              <rect
                x={d.x0}
                y={d.y0}
                width={w}
                height={h}
                fill={fillColor}
                stroke={isSelected ? "#ffffff" : isHovered ? "#94a3b8" : "none"}
                strokeWidth={isSelected ? 2 : 1}
                opacity={opacity}
                className="cursor-pointer transition-all duration-300"
                onClick={() =>
                  dispatch({ type: "SELECT_NODE", nodeId: nodeData.id })
                }
                onMouseEnter={(e) => {
                  dispatch({ type: "HOVER_NODE", nodeId: nodeData.id });
                  setTooltip({ x: e.clientX, y: e.clientY, node: nodeData });
                }}
                onMouseMove={(e) =>
                  setTooltip({ x: e.clientX, y: e.clientY, node: nodeData })
                }
                onMouseLeave={() => {
                  dispatch({ type: "HOVER_NODE", nodeId: null });
                  setTooltip(null);
                }}
              />
              {w > 40 && h > 14 && (
                <text
                  x={d.x0 + 3}
                  y={d.y0 + 11}
                  className="fill-text-primary text-[9px] pointer-events-none"
                  opacity={opacity}
                >
                  {fileName(nodeData.id).slice(0, Math.floor(w / 6))}
                </text>
              )}
              {showBadge && (
                <text
                  x={d.x1 - 14}
                  y={d.y0 + 13}
                  className="text-[10px] pointer-events-none"
                  fill={
                    SIGNAL_COLORS[signals![0].type as SignalType] || "#ef4444"
                  }
                >
                  {SIGNAL_ICONS[signals![0].type as SignalType] || "!"}
                </text>
              )}
            </g>
          );
        })}
      </svg>

      {tooltip && (
        <Tooltip x={tooltip.x} y={tooltip.y}>
          <div className="space-y-1">
            <div className="font-medium">{tooltip.node.id}</div>
            <div className="text-text-secondary">
              LOC: {tooltip.node.loc} | Complexity: {tooltip.node.complexity}
            </div>
            <div className="text-text-secondary">
              Hotspot: {tooltip.node.hotspot.toFixed(2)} | Churn:{" "}
              {tooltip.node.churn_rate.toFixed(2)}
            </div>
            <div className="text-text-secondary">
              Bugs: {tooltip.node.bug_count} | Fan-in: {tooltip.node.fan_in} |
              Fan-out: {tooltip.node.fan_out}
            </div>
          </div>
        </Tooltip>
      )}
    </div>
  );
}
