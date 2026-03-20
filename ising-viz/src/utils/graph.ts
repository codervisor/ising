import type { VizEdge, VizNode } from "../types";

export interface NeighborhoodResult {
  nodes: VizNode[];
  edges: VizEdge[];
}

export function computeNeighborhood(
  centerId: string,
  depth: 1 | 2 | 3,
  allNodes: Map<string, VizNode>,
  allEdges: VizEdge[],
  activeLayers: { structural: boolean; change: boolean; defect: boolean },
  maxNodes: number = 50
): NeighborhoodResult {
  const nodeIds = new Set<string>([centerId]);
  const frontier = [centerId];

  const filteredEdges = allEdges.filter(
    (e) => activeLayers[e.layer as keyof typeof activeLayers]
  );

  for (let d = 0; d < depth; d++) {
    const nextFrontier: string[] = [];
    for (const nodeId of frontier) {
      for (const edge of filteredEdges) {
        if (edge.source === nodeId && !nodeIds.has(edge.target)) {
          nodeIds.add(edge.target);
          nextFrontier.push(edge.target);
        } else if (edge.target === nodeId && !nodeIds.has(edge.source)) {
          nodeIds.add(edge.source);
          nextFrontier.push(edge.source);
        }
      }
    }
    frontier.length = 0;
    frontier.push(...nextFrontier);
  }

  // Prune to maxNodes by combined edge weight
  if (nodeIds.size > maxNodes) {
    const weightMap = new Map<string, number>();
    for (const edge of filteredEdges) {
      if (nodeIds.has(edge.source) && nodeIds.has(edge.target)) {
        weightMap.set(
          edge.source,
          (weightMap.get(edge.source) || 0) + edge.weight
        );
        weightMap.set(
          edge.target,
          (weightMap.get(edge.target) || 0) + edge.weight
        );
      }
    }
    // Always keep center
    weightMap.set(centerId, Infinity);
    const sorted = Array.from(nodeIds)
      .map((id) => ({ id, w: weightMap.get(id) || 0 }))
      .sort((a, b) => b.w - a.w)
      .slice(0, maxNodes);
    nodeIds.clear();
    for (const { id } of sorted) nodeIds.add(id);
  }

  const nodes: VizNode[] = [];
  for (const id of nodeIds) {
    const node = allNodes.get(id);
    if (node) nodes.push(node);
  }

  const edges = filteredEdges.filter(
    (e) => nodeIds.has(e.source) && nodeIds.has(e.target)
  );

  return { nodes, edges };
}
