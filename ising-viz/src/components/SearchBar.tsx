import React from "react";
import { useAppState, useAppDispatch } from "../state/context";

export function SearchBar() {
  const state = useAppState();
  const dispatch = useAppDispatch();

  return (
    <input
      type="text"
      placeholder="Search files..."
      value={state.searchQuery}
      onChange={(e) =>
        dispatch({ type: "SET_SEARCH", query: e.target.value })
      }
      className="bg-elevated border border-text-muted/20 rounded px-3 py-1 text-xs text-text-primary placeholder-text-muted w-full focus:outline-none focus:border-text-secondary"
    />
  );
}
