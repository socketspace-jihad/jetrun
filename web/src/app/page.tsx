"use client";

import { Card, CardContent, CardTitle } from "@/components/ui/card";
import { StatusBadge } from "@/components/status-badge";
import { Badge } from "@/components/ui/badge";
import {
  Zap,
  CheckCircle,
  XCircle,
  Clock,
  TrendingUp,
  Database,
  GitBranch,
} from "lucide-react";
import type { BuildStatus } from "@/types";

// Mock data for the dashboard
const recentBuilds = [
  {
    id: "1",
    pipeline: "api-service",
    number: 142,
    status: "success" as BuildStatus,
    branch: "main",
    commit: "a3f9c1d",
    duration: "1m 23s",
    time: "2m ago",
  },
  {
    id: "2",
    pipeline: "web-frontend",
    number: 89,
    status: "running" as BuildStatus,
    branch: "feat/auth",
    commit: "e7b2f4a",
    duration: "—",
    time: "just now",
  },
  {
    id: "3",
    pipeline: "worker-service",
    number: 67,
    status: "failed" as BuildStatus,
    branch: "fix/timeout",
    commit: "9d1c3e8",
    duration: "45s",
    time: "15m ago",
  },
  {
    id: "4",
    pipeline: "api-service",
    number: 141,
    status: "success" as BuildStatus,
    branch: "main",
    commit: "f2a8b7c",
    duration: "1m 18s",
    time: "1h ago",
  },
  {
    id: "5",
    pipeline: "deploy-prod",
    number: 23,
    status: "queued" as BuildStatus,
    branch: "main",
    commit: "a3f9c1d",
    duration: "—",
    time: "just now",
  },
];

const stats = [
  {
    label: "Total Builds",
    value: "1,247",
    icon: Zap,
    change: "+12%",
    color: "bg-nb-yellow",
  },
  {
    label: "Success Rate",
    value: "94.2%",
    icon: CheckCircle,
    change: "+2.1%",
    color: "bg-nb-green",
  },
  {
    label: "Avg Duration",
    value: "1m 34s",
    icon: Clock,
    change: "-18%",
    color: "bg-nb-blue",
  },
  {
    label: "Cache Hit Rate",
    value: "87.5%",
    icon: Database,
    change: "+5.3%",
    color: "bg-nb-purple",
  },
];

export default function DashboardPage() {
  return (
    <div className="p-8">
      {/* Header */}
      <div className="mb-8">
        <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
          Dashboard
        </h1>
        <p className="text-[13px] text-nb-gray mt-1">
          Build system overview and recent activity
        </p>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-4 gap-5 mb-8">
        {stats.map((stat) => {
          const Icon = stat.icon;
          return (
            <Card key={stat.label}>
              <div className="flex items-start justify-between">
                <div>
                  <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">
                    {stat.label}
                  </p>
                  <p className="text-[24px] font-black text-nb-black">
                    {stat.value}
                  </p>
                </div>
                <div
                  className={`w-10 h-10 ${stat.color} border-2 border-nb-black rounded-xl shadow-neo-sm flex items-center justify-center`}
                >
                  <Icon className="w-5 h-5 text-white" />
                </div>
              </div>
              <div className="flex items-center gap-1 mt-2">
                <TrendingUp className="w-3 h-3 text-nb-green" />
                <span className="text-[11px] font-bold text-nb-green">
                  {stat.change}
                </span>
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
          <div className="space-y-1">
            {/* Header */}
            <div className="grid grid-cols-[1fr_100px_120px_100px_80px_80px] gap-4 px-3 py-2 text-[10px] font-black uppercase tracking-widest text-nb-gray">
              <span>Pipeline</span>
              <span>Status</span>
              <span>Branch</span>
              <span>Commit</span>
              <span>Duration</span>
              <span className="text-right">Time</span>
            </div>
            {/* Rows */}
            {recentBuilds.map((build) => (
              <div
                key={build.id}
                className="grid grid-cols-[1fr_100px_120px_100px_80px_80px] gap-4 px-3 py-2.5 rounded-xl hover:bg-nb-bg transition-colors cursor-pointer items-center"
              >
                <div className="flex items-center gap-2">
                  <span className="font-black text-[13px] text-nb-black">
                    {build.pipeline}
                  </span>
                  <span className="text-[11px] text-nb-gray font-bold">
                    #{build.number}
                  </span>
                </div>
                <StatusBadge status={build.status} />
                <span className="flex items-center gap-1 text-[12px] text-nb-gray">
                  <GitBranch className="w-3 h-3" />
                  {build.branch}
                </span>
                <span className="font-mono text-[11px] text-nb-gray">
                  {build.commit}
                </span>
                <span className="text-[12px] text-nb-gray font-medium">
                  {build.duration}
                </span>
                <span className="text-[11px] text-nb-gray text-right">
                  {build.time}
                </span>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
