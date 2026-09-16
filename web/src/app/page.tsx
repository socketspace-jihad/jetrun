"use client";

import { useEffect, useState } from "react";
import { Card, CardContent, CardTitle } from "@/components/ui/card";
import { StatusBadge } from "@/components/status-badge";
import { Badge } from "@/components/ui/badge";
import {
  Zap,
  CheckCircle,
  Clock,
  TrendingUp,
  Database,
  GitBranch,
} from "lucide-react";
import type { BuildStatus } from "@/types";
import { api } from "@/lib/api";
import { DEMO_ENABLED, demoRecentBuilds, demoStats } from "@/lib/demo";

const statIcons = [Zap, CheckCircle, Clock, Database];

export default function DashboardPage() {
  const [builds, setBuilds] = useState(DEMO_ENABLED ? demoRecentBuilds : []);
  const [stats, setStats] = useState(DEMO_ENABLED ? demoStats : []);
  const [loading, setLoading] = useState(!DEMO_ENABLED);

  useEffect(() => {
    if (DEMO_ENABLED) return;
    api.listBuilds()
      .then((res) => {
        const mapped = (res.builds as any[]).slice(0, 5).map((b) => ({
          id: b.id,
          pipeline: b.pipeline_id,
          number: b.number,
          status: b.status as BuildStatus,
          branch: b.branch || "—",
          commit: b.commit_sha?.slice(0, 7) || "—",
          duration: b.finished_at && b.started_at
            ? `${Math.round((new Date(b.finished_at).getTime() - new Date(b.started_at).getTime()) / 1000)}s`
            : "—",
          time: b.created_at,
        }));
        setBuilds(mapped);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
          Dashboard
        </h1>
        <p className="text-[13px] text-nb-gray mt-1">
          Build system overview and recent activity
          {DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo Mode</Badge>}
        </p>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-4 gap-5 mb-8">
        {stats.map((stat, i) => {
          const Icon = statIcons[i % statIcons.length];
          return (
            <Card key={stat.label}>
              <div className="flex items-start justify-between">
                <div>
                  <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">
                    {stat.label}
                  </p>
                  <p className="text-[24px] font-black text-nb-black">{stat.value}</p>
                </div>
                <div className={`w-10 h-10 ${stat.color} border-2 border-nb-black rounded-xl shadow-neo-sm flex items-center justify-center`}>
                  <Icon className="w-5 h-5 text-white" />
                </div>
              </div>
              <div className="flex items-center gap-1 mt-2">
                <TrendingUp className="w-3 h-3 text-nb-green" />
                <span className="text-[11px] font-bold text-nb-green">{stat.change}</span>
                <span className="text-[11px] text-nb-gray">vs last week</span>
              </div>
            </Card>
          );
        })}
      </div>

      {/* Recent Builds */}
      <Card>
        <CardTitle className="mb-5">Recent Builds</CardTitle>
        <CardContent>
          {loading ? (
            <p className="text-[13px] text-nb-gray py-8 text-center">Loading builds...</p>
          ) : builds.length === 0 ? (
            <p className="text-[13px] text-nb-gray py-8 text-center">No builds yet. Trigger your first build to see it here.</p>
          ) : (
            <div className="space-y-1">
              <div className="grid grid-cols-[1fr_100px_120px_100px_80px_80px] gap-4 px-3 py-2 text-[10px] font-black uppercase tracking-widest text-nb-gray">
                <span>Pipeline</span>
                <span>Status</span>
                <span>Branch</span>
                <span>Commit</span>
                <span>Duration</span>
                <span className="text-right">Time</span>
              </div>
              {builds.map((build) => (
                <div
                  key={build.id}
                  className="grid grid-cols-[1fr_100px_120px_100px_80px_80px] gap-4 px-3 py-2.5 rounded-xl hover:bg-nb-bg transition-colors cursor-pointer items-center"
                >
                  <div className="flex items-center gap-2">
                    <span className="font-black text-[13px] text-nb-black">{build.pipeline}</span>
                    <span className="text-[11px] text-nb-gray font-bold">#{build.number}</span>
                  </div>
                  <StatusBadge status={build.status} />
                  <span className="flex items-center gap-1 text-[12px] text-nb-gray">
                    <GitBranch className="w-3 h-3" />
                    {build.branch}
                  </span>
                  <span className="font-mono text-[11px] text-nb-gray">{build.commit}</span>
                  <span className="text-[12px] text-nb-gray font-medium">{build.duration}</span>
                  <span className="text-[11px] text-nb-gray text-right">{build.time}</span>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
