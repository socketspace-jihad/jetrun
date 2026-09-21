"use client";

import { useEffect, useRef, useState, useCallback } from "react";
import { useParams } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { StatusBadge } from "@/components/status-badge";
import { BuildLogViewer } from "@/components/build-log-viewer";
import {
  ArrowLeft, Play, GitBranch, ExternalLink, Copy, Check,
  Loader2, Zap, Webhook, ChevronRight, Clock, GitCommit, Settings, List, Terminal,
} from "lucide-react";
import Link from "next/link";
import { api } from "@/lib/api";
import { DEMO_ENABLED } from "@/lib/demo";
import { cn } from "@/lib/utils";

type Tab = "builds" | "config";

export default function PipelineDetailPage() {
  const params = useParams();
  const projectId = params.id as string;

  const [project, setProject] = useState<any>(null);
  const [builds, setBuilds] = useState<any[]>([]);
  const [selectedBuild, setSelectedBuild] = useState<any>(null);
  const [tab, setTab] = useState<Tab>("builds");
  const [loading, setLoading] = useState(true);
  const [triggering, setTriggering] = useState(false);
  const [triggerMsg, setTriggerMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const [copied, setCopied] = useState("");
  const [logLines, setLogLines] = useState<{ line_number: number; stream: "stdout" | "stderr" | "system"; content: string; timestamp: string }[]>([]);
  const [logSource, setLogSource] = useState<string>("");
  const [showLogs, setShowLogs] = useState(false);
  const logPollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchLogs = useCallback(async (buildId: string) => {
    try {
      const res = await api.getBuildLogs(buildId);
      setLogSource(res.source);
      if (!res.content) { setLogLines([]); return; }
      const lines = res.content.split("\n").filter(Boolean).map((line, i) => {
        const match = line.match(/^\[([^\]]+)\]\s*\[([^\]]+)\]\s*\[(stdout|stderr)\]\s*(.*)/);
        if (match) {
          return { line_number: i + 1, timestamp: match[1], stream: match[3] as "stdout" | "stderr", content: match[4] };
        }
        return { line_number: i + 1, timestamp: "", stream: "stdout" as const, content: line };
      });
      setLogLines(lines);
    } catch {
      setLogLines([]);
    }
  }, []);

  // Poll logs for running builds
  useEffect(() => {
    if (logPollRef.current) { clearInterval(logPollRef.current); logPollRef.current = null; }
    if (!selectedBuild || !showLogs) return;
    fetchLogs(selectedBuild.id);
    if (selectedBuild.status === "running" || selectedBuild.status === "queued") {
      logPollRef.current = setInterval(() => fetchLogs(selectedBuild.id), 3000);
    }
    return () => { if (logPollRef.current) clearInterval(logPollRef.current); };
  }, [selectedBuild?.id, selectedBuild?.status, showLogs, fetchLogs]);

  const reload = async () => {
    try {
      const [proj, buildRes] = await Promise.all([
        api.getProject(projectId),
        api.listProjectBuilds(projectId),
      ]);
      setProject(proj);
      const buildList = (buildRes.builds as any[]) || [];
      setBuilds(buildList);
      if (buildList.length > 0 && !selectedBuild) setSelectedBuild(buildList[0]);
    } catch {}
  };

  useEffect(() => {
    if (DEMO_ENABLED || !projectId) { setLoading(false); return; }
    reload().finally(() => setLoading(false));
  }, [projectId]);

  const handleTrigger = async () => {
    setTriggering(true);
    setTriggerMsg(null);
    try {
      await api.triggerBuild(projectId);
      setTriggerMsg({ type: "success", text: "Build triggered! Syncing..." });
      setTimeout(() => reload(), 5000);
    } catch (err) {
      setTriggerMsg({ type: "error", text: err instanceof Error ? err.message : "Failed" });
    }
    setTriggering(false);
  };

  const handleCopy = (text: string, key: string) => {
    navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(""), 2000);
  };

  if (loading) return <div className="p-8 flex items-center justify-center h-64"><Loader2 className="w-8 h-8 text-nb-gray animate-spin" /></div>;
  if (!project || project.error) return (
    <div className="p-8">
      <Link href="/pipelines" className="flex items-center gap-2 text-[13px] text-nb-gray hover:text-nb-black mb-4"><ArrowLeft className="w-4 h-4" />Back</Link>
      <Card><p className="text-nb-gray text-center py-12">Project not found</p></Card>
    </div>
  );

  const webhookUrls = [
    { provider: "GitHub", url: `https://api.jetrun.devopsinstitute.id/api/v1/webhooks/github?project_id=${projectId}` },
    { provider: "GitLab", url: `https://api.jetrun.devopsinstitute.id/api/v1/webhooks/gitlab?project_id=${projectId}` },
    { provider: "Bitbucket", url: `https://api.jetrun.devopsinstitute.id/api/v1/webhooks/bitbucket?project_id=${projectId}` },
  ];

  return (
    <div className="p-8">
      {/* Header */}
      <div className="flex items-center gap-3 mb-6">
        <Link href="/pipelines" className="w-9 h-9 rounded-xl border-2 border-nb-black flex items-center justify-center hover:bg-nb-bg transition-colors">
          <ArrowLeft className="w-4 h-4" />
        </Link>
        <div className="flex-1">
          <h1 className="font-black text-[24px] text-nb-black uppercase tracking-wider">{project.name}</h1>
          <div className="flex items-center gap-4 mt-1 text-[12px] text-nb-gray">
            {project.repo_url && <span className="flex items-center gap-1"><ExternalLink className="w-3 h-3" />{project.repo_url.replace("https://github.com/", "").replace("git@github.com:", "").replace(".git", "")}</span>}
            <span className="flex items-center gap-1"><GitBranch className="w-3 h-3" />{project.branch || project.default_branch || "main"}</span>
          </div>
        </div>
        <Button onClick={handleTrigger} disabled={triggering}>
          <Play className="w-3.5 h-3.5 mr-1.5" />{triggering ? "Triggering..." : "Run Build"}
        </Button>
      </div>

      {triggerMsg && (
        <div className={`rounded-xl px-4 py-3 mb-6 text-[12px] font-bold border-2 ${triggerMsg.type === "success" ? "bg-nb-green/10 border-nb-green text-nb-green" : "bg-nb-red/10 border-nb-red text-nb-red"}`}>{triggerMsg.text}</div>
      )}

      {/* Tabs */}
      <div className="flex gap-1 mb-6 border-b-2 border-nb-light">
        {[
          { key: "builds" as Tab, label: "Builds", icon: List },
          { key: "config" as Tab, label: "Configuration", icon: Settings },
        ].map((t) => {
          const Icon = t.icon;
          return (
            <button
              key={t.key}
              onClick={() => setTab(t.key)}
              className={cn(
                "flex items-center gap-2 px-5 py-3 text-[12px] font-black uppercase tracking-wider border-b-2 -mb-[2px] transition-colors",
                tab === t.key
                  ? "border-nb-yellow text-nb-black"
                  : "border-transparent text-nb-gray hover:text-nb-black"
              )}
            >
              <Icon className="w-4 h-4" />{t.label}
              {t.key === "builds" && builds.length > 0 && (
                <Badge variant="default" className="text-[9px] px-1.5 py-0 ml-1">{builds.length}</Badge>
              )}
            </button>
          );
        })}
      </div>

      {/* ── Builds Tab ── */}
      {tab === "builds" && (
        <div className="grid grid-cols-[1fr_320px] gap-6">
          <div className="space-y-6">
            {/* Selected build stages */}
            {selectedBuild && selectedBuild.stages && selectedBuild.stages.length > 0 && (
              <Card>
                <CardTitle className="mb-4 flex items-center justify-between">
                  <span className="flex items-center gap-2"><Zap className="w-4 h-4" />Build #{selectedBuild.number}</span>
                  <StatusBadge status={selectedBuild.status} />
                </CardTitle>
                <CardContent>
                  <div className="flex items-center gap-3 overflow-x-auto pb-2">
                    {selectedBuild.stages.map((stage: any, i: number) => (
                      <div key={stage.id || i} className="flex items-center gap-3 shrink-0">
                        <div className="bg-nb-bg border-2 border-nb-black rounded-xl p-3 min-w-[180px]">
                          <div className="flex items-center justify-between mb-2">
                            <span className="font-black text-[12px] uppercase tracking-wider">{stage.name}</span>
                            <StatusBadge status={stage.status} />
                          </div>
                          {stage.steps?.map((step: any) => (
                            <div key={step.id || step.name} className="flex items-center justify-between text-[11px] mt-1">
                              <span className="text-nb-gray font-medium truncate mr-2">{step.name}</span>
                              <StatusBadge status={step.status} />
                            </div>
                          ))}
                        </div>
                        {i < selectedBuild.stages.length - 1 && <ChevronRight className="w-5 h-5 text-nb-gray shrink-0" />}
                      </div>
                    ))}
                  </div>
                  <div className="flex items-center gap-4 mt-4 pt-4 border-t border-nb-light text-[11px] text-nb-gray">
                    {selectedBuild.branch && <span className="flex items-center gap-1"><GitBranch className="w-3 h-3" />{selectedBuild.branch}</span>}
                    {selectedBuild.commit_sha && <span className="flex items-center gap-1"><GitCommit className="w-3 h-3" />{selectedBuild.commit_sha.slice(0, 7)}</span>}
                    <span className="flex items-center gap-1"><Clock className="w-3 h-3" />{new Date(selectedBuild.created_at).toLocaleString()}</span>
                  </div>
                </CardContent>
              </Card>
            )}

            {/* Build Logs */}
            {selectedBuild && (
              <div>
                <button
                  onClick={() => setShowLogs(!showLogs)}
                  className="flex items-center gap-2 mb-3 text-[12px] font-black uppercase tracking-wider text-nb-gray hover:text-nb-black transition-colors"
                >
                  <Terminal className="w-4 h-4" />
                  {showLogs ? "Hide Logs" : "Show Logs"}
                  {logSource === "live" && (
                    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-nb-green/15 text-nb-green text-[9px] font-black">
                      <span className="w-1.5 h-1.5 rounded-full bg-nb-green animate-pulse" />LIVE
                    </span>
                  )}
                </button>
                {showLogs && <BuildLogViewer logs={logLines} autoScroll={logSource === "live"} />}
              </div>
            )}

            {builds.length === 0 && (
              <Card>
                <div className="text-center py-12">
                  <Zap className="w-12 h-12 text-nb-light mx-auto mb-4" />
                  <p className="text-nb-gray text-[14px] font-bold mb-2">No builds yet</p>
                  <p className="text-nb-gray text-[12px] mb-4">Click &quot;Run Build&quot; or push to your repo.</p>
                  <Button onClick={handleTrigger} disabled={triggering} size="sm"><Play className="w-3.5 h-3.5 mr-1.5" />Run First Build</Button>
                </div>
              </Card>
            )}
          </div>

          {/* Sidebar: Build history */}
          <Card>
            <CardTitle className="mb-3 text-[13px]">History</CardTitle>
            <CardContent>
              {builds.length === 0 ? (
                <p className="text-[12px] text-nb-gray text-center py-4">No builds</p>
              ) : (
                <div className="space-y-2 max-h-[500px] overflow-y-auto custom-scrollbar">
                  {builds.map((b: any) => (
                    <button key={b.id} onClick={() => setSelectedBuild(b)}
                      className={cn("w-full text-left px-3 py-2.5 rounded-xl transition-all",
                        selectedBuild?.id === b.id ? "bg-nb-yellow/15 border border-nb-yellow/40" : "hover:bg-nb-bg border border-transparent"
                      )}>
                      <div className="flex items-center justify-between mb-1">
                        <span className="font-black text-[12px]">#{b.number}</span>
                        <StatusBadge status={b.status} />
                      </div>
                      <div className="flex items-center gap-2 text-[10px] text-nb-gray">
                        {b.branch && <span className="flex items-center gap-0.5"><GitBranch className="w-2.5 h-2.5" />{b.branch}</span>}
                        {b.commit_sha && <span className="font-mono">{b.commit_sha.slice(0, 7)}</span>}
                      </div>
                      <p className="text-[9px] text-nb-gray mt-0.5">{new Date(b.created_at).toLocaleString()}</p>
                    </button>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      )}

      {/* ── Configuration Tab ── */}
      {tab === "config" && (
        <div className="grid grid-cols-2 gap-6">
          {/* Project Info */}
          <Card>
            <CardTitle className="flex items-center gap-2 mb-4"><Settings className="w-4 h-4" />Project Info</CardTitle>
            <CardContent>
              <div className="space-y-3">
                <div>
                  <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">Repository</label>
                  <p className="text-[13px] font-mono text-nb-black break-all">{project.repo_url}</p>
                </div>
                <div>
                  <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">Branch</label>
                  <p className="text-[13px] text-nb-black">{project.branch || project.default_branch || "main"}</p>
                </div>
                <div>
                  <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">Config Path</label>
                  <code className="text-[12px] font-mono bg-nb-bg px-2 py-1 rounded border border-nb-light">{project.config_path || ".jetrun/pipeline.yaml"}</code>
                </div>
                <div>
                  <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">Created</label>
                  <p className="text-[12px] text-nb-gray">{project.created_at ? new Date(project.created_at).toLocaleString() : "—"}</p>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* Webhook Setup */}
          <Card>
            <CardTitle className="flex items-center gap-2 mb-4"><Webhook className="w-4 h-4" />Webhook Setup</CardTitle>
            <CardContent>
              <p className="text-[12px] text-nb-gray mb-4">Add one of these URLs to your repo webhook settings for auto-builds on push.</p>
              <div className="space-y-3">
                {webhookUrls.map((wh) => (
                  <div key={wh.provider}>
                    <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">{wh.provider}</p>
                    <div className="flex items-center gap-2">
                      <code className="flex-1 font-mono text-[10px] bg-nb-bg px-3 py-2 rounded-lg text-nb-gray truncate">{wh.url}</code>
                      <button onClick={() => handleCopy(wh.url, wh.provider)} className="shrink-0">
                        {copied === wh.provider ? <Check className="w-3.5 h-3.5 text-nb-green" /> : <Copy className="w-3.5 h-3.5 text-nb-gray hover:text-nb-black" />}
                      </button>
                    </div>
                  </div>
                ))}
              </div>
              <div className="mt-4 pt-4 border-t border-nb-light text-[10px] text-nb-gray">
                <p><strong>Content type:</strong> application/json</p>
                <p><strong>Events:</strong> Push, Pull Request</p>
              </div>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  );
}
