import React, { useEffect, useState, useMemo, useRef, useCallback } from "react";
import { AppProvider, useAppState } from "./state/context";
import { loadVizData } from "./data/loader";
import { computeDerivedData, type DerivedData } from "./data/derived";
import { computeMaxValues } from "./utils/colors";
import { Header } from "./components/Header";
import { Treemap } from "./views/Treemap";
import { BlastRadius } from "./views/BlastRadius";
import { Sidebar } from "./views/Sidebar";
import type { VizExport } from "./types";

function AppContent() {
  const state = useAppState();
  const [data, setData] = useState<VizExport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const canvasRef = useRef<HTMLDivElement>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 800, height: 600 });

  useEffect(() => {
    loadVizData()
      .then(setData)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, []);

  const updateSize = useCallback(() => {
    if (canvasRef.current) {
      const rect = canvasRef.current.getBoundingClientRect();
      setCanvasSize({ width: rect.width, height: rect.height });
    }
  }, []);

  useEffect(() => {
    updateSize();
    window.addEventListener("resize", updateSize);
    return () => window.removeEventListener("resize", updateSize);
  }, [updateSize]);

  // Recompute after data loads
  useEffect(() => {
    if (data) updateSize();
  }, [data, updateSize]);

  const derived: DerivedData | null = useMemo(
    () => (data ? computeDerivedData(data) : null),
    [data]
  );

  const maxValues = useMemo(
    () => (data ? computeMaxValues(data.nodes) : {}),
    [data]
  );

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center text-text-muted">
        Loading graph data...
      </div>
    );
  }

  if (error) {
    return (
      <div className="h-full flex items-center justify-center text-red-400">
        <div className="text-center">
          <div className="text-lg mb-2">Failed to load data</div>
          <div className="text-sm text-text-muted">{error}</div>
          <div className="text-xs text-text-muted mt-4">
            Run <code className="bg-elevated px-1 rounded">ising export --format viz-json --output ising-viz-data.json</code> first,
            then open with <code className="bg-elevated px-1 rounded">?data=path/to/file.json</code>
          </div>
        </div>
      </div>
    );
  }

  if (!data || !derived) return null;

  return (
    <div className="h-full flex flex-col">
      <Header />
      <div className="flex flex-1 min-h-0">
        <div ref={canvasRef} className="flex-1 overflow-hidden">
          {state.activeView === "treemap" ? (
            <Treemap
              data={data}
              derived={derived}
              width={canvasSize.width}
              height={canvasSize.height}
            />
          ) : (
            <BlastRadius
              data={data}
              derived={derived}
              width={canvasSize.width}
              height={canvasSize.height}
            />
          )}
        </div>
        <Sidebar data={data} derived={derived} maxValues={maxValues} />
      </div>
    </div>
  );
}

export default function App() {
  return (
    <AppProvider>
      <AppContent />
    </AppProvider>
  );
}
