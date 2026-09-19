"use client";

import { useEffect, useState } from "react";
import { useParams } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  ArrowLeft,
  Play,
  GitBranch,
  ExternalLink,
  Copy,
  Check,
  Settings,
  Loader2,
  Zap,
  Webhook,
} from "lucide-react";
import Link from "next/link";
import { api } from "@/lib/api";
import { DEMO_ENABLED } from "@/lib/demo";

export default function PipelineDetailPage() {
  const params = useParams();
  const projectId = params.id as string;

  const [project, setProject] = useState<any>(null);
  const [loading, setLoading] = useState(true);
  const [triggering, setTriggering] = useState(false);
  const [triggerMsg, setTriggerMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const [copied, setCopied] = useState("");

  useEffect(() => {
    if (DEMO_ENABLED || !projectId) { setLoading(false); return; }
    api.getProject(projectId)
      .then((res) => setProject(res))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [projectId]);

  const handleTrigger = async () => {
    setTriggering(true);
    setTriggerMsg(null);
    try {
      const res = await api.triggerBuild(projectId);
      setTriggerMsg({ type: "success", text: res.message || "Build triggered!" });
    } catch (err) {
      setTriggerMsg({ type: "error", text: err instanceof Error ? err.message : "Failed to trigger" });
    }
    setTriggering(false);
  };

  const handleCopy = (text: string, key: string) => {
    navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(""), 2000);
  };

  if (loading) {
    return (
      <div className="p-8 flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 text-nb-gray animate-spin" />
      </div>
    );
  }

  if (!project || project.error) {
    return (
      <div className="p-8">
        <Link href="/pipelines" className="flex items-center gap-2 text-[13px] text-nb-gray hover:text-nb-black mb-4">
          <ArrowLeft className="w-4 h-4" />Back to Pipelines
        </Link>
        <Card><p className="text-nb-gray text-center py-12">Project not found</p></Card>
      </div>
    );
  }

  const webhookUrls = [
    { provider: "GitHub", url: `https://api.jetrun.devopsinstitute.id/api/v1/webhooks/github?project_id=${projectId}`, color: "bg-nb-black text-white" },
    { provider: "GitLab", url: `https://api.jetrun.devopsinstitute.id/api/v1/webhooks/gitlab?project_id=${projectId}`, color: "bg-nb-orange text-white" },
    { provider: "Bitbucket", url: `https://api.jetrun.devopsinstitute.id/api/v1/webhooks/bitbucket?project_id=${projectId}`, color: "bg-nb-blue text-white" },
  ];

  return (
    <div className="p-8">
      {/* Header */}
      <div className="flex items-center gap-3 mb-6">
        <Link href="/pipelines" className="w-9 h-9 rounded-xl border-2 border-nb-black flex items-center justify-center hover:bg-nb-bg transition-colors">
          <ArrowLeft className="w-4 h-4" />
        </Link>
        <div className="flex-1">
          <div className="flex items-center gap-3">
            <h1 className="font-black text-[24px] text-nb-black uppercase tracking-wider">{project.name}</h1>
          </div>
          <div className="flex items-center gap-4 mt-1 text-[12px] text-nb-gray">
            {project.repo_url && (
              <span className="flex items-center gap-1">
                <ExternalLink className="w-3 h-3" />
                {project.repo_url.replace("https://github.com/", "").replace(".git", "")}
              </span>
            )}
            <span className="flex items-center gap-1">
              <GitBranch className="w-3 h-3" />
              {project.branch || project.default_branch || "main"}
            </span>
          </div>
        </div>
        <Button onClick={handleTrigger} disabled={triggering}>
          <Play className="w-3.5 h-3.5 mr-1.5" />
          {triggering ? "Triggering..." : "Run Build"}
        </Button>
      </div>

      {triggerMsg && (
        <div className={`rounded-xl px-4 py-3 mb-6 text-[12px] font-bold border-2 ${triggerMsg.type === "success" ? "bg-nb-green/10 border-nb-green text-nb-green" : "bg-nb-red/10 border-nb-red text-nb-red"}`}>
          {triggerMsg.text}
        </div>
      )}

      <div className="grid grid-cols-2 gap-6">
        {/* Project Info */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-4">
            <Settings className="w-4 h-4" />Project Info
          </CardTitle>
          <CardContent>
            <div className="space-y-3">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">Repository</label>
                <p className="text-[13px] font-mono text-nb-black">{project.repo_url}</p>
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">Branch</label>
                <p className="text-[13px] text-nb-black">{project.branch || project.default_branch || "main"}</p>
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1">Config Path</label>
                <code className="text-[12px] font-mono bg-nb-bg px-2 py-1 rounded">{project.config_path || ".jetrun/pipeline.yaml"}</code>
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
          <CardTitle className="flex items-center gap-2 mb-4">
            <Webhook className="w-4 h-4" />Webhook Setup
          </CardTitle>
          <CardContent>
            <p className="text-[12px] text-nb-gray mb-4">Add one of these webhook URLs to your repository settings to enable automatic builds on push.</p>
            <div className="space-y-3">
              {webhookUrls.map((wh) => (
                <div key={wh.provider}>
                  <div className="flex items-center gap-2 mb-1.5">
                    <Badge variant="default" className={`text-[9px] ${wh.color} border-none`}>{wh.provider}</Badge>
                  </div>
                  <div className="flex items-center gap-2">
                    <code className="flex-1 font-mono text-[10px] bg-nb-bg px-3 py-2 rounded-lg text-nb-gray truncate">{wh.url}</code>
                    <button onClick={() => handleCopy(wh.url, wh.provider)} className="text-nb-gray hover:text-nb-black shrink-0">
                      {copied === wh.provider ? <Check className="w-3.5 h-3.5 text-nb-green" /> : <Copy className="w-3.5 h-3.5" />}
                    </button>
                  </div>
                </div>
              ))}
            </div>
            <div className="mt-4 pt-4 border-t border-nb-light">
              <p className="text-[10px] text-nb-gray"><strong>Content type:</strong> application/json</p>
              <p className="text-[10px] text-nb-gray"><strong>Events:</strong> Push, Pull Request</p>
            </div>
          </CardContent>
        </Card>

        {/* Pipeline Config */}
        <Card className="col-span-2">
          <CardTitle className="flex items-center gap-2 mb-4">
            <Zap className="w-4 h-4" />Pipeline
          </CardTitle>
          <CardContent>
            <div className="bg-nb-bg border border-nb-light rounded-xl px-5 py-4">
              <p className="text-[12px] text-nb-gray mb-2">Pipeline configuration is read from <code className="bg-nb-white px-2 py-0.5 rounded border border-nb-light">.jetrun/pipeline.yaml</code> in your repository.</p>
              <p className="text-[12px] text-nb-gray">Click <strong>"Run Build"</strong> to trigger a sync — jetrun will clone your repo, read the config, and start the build pipeline.</p>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
