import type { VizExport } from "../types";

export async function loadVizData(): Promise<VizExport> {
  const params = new URLSearchParams(window.location.search);
  const dataPath = params.get("data") || "./sample-data.json";

  const response = await fetch(dataPath);
  if (!response.ok) {
    throw new Error(`Failed to load data from ${dataPath}: ${response.status}`);
  }
  return response.json();
}
