export interface VizMeta {
  repo: string;
  commit: string;
  built_at: string;
  time_window: string;
  file_count: number;
  signal_count: number;
}

export interface VizNode {
  id: string;
  type: "module" | "class" | "function" | "import";
  module: string;
  language: string | null;
  loc: number;
  complexity: number;
  nesting_depth: number;
  fan_in: number;
  fan_out: number;
  change_freq: number;
  churn_rate: number;
  hotspot: number;
  bug_count: number;
  fix_inducing_rate: number;
  defect_density: number;
  sum_coupling: number;
  last_changed: string | null;
}

export type SignalType =
  | "ticking_bomb"
  | "fragile_boundary"
  | "ghost_coupling"
  | "over_engineering"
  | "stable_core";

export interface VizSignal {
  type: SignalType;
  node_a: string;
  node_b: string | null;
  severity: number;
  detail: string;
  evidence: Record<string, unknown> | null;
}

export interface VizEdge {
  source: string;
  target: string;
  layer: "structural" | "change" | "defect";
  type: "imports" | "calls" | "co_changes" | "fault_propagates" | string;
  weight: number;
  metadata: Record<string, unknown> | null;
}

export interface VizExport {
  meta: VizMeta;
  nodes: VizNode[];
  edges: VizEdge[];
  signals: VizSignal[];
}

export type ColorMode =
  | "hotspot"
  | "defect"
  | "churn"
  | "module"
  | "coupling"
  | "age";

export type ViewType = "treemap" | "blast";

export interface AppState {
  selectedNode: string | null;
  hoveredNode: string | null;
  activeView: ViewType;
  treemapColorMode: ColorMode;
  signalOverlay: boolean;
  blastDepth: 1 | 2 | 3;
  activeLayers: {
    structural: boolean;
    change: boolean;
    defect: boolean;
  };
  signalTypeFilter: Set<SignalType>;
  signalMinSeverity: number;
  searchQuery: string;
  treemapZoomModule: string | null;
}

export type AppAction =
  | { type: "SELECT_NODE"; nodeId: string | null }
  | { type: "HOVER_NODE"; nodeId: string | null }
  | { type: "SET_VIEW"; view: ViewType }
  | { type: "SET_COLOR_MODE"; mode: ColorMode }
  | { type: "TOGGLE_SIGNAL_OVERLAY" }
  | { type: "SET_BLAST_DEPTH"; depth: 1 | 2 | 3 }
  | { type: "TOGGLE_LAYER"; layer: "structural" | "change" | "defect" }
  | { type: "TOGGLE_SIGNAL_TYPE"; signalType: SignalType }
  | { type: "SET_SIGNAL_MIN_SEVERITY"; severity: number }
  | { type: "SET_SEARCH"; query: string }
  | { type: "ZOOM_MODULE"; module: string | null };
