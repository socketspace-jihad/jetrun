"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { PipelineCard } from "@/components/pipeline-card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Plus, Search } from "lucide-react";
import type { Pipeline, Build } from "@/types";
import { api } from "@/lib/api";
import { DEMO_ENABLED, demoPipelines } from "@/lib/demo";

export default function PipelinesPage() {
  const [search, setSearch] = useState("");
  const [pipelines, setPipelines] = useState<(Pipeline & { lastBuild?: Build })[]>(
    DEMO_ENABLED ? (demoPipelines as any) : []
  );
  const [loading, setLoading] = useState(!DEMO_ENABLED);

  useEffect(() => {
    if (DEMO_ENABLED) return;
    api.listPipelines()
      .then((res) => setPipelines(res.pipelines as any))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const filtered = pipelines.filter((p) =>
    p.name.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
            Pipelines
          </h1>
          <p className="text-[13px] text-nb-gray mt-1">
            {pipelines.length} pipelines configured
            {DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}
          </p>
        </div>
        <Button size="md">
          <Plus className="w-4 h-4 mr-2" />
          New Pipeline
        </Button>
      </div>

      <div className="relative mb-6">
        <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-4 h-4 text-nb-gray" />
        <Input
          placeholder="Search pipelines..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-11"
        />
      </div>

      {loading ? (
        <p className="text-center py-16 text-nb-gray text-[14px]">Loading pipelines...</p>
      ) : (
        <div className="grid grid-cols-2 gap-5">
          {filtered.map((pipeline) => (
            <PipelineCard
              key={pipeline.id}
              pipeline={pipeline}
              lastBuild={pipeline.lastBuild}
            />
          ))}
        </div>
      )}

      {!loading && filtered.length === 0 && (
        <div className="text-center py-16">
          <p className="text-nb-gray text-[14px] font-bold">No pipelines found</p>
        </div>
      )}
    </div>
  );
}
