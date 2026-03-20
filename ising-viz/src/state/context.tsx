import React, { createContext, useContext, useReducer } from "react";
import type { AppState, AppAction, SignalType } from "../types";

const ALL_SIGNAL_TYPES: SignalType[] = [
  "ticking_bomb",
  "fragile_boundary",
  "ghost_coupling",
  "over_engineering",
  "stable_core",
];

const initialState: AppState = {
  selectedNode: null,
  hoveredNode: null,
  activeView: "treemap",
  treemapColorMode: "hotspot",
  signalOverlay: true,
  blastDepth: 2,
  activeLayers: {
    structural: true,
    change: true,
    defect: true,
  },
  signalTypeFilter: new Set(ALL_SIGNAL_TYPES),
  signalMinSeverity: 0,
  searchQuery: "",
  treemapZoomModule: null,
};

function appReducer(state: AppState, action: AppAction): AppState {
  switch (action.type) {
    case "SELECT_NODE":
      return { ...state, selectedNode: action.nodeId };
    case "HOVER_NODE":
      return { ...state, hoveredNode: action.nodeId };
    case "SET_VIEW":
      return { ...state, activeView: action.view };
    case "SET_COLOR_MODE":
      return { ...state, treemapColorMode: action.mode };
    case "TOGGLE_SIGNAL_OVERLAY":
      return { ...state, signalOverlay: !state.signalOverlay };
    case "SET_BLAST_DEPTH":
      return { ...state, blastDepth: action.depth };
    case "TOGGLE_LAYER":
      return {
        ...state,
        activeLayers: {
          ...state.activeLayers,
          [action.layer]: !state.activeLayers[action.layer],
        },
      };
    case "TOGGLE_SIGNAL_TYPE": {
      const next = new Set(state.signalTypeFilter);
      if (next.has(action.signalType)) {
        next.delete(action.signalType);
      } else {
        next.add(action.signalType);
      }
      return { ...state, signalTypeFilter: next };
    }
    case "SET_SIGNAL_MIN_SEVERITY":
      return { ...state, signalMinSeverity: action.severity };
    case "SET_SEARCH":
      return { ...state, searchQuery: action.query };
    case "ZOOM_MODULE":
      return { ...state, treemapZoomModule: action.module };
    default:
      return state;
  }
}

const AppStateContext = createContext<AppState>(initialState);
const AppDispatchContext = createContext<React.Dispatch<AppAction>>(() => {});

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(appReducer, initialState);
  return (
    <AppStateContext.Provider value={state}>
      <AppDispatchContext.Provider value={dispatch}>
        {children}
      </AppDispatchContext.Provider>
    </AppStateContext.Provider>
  );
}

export function useAppState() {
  return useContext(AppStateContext);
}

export function useAppDispatch() {
  return useContext(AppDispatchContext);
}
