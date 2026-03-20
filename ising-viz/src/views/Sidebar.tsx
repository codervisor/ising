import React from "react";
import { FileDetail } from "./FileDetail";
import { SignalFeed } from "./SignalFeed";
import type { VizExport } from "../types";
import type { DerivedData } from "../data/derived";

interface SidebarProps {
  data: VizExport;
  derived: DerivedData;
  maxValues: Record<string, number>;
}

export function Sidebar({ data, derived, maxValues }: SidebarProps) {
  return (
    <div className="w-[340px] bg-surface border-l border-elevated flex flex-col shrink-0 h-full overflow-hidden">
      {/* V8: File Detail (top) */}
      <div className="border-b border-elevated overflow-y-auto max-h-[45%]">
        <div className="px-3 py-1.5 text-[10px] font-medium text-text-muted uppercase tracking-wider bg-surface sticky top-0">
          File Detail
        </div>
        <FileDetail data={data} derived={derived} maxValues={maxValues} />
      </div>

      {/* V3: Signal Feed (bottom) */}
      <div className="flex-1 overflow-hidden flex flex-col min-h-0">
        <div className="px-3 py-1.5 text-[10px] font-medium text-text-muted uppercase tracking-wider bg-surface shrink-0">
          Signals ({data.signals.length})
        </div>
        <div className="flex-1 min-h-0">
          <SignalFeed data={data} derived={derived} />
        </div>
      </div>
    </div>
  );
}
