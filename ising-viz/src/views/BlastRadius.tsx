import React, { useEffect, useRef, useMemo, useState } from "react";
import * as d3 from "d3";
import { useAppState, useAppDispatch } from "../state/context";
import { computeNeighborhood } from "../utils/graph";
import { LAYER_COLORS, LAYER_DASH, LAYER_WIDTH_FACTOR, SIGNAL_COLORS } from "../utils/colors";
import { fileName } from "../utils/format";
import { Tooltip } from "../components/Tooltip";
import type { VizExport, VizEdge, VizNode, SignalType } from "../types";
import type { DerivedData } from "../data/derived";

interface BlastRadiusProps {
  data: VizExport;
  derived: DerivedData;
  width: number;
  height: number;
}

interface SimNode extends d3.SimulationNodeDatum {
  id: string;
  nodeData: VizNode;
  isCenter: boolean;
}

interface SimLink extends d3.SimulationLinkDatum<SimNode> {
  edgeData: VizEdge;
}

export function BlastRadius({
  data,
  derived,
  width,
  height,
}: BlastRadiusProps) {
  const state = useAppState();
  const dispatch = useAppDispatch();
  const svgRef = useRef<SVGSVGElement>(null);
  const simulationRef = useRef<d3.Simulation<SimNode, SimLink> | null>(null);
  const [tooltip, setTooltip] = useState<{
    x: number;
    y: number;
    content: string;
  } | null>(null);

  const centerId = state.selectedNode;

  const neighborhood = useMemo(() => {
    if (!centerId) return null;
    return computeNeighborhood(
      centerId,
      state.blastDepth,
      derived.nodeMap,
      data.edges,
      state.activeLayers
    );
  }, [centerId, state.blastDepth, derived.nodeMap, data.edges, state.activeLayers]);

  useEffect(() => {
    if (!svgRef.current || !neighborhood || !centerId) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll("*").remove();

    const g = svg.append("g");

    // Zoom
    const zoom = d3
      .zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.3, 3])
      .on("zoom", (event) => {
        g.attr("transform", event.transform);
      });
    svg.call(zoom);

    const simNodes: SimNode[] = neighborhood.nodes.map((n) => ({
      id: n.id,
      nodeData: n,
      isCenter: n.id === centerId,
      x: n.id === centerId ? width / 2 : undefined,
      y: n.id === centerId ? height / 2 : undefined,
      fx: n.id === centerId ? width / 2 : undefined,
      fy: n.id === centerId ? height / 2 : undefined,
    }));

    const nodeById = new Map(simNodes.map((n) => [n.id, n]));

    const simLinks: SimLink[] = neighborhood.edges
      .map((e) => ({
        source: nodeById.get(e.source)!,
        target: nodeById.get(e.target)!,
        edgeData: e,
      }))
      .filter((l) => l.source && l.target);

    const simulation = d3
      .forceSimulation(simNodes)
      .force(
        "link",
        d3
          .forceLink(simLinks)
          .id((d) => (d as SimNode).id)
          .distance((d) => 80 / Math.max((d as SimLink).edgeData.weight, 0.1))
      )
      .force("charge", d3.forceManyBody().strength(-200))
      .force("center", d3.forceCenter(width / 2, height / 2))
      .alphaDecay(0.05);

    simulationRef.current = simulation;

    // Edges
    const link = g
      .append("g")
      .selectAll("line")
      .data(simLinks)
      .join("line")
      .attr("stroke", (d) => LAYER_COLORS[d.edgeData.layer as keyof typeof LAYER_COLORS] || "#64748b")
      .attr("stroke-dasharray", (d) => LAYER_DASH[d.edgeData.layer as keyof typeof LAYER_DASH] || "none")
      .attr("stroke-width", (d) => {
        const factor = LAYER_WIDTH_FACTOR[d.edgeData.layer as keyof typeof LAYER_WIDTH_FACTOR] || 1;
        return Math.max(d.edgeData.weight * factor, 1);
      })
      .attr("opacity", 0.7);

    // Add glow for fragile boundary signals
    const signalEdges = new Set<string>();
    for (const signal of data.signals) {
      if (signal.type === "fragile_boundary" && signal.node_b) {
        signalEdges.add(`${signal.node_a}-${signal.node_b}`);
        signalEdges.add(`${signal.node_b}-${signal.node_a}`);
      }
    }

    link.classed("edge-glow", (d) => {
      const src = (d.source as SimNode).id;
      const tgt = (d.target as SimNode).id;
      return signalEdges.has(`${src}-${tgt}`);
    });

    // Ghost coupling indicators
    const ghostEdges = new Set<string>();
    for (const signal of data.signals) {
      if (signal.type === "ghost_coupling" && signal.node_b) {
        ghostEdges.add(`${signal.node_a}-${signal.node_b}`);
        ghostEdges.add(`${signal.node_b}-${signal.node_a}`);
      }
    }

    // Nodes
    const nodeRadius = (n: SimNode) =>
      n.isCenter ? 16 : Math.max(Math.sqrt(n.nodeData.loc) * 0.8, 6);

    const node = g
      .append("g")
      .selectAll("g")
      .data(simNodes)
      .join("g")
      .attr("cursor", "pointer")
      .call(
        d3
          .drag<SVGGElement, SimNode>()
          .on("start", (event, d) => {
            if (!event.active) simulation.alphaTarget(0.3).restart();
            d.fx = d.x;
            d.fy = d.y;
          })
          .on("drag", (event, d) => {
            d.fx = event.x;
            d.fy = event.y;
          })
          .on("end", (event, d) => {
            if (!event.active) simulation.alphaTarget(0);
            if (!d.isCenter) {
              d.fx = null;
              d.fy = null;
            }
          }) as any
      );

    // Signal ring for ticking bombs
    const tickingBombs = new Set(
      data.signals
        .filter((s) => s.type === "ticking_bomb")
        .map((s) => s.node_a)
    );

    node
      .append("circle")
      .attr("r", (d) => nodeRadius(d) + 4)
      .attr("fill", "none")
      .attr("stroke", "#dc2626")
      .attr("stroke-width", 2)
      .attr("opacity", (d) => (tickingBombs.has(d.id) ? 1 : 0))
      .classed("signal-pulse", (d) => tickingBombs.has(d.id));

    node
      .append("circle")
      .attr("r", nodeRadius)
      .attr("fill", (d) => derived.moduleColorMap.get(d.nodeData.module) || "#6b7280")
      .attr("stroke", (d) => (d.isCenter ? "#ffffff" : "none"))
      .attr("stroke-width", (d) => (d.isCenter ? 2 : 0))
      .attr("opacity", (d) => (d.isCenter ? 1 : 0.85));

    // Labels
    node
      .append("text")
      .text((d) => fileName(d.id))
      .attr("dy", (d) => nodeRadius(d) + 12)
      .attr("text-anchor", "middle")
      .attr("fill", "#94a3b8")
      .attr("font-size", "9px")
      .attr("pointer-events", "none");

    // Interactions
    node.on("click", (_event, d) => {
      dispatch({ type: "SELECT_NODE", nodeId: d.id });
    });

    node
      .on("mouseenter", (event, d) => {
        setTooltip({
          x: event.clientX,
          y: event.clientY,
          content: `${d.nodeData.id}\nLOC: ${d.nodeData.loc} | Complexity: ${d.nodeData.complexity}\nHotspot: ${d.nodeData.hotspot.toFixed(2)} | Bugs: ${d.nodeData.bug_count}`,
        });
      })
      .on("mouseleave", () => setTooltip(null));

    link
      .on("mouseenter", (event, d) => {
        setTooltip({
          x: event.clientX,
          y: event.clientY,
          content: `${d.edgeData.layer} | ${d.edgeData.type}\nWeight: ${d.edgeData.weight.toFixed(2)}`,
        });
      })
      .on("mouseleave", () => setTooltip(null));

    simulation.on("tick", () => {
      link
        .attr("x1", (d) => (d.source as SimNode).x!)
        .attr("y1", (d) => (d.source as SimNode).y!)
        .attr("x2", (d) => (d.target as SimNode).x!)
        .attr("y2", (d) => (d.target as SimNode).y!);

      node.attr("transform", (d) => `translate(${d.x},${d.y})`);
    });

    return () => {
      simulation.stop();
    };
  }, [neighborhood, centerId, width, height, data.signals, derived.moduleColorMap, dispatch]);

  if (!centerId) {
    return (
      <div className="flex items-center justify-center h-full text-text-muted text-sm">
        Click a file to see its blast radius
      </div>
    );
  }

  return (
    <div className="relative">
      <svg ref={svgRef} width={width} height={height} />
      {tooltip && (
        <Tooltip x={tooltip.x} y={tooltip.y}>
          {tooltip.content.split("\n").map((line, i) => (
            <div key={i} className={i > 0 ? "text-text-secondary" : "font-medium"}>
              {line}
            </div>
          ))}
        </Tooltip>
      )}
    </div>
  );
}
