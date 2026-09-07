"use client";

import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

interface LogLine {
  line_number: number;
  stream: "stdout" | "stderr" | "system";
  content: string;
  timestamp: string;
}

interface BuildLogViewerProps {
  logs: LogLine[];
  autoScroll?: boolean;
}

export function BuildLogViewer({
  logs,
  autoScroll = true,
}: BuildLogViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [followOutput, setFollowOutput] = useState(autoScroll);

  useEffect(() => {
    if (followOutput && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [logs, followOutput]);

  const handleScroll = () => {
    if (!containerRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
    const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
    setFollowOutput(isAtBottom);
  };

  return (
    <div className="bg-nb-black border-2 border-nb-black rounded-xl overflow-hidden">
      <div className="flex items-center justify-between px-4 py-2 border-b border-white/10">
        <span className="text-white/40 font-bold uppercase tracking-wide text-[10px]">
          Build Output
        </span>
        <div className="flex items-center gap-2">
          <span className="text-white/30 text-[10px] font-mono">
            {logs.length} lines
          </span>
          <button
            onClick={() => setFollowOutput(!followOutput)}
            className={cn(
              "px-2 py-0.5 rounded text-[9px] font-black uppercase tracking-wider transition-colors",
              followOutput
                ? "bg-nb-yellow text-nb-black"
                : "bg-white/10 text-white/50 hover:text-white"
            )}
          >
            {followOutput ? "Following" : "Follow"}
          </button>
        </div>
      </div>
      <div
        ref={containerRef}
        onScroll={handleScroll}
        className="overflow-y-auto max-h-[600px] custom-scrollbar p-4"
      >
        {logs.length === 0 && (
          <p className="text-white/30 font-mono text-xs">
            Waiting for output...
          </p>
        )}
        {logs.map((log) => (
          <div key={log.line_number} className="flex gap-3 log-line group">
            <span className="text-white/20 select-none w-8 text-right shrink-0 font-mono">
              {log.line_number}
            </span>
            <span
              className={cn("font-mono whitespace-pre-wrap break-all", {
                "text-nb-bg": log.stream === "stdout",
                "text-nb-red": log.stream === "stderr",
                "text-nb-blue": log.stream === "system",
              })}
            >
              {log.content}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
