import * as d3 from "d3";
import type { VizExport, VizNode, VizSignal, VizEdge } from "../types";

export interface TreemapHierarchy {
  name: string;
  children: {
    name: string;
    children: (VizNode & { value: number })[];
  }[];
}

export interface PercentileRanks {
  hotspot: Map<string, number>;
  complexity: Map<string, number>;
  churn: Map<string, number>;
  defect: Map<string, number>;
  coupling: Map<string, number>;
}

export interface SignalIndex {
  byNode: Map<string, VizSignal[]>;
  byType: Map<string, VizSignal[]>;
}

export interface DerivedData {
  treemapHierarchy: TreemapHierarchy;
  percentiles: PercentileRanks;
  signalIndex: SignalIndex;
  moduleColorMap: Map<string, string>;
  nodeMap: Map<string, VizNode>;
  edgeIndex: Map<string, VizEdge[]>;
}

const MODULE_COLORS: Record<string, string> = {
  auth: "#ef4444",
  api: "#3b82f6",
  db: "#10b981",
  events: "#f59e0b",
  middleware: "#8b5cf6",
  utils: "#6b7280",
  tests: "#06b6d4",
  config: "#d946ef",
};

function computePercentile(
  values: number[],
  nodeIds: string[]
): Map<string, number> {
  const sorted = [...values].sort((a, b) => a - b);
  const result = new Map<string, number>();
  for (let i = 0; i < values.length; i++) {
    const rank = sorted.indexOf(values[i]);
    result.set(nodeIds[i], rank / Math.max(sorted.length - 1, 1));
  }
  return result;
}

export function computeDerivedData(data: VizExport): DerivedData {
  const nodeMap = new Map<string, VizNode>();
  for (const node of data.nodes) {
    nodeMap.set(node.id, node);
  }

  // Treemap hierarchy: group by module
  const moduleGroups = new Map<string, VizNode[]>();
  for (const node of data.nodes) {
    const group = moduleGroups.get(node.module) || [];
    group.push(node);
    moduleGroups.set(node.module, group);
  }

  const treemapHierarchy: TreemapHierarchy = {
    name: "root",
    children: Array.from(moduleGroups.entries()).map(([mod, nodes]) => ({
      name: mod,
      children: nodes.map((n) => ({ ...n, value: Math.max(n.loc, 1) })),
    })),
  };

  // Percentile ranks
  const nodeIds = data.nodes.map((n) => n.id);
  const percentiles: PercentileRanks = {
    hotspot: computePercentile(
      data.nodes.map((n) => n.hotspot),
      nodeIds
    ),
    complexity: computePercentile(
      data.nodes.map((n) => n.complexity),
      nodeIds
    ),
    churn: computePercentile(
      data.nodes.map((n) => n.churn_rate),
      nodeIds
    ),
    defect: computePercentile(
      data.nodes.map((n) => n.defect_density),
      nodeIds
    ),
    coupling: computePercentile(
      data.nodes.map((n) => n.sum_coupling),
      nodeIds
    ),
  };

  // Signal index
  const signalIndex: SignalIndex = {
    byNode: new Map(),
    byType: new Map(),
  };
  for (const signal of data.signals) {
    // By node
    for (const nodeId of [signal.node_a, signal.node_b]) {
      if (nodeId) {
        const existing = signalIndex.byNode.get(nodeId) || [];
        existing.push(signal);
        signalIndex.byNode.set(nodeId, existing);
      }
    }
    // By type
    const typeList = signalIndex.byType.get(signal.type) || [];
    typeList.push(signal);
    signalIndex.byType.set(signal.type, typeList);
  }

  // Module color map
  const modules = Array.from(moduleGroups.keys());
  const moduleColorMap = new Map<string, string>();
  const tableau = d3.schemeTableau10;
  let tableauIdx = 0;
  for (const mod of modules) {
    if (MODULE_COLORS[mod]) {
      moduleColorMap.set(mod, MODULE_COLORS[mod]);
    } else {
      moduleColorMap.set(mod, tableau[tableauIdx % tableau.length]);
      tableauIdx++;
    }
  }

  // Edge index: nodeId -> edges involving that node
  const edgeIndex = new Map<string, VizEdge[]>();
  for (const edge of data.edges) {
    for (const nodeId of [edge.source, edge.target]) {
      const existing = edgeIndex.get(nodeId) || [];
      existing.push(edge);
      edgeIndex.set(nodeId, existing);
    }
  }

  return {
    treemapHierarchy,
    percentiles,
    signalIndex,
    moduleColorMap,
    nodeMap,
    edgeIndex,
  };
}
