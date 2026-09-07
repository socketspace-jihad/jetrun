"use client";

import { Button } from "@/components/ui/button";
import { PipelineCard } from "@/components/pipeline-card";
import { Input } from "@/components/ui/input";
import { Plus, Search } from "lucide-react";
import type { Pipeline, Build } from "@/types";
import { useState } from "react";

// Mock data
const mockPipelines: (Pipeline & { lastBuild?: Build })[] = [
  {
    id: "1",
    project_id: "p1",
    name: "api-service",
    description: "Main API service — build, test, deploy",
    config_path: ".jetrun/pipeline.yml",
    active: true,
    created_at: "2024-01-15T10:00:00Z",
    updated_at: "2024-03-10T14:30:00Z",
    lastBuild: {
      id: "b1",
      pipeline_id: "1",
      number: 142,
      status: "success",
      trigger: "push",
      commit_sha: "a3f9c1d82e",
      branch: "main",
      stages: [],
      started_at: "2024-03-10T14:28:00Z",
      finished_at: "2024-03-10T14:29:23Z",
      created_at: "2024-03-10T14:28:00Z",
    },
  },
  {
    id: "2",
    project_id: "p1",
    name: "web-frontend",
    description: "Next.js frontend build and preview deploy",
    config_path: ".jetrun/pipeline.yml",
    active: true,
    created_at: "2024-02-01T10:00:00Z",
    updated_at: "2024-03-10T15:00:00Z",
    lastBuild: {
      id: "b2",
      pipeline_id: "2",
      number: 89,
      status: "running",
      trigger: "push",
      commit_sha: "e7b2f4a91c",
      branch: "feat/auth",
      stages: [],
      started_at: "2024-03-10T15:00:00Z",
      finished_at: null,
      created_at: "2024-03-10T15:00:00Z",
    },
  },
  {
    id: "3",
    project_id: "p1",
    name: "worker-service",
    description: "Background worker service",
    config_path: ".jetrun/pipeline.yml",
    active: true,
    created_at: "2024-02-15T10:00:00Z",
    updated_at: "2024-03-10T13:00:00Z",
    lastBuild: {
      id: "b3",
      pipeline_id: "3",
      number: 67,
      status: "failed",
      trigger: "push",
      commit_sha: "9d1c3e8b4f",
      branch: "fix/timeout",
      stages: [],
      started_at: "2024-03-10T12:59:15Z",
      finished_at: "2024-03-10T13:00:00Z",
      created_at: "2024-03-10T12:59:15Z",
    },
  },
  {
    id: "4",
    project_id: "p2",
    name: "deploy-prod",
    description: "Production deployment pipeline",
    config_path: ".jetrun/deploy.yml",
    active: true,
    created_at: "2024-01-20T10:00:00Z",
    updated_at: "2024-03-09T10:00:00Z",
  },
];

export default function PipelinesPage() {
  const [search, setSearch] = useState("");

  const filtered = mockPipelines.filter((p) =>
    p.name.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div className="p-8">
      {/* Header */}
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
            Pipelines
          </h1>
          <p className="text-[13px] text-nb-gray mt-1">
            {mockPipelines.length} pipelines configured
          </p>
        </div>
        <Button size="md">
          <Plus className="w-4 h-4 mr-2" />
          New Pipeline
        </Button>
      </div>

      {/* Search */}
      <div className="relative mb-6">
        <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-4 h-4 text-nb-gray" />
        <Input
          placeholder="Search pipelines..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-11"
        />
      </div>

      {/* Pipeline Grid */}
      <div className="grid grid-cols-2 gap-5">
        {filtered.map((pipeline) => (
          <PipelineCard
            key={pipeline.id}
            pipeline={pipeline}
            lastBuild={pipeline.lastBuild}
          />
        ))}
      </div>

      {filtered.length === 0 && (
        <div className="text-center py-16">
          <p className="text-nb-gray text-[14px] font-bold">
            No pipelines found
          </p>
        </div>
      )}
    </div>
  );
}
