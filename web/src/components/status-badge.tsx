"use client";

import { Badge } from "@/components/ui/badge";
import type { BuildStatus } from "@/types";
import {
  CheckCircle,
  XCircle,
  Clock,
  Loader2,
  Ban,
  SkipForward,
} from "lucide-react";

const statusConfig: Record<
  BuildStatus,
  { variant: "success" | "danger" | "warning" | "info" | "muted" | "default"; icon: React.ComponentType<{ className?: string }>; label: string }
> = {
  success: { variant: "success", icon: CheckCircle, label: "Success" },
  failed: { variant: "danger", icon: XCircle, label: "Failed" },
  running: { variant: "info", icon: Loader2, label: "Running" },
  queued: { variant: "warning", icon: Clock, label: "Queued" },
  cancelled: { variant: "muted", icon: Ban, label: "Cancelled" },
  skipped: { variant: "muted", icon: SkipForward, label: "Skipped" },
};

export function StatusBadge({ status }: { status: BuildStatus }) {
  const config = statusConfig[status];
  const Icon = config.icon;

  return (
    <Badge variant={config.variant}>
      <Icon
        className={`w-3 h-3 mr-1 ${status === "running" ? "animate-spin" : ""}`}
      />
      {config.label}
    </Badge>
  );
}
