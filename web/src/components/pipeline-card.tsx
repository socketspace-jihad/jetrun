"use client";

import { Card } from "@/components/ui/card";
import { StatusBadge } from "@/components/status-badge";
import { formatDuration, timeAgo } from "@/lib/utils";
import type { Pipeline, Build } from "@/types";
import {
  GitBranch,
  GitCommit,
  Clock,
  Zap,
} from "lucide-react";
import Link from "next/link";

interface PipelineCardProps {
  pipeline: Pipeline;
  lastBuild?: Build;
}

export function PipelineCard({ pipeline, lastBuild }: PipelineCardProps) {
  return (
    <Link href={`/pipelines/${pipeline.id}`}>
      <Card className="hover:shadow-neo-lg hover:-translate-y-0.5 transition-all cursor-pointer">
        <div className="flex items-start justify-between mb-3">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 bg-nb-yellow border-2 border-nb-black rounded-xl shadow-neo-sm flex items-center justify-center">
              <Zap className="w-5 h-5 text-nb-black" />
            </div>
            <div>
              <h3 className="font-black text-[14px] text-nb-black">
                {pipeline.name}
              </h3>
              {pipeline.description && (
                <p className="text-[12px] text-nb-gray mt-0.5">
                  {pipeline.description}
                </p>
              )}
            </div>
          </div>
          {lastBuild && <StatusBadge status={lastBuild.status} />}
        </div>

        {lastBuild && (
          <div className="flex items-center gap-4 text-[11px] text-nb-gray mt-3 pt-3 border-t border-nb-light">
            <span className="flex items-center gap-1">
              <span className="font-black text-nb-black">#{lastBuild.number}</span>
            </span>
            {lastBuild.branch && (
              <span className="flex items-center gap-1">
                <GitBranch className="w-3 h-3" />
                {lastBuild.branch}
              </span>
            )}
            {lastBuild.commit_sha && (
              <span className="flex items-center gap-1">
                <GitCommit className="w-3 h-3" />
                {lastBuild.commit_sha.slice(0, 7)}
              </span>
            )}
            {lastBuild.started_at && lastBuild.finished_at && (
              <span className="flex items-center gap-1">
                <Clock className="w-3 h-3" />
                {formatDuration(
                  new Date(lastBuild.finished_at).getTime() -
                    new Date(lastBuild.started_at).getTime()
                )}
              </span>
            )}
            <span className="ml-auto">
              {timeAgo(lastBuild.created_at)}
            </span>
          </div>
        )}

        {!lastBuild && (
          <p className="text-[11px] text-nb-gray mt-2 pt-3 border-t border-nb-light">
            No builds yet
          </p>
        )}
      </Card>
    </Link>
  );
}
