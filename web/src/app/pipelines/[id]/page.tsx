"use client";

import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { StatusBadge } from "@/components/status-badge";
import { BuildLogViewer } from "@/components/build-log-viewer";
import { Badge } from "@/components/ui/badge";
import { formatDuration } from "@/lib/utils";
import {
  ArrowLeft,
  Play,
  GitBranch,
  GitCommit,
  Clock,
  Database,
  ChevronRight,
} from "lucide-react";
import Link from "next/link";
import type { BuildStatus } from "@/types";

// Mock build detail data
const mockBuild = {
  id: "b1",
  pipeline: "api-service",
  number: 142,
  status: "success" as BuildStatus,
  trigger: "push",
  branch: "main",
  commit: "a3f9c1d82e4f",
  started_at: "2024-03-10T14:28:00Z",
  finished_at: "2024-03-10T14:29:23Z",
  stages: [
    {
      name: "Lint",
      status: "success" as BuildStatus,
      duration_ms: 12400,
      steps: [
        { name: "Check formatting", status: "success" as BuildStatus, duration_ms: 5200, cache_hit: false },
        { name: "Clippy", status: "success" as BuildStatus, duration_ms: 7200, cache_hit: true },
      ],
    },
    {
      name: "Test",
      status: "success" as BuildStatus,
      duration_ms: 45600,
      steps: [
        { name: "Unit tests", status: "success" as BuildStatus, duration_ms: 23400, cache_hit: true },
        { name: "Integration tests", status: "success" as BuildStatus, duration_ms: 22200, cache_hit: false },
      ],
    },
    {
      name: "Build",
      status: "success" as BuildStatus,
      duration_ms: 25300,
      steps: [
        { name: "Build release", status: "success" as BuildStatus, duration_ms: 25300, cache_hit: false },
      ],
    },
  ],
};

const mockLogs = Array.from({ length: 30 }, (_, i) => ({
  line_number: i + 1,
  stream: (i % 5 === 4 ? "stderr" : i % 10 === 0 ? "system" : "stdout") as
    | "stdout"
    | "stderr"
    | "system",
  content:
    i % 10 === 0
      ? `▸ Stage: ${["Lint", "Test", "Build"][Math.floor(i / 10)] || "Build"}`
      : i % 5 === 4
        ? "warning: unused variable `temp`"
        : `   Compiling jetrun-${["common", "gateway", "engine", "worker", "cache"][i % 5]} v0.1.0`,
  timestamp: new Date(Date.now() - (30 - i) * 1000).toISOString(),
}));

export default function PipelineDetailPage() {
  return (
    <div className="p-8">
      {/* Header */}
      <div className="flex items-center gap-3 mb-6">
        <Link
          href="/pipelines"
          className="w-9 h-9 rounded-xl border-2 border-nb-black flex items-center justify-center hover:bg-nb-bg transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
        </Link>
        <div className="flex-1">
          <div className="flex items-center gap-3">
            <h1 className="font-black text-[24px] text-nb-black uppercase tracking-wider">
              {mockBuild.pipeline}
            </h1>
            <span className="text-[14px] text-nb-gray font-bold">
              #{mockBuild.number}
            </span>
            <StatusBadge status={mockBuild.status} />
          </div>
          <div className="flex items-center gap-4 mt-1 text-[12px] text-nb-gray">
            <span className="flex items-center gap-1">
              <GitBranch className="w-3 h-3" />
              {mockBuild.branch}
            </span>
            <span className="flex items-center gap-1">
              <GitCommit className="w-3 h-3" />
              {mockBuild.commit}
            </span>
            <span className="flex items-center gap-1">
              <Clock className="w-3 h-3" />
              {formatDuration(
                new Date(mockBuild.finished_at).getTime() -
                  new Date(mockBuild.started_at).getTime()
              )}
            </span>
          </div>
        </div>
        <Button variant="secondary" size="sm">
          <Play className="w-3 h-3 mr-1.5" />
          Re-run
        </Button>
      </div>

      {/* Stage Pipeline Visualization */}
      <Card className="mb-6">
        <CardTitle className="mb-4">Stages</CardTitle>
        <CardContent>
          <div className="flex items-center gap-3">
            {mockBuild.stages.map((stage, i) => (
              <div key={stage.name} className="flex items-center gap-3">
                <div className="bg-nb-bg border-2 border-nb-black rounded-xl p-3 min-w-[180px]">
                  <div className="flex items-center justify-between mb-2">
                    <span className="font-black text-[12px] uppercase tracking-wider">
                      {stage.name}
                    </span>
                    <StatusBadge status={stage.status} />
                  </div>
                  <div className="space-y-1.5">
                    {stage.steps.map((step) => (
                      <div
                        key={step.name}
                        className="flex items-center justify-between text-[11px]"
                      >
                        <span className="text-nb-gray font-medium truncate mr-2">
                          {step.name}
                        </span>
                        <div className="flex items-center gap-1.5 shrink-0">
                          {step.cache_hit && (
                            <Badge variant="info" className="text-[8px] px-1.5 py-0">
                              <Database className="w-2 h-2 mr-0.5" />
                              Cache
                            </Badge>
                          )}
                          <span className="text-nb-gray font-mono text-[10px]">
                            {formatDuration(step.duration_ms)}
                          </span>
                        </div>
                      </div>
                    ))}
                  </div>
                  <div className="mt-2 pt-2 border-t border-nb-light text-[10px] text-nb-gray font-mono text-right">
                    {formatDuration(stage.duration_ms)}
                  </div>
                </div>
                {i < mockBuild.stages.length - 1 && (
                  <ChevronRight className="w-5 h-5 text-nb-gray shrink-0" />
                )}
              </div>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* Build Logs */}
      <BuildLogViewer logs={mockLogs} />
    </div>
  );
}
