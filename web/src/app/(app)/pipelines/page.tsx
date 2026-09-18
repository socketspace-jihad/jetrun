"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Plus, Search, GitBranch, ExternalLink, X, Copy, Check, Zap } from "lucide-react";
import { api } from "@/lib/api";
import { DEMO_ENABLED, demoPipelines } from "@/lib/demo";
import Link from "next/link";

export default function PipelinesPage() {
  const [search, setSearch] = useState("");
  const [projects, setProjects] = useState<any[]>(DEMO_ENABLED ? demoPipelines : []);
  const [loading, setLoading] = useState(!DEMO_ENABLED);

  const [showCreate, setShowCreate] = useState(false);
  const [name, setName] = useState("");
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("main");
  const [credentialId, setCredentialId] = useState("");
  const [secretNames, setSecretNames] = useState<{ id: string; name: string; type: string }[]>([]);
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<{ id: string; webhook_url: string } | null>(null);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);

  const reload = async () => {
    try {
      const res = await api.listProjects();
      setProjects(res.projects as any);
    } catch {}
  };

  useEffect(() => {
    if (DEMO_ENABLED) return;
    reload().finally(() => setLoading(false));
  }, []);

  // Load secret names when create form opens
  useEffect(() => {
    if (showCreate && !DEMO_ENABLED) {
      api.listSecretNames().then((res) => setSecretNames(res.secrets || [])).catch(() => {});
    }
  }, [showCreate]);

  const handleCreate = async () => {
    if (!name.trim() || !repoUrl.trim()) return;
    setCreating(true);
    setError("");
    try {
      const data: any = { name, repo_url: repoUrl, branch };
      if (credentialId) data.credential_id = credentialId;
      const res = await api.createProject(data);
      setCreated(res);
      setName(""); setRepoUrl(""); setBranch("main"); setCredentialId("");
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create project");
    }
    setCreating(false);
  };

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const filtered = projects.filter((p) =>
    (p.name || "").toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">Pipelines</h1>
          <p className="text-[13px] text-nb-gray mt-1">
            {projects.length} projects
            {DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}
          </p>
        </div>
        <Button onClick={() => { setShowCreate(!showCreate); setCreated(null); setError(""); }}>
          <Plus className="w-4 h-4 mr-2" />New Pipeline
        </Button>
      </div>

      {/* Created success */}
      {created && (
        <Card className="mb-6 border-nb-green">
          <div className="flex items-center justify-between mb-3">
            <p className="font-black text-[13px] text-nb-green uppercase tracking-wider">Project Created!</p>
            <button onClick={() => setCreated(null)} className="text-nb-gray hover:text-nb-black"><X className="w-4 h-4" /></button>
          </div>
          <p className="text-[12px] text-nb-gray mb-3">Add this webhook URL to your repository settings:</p>
          <div className="flex items-center gap-2">
            <code className="flex-1 font-mono text-[12px] bg-nb-black text-nb-green px-4 py-2.5 rounded-lg truncate">{created.webhook_url}</code>
            <Button size="sm" variant="secondary" onClick={() => handleCopy(created.webhook_url)}>
              {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
            </Button>
          </div>
          <p className="text-[10px] text-nb-gray mt-2">Content type: application/json · Events: Push, Pull Request</p>
        </Card>
      )}

      {/* Create form */}
      {showCreate && !created && (
        <Card className="mb-6 border-nb-yellow">
          <div className="flex items-center justify-between mb-4">
            <h3 className="font-black text-[14px] uppercase tracking-wider">New Project</h3>
            <button onClick={() => setShowCreate(false)} className="text-nb-gray hover:text-nb-black"><X className="w-4 h-4" /></button>
          </div>
          {error && <div className="bg-nb-red/10 border-2 border-nb-red text-nb-red rounded-xl px-4 py-3 mb-4 text-[12px] font-bold">{error}</div>}
          <div className="grid grid-cols-2 gap-4 mb-4">
            <div>
              <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Project Name</label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. my-api-service" />
            </div>
            <div>
              <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Branch</label>
              <Input value={branch} onChange={(e) => setBranch(e.target.value)} placeholder="main" />
            </div>
          </div>
          <div className="mb-4">
            <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Repository URL</label>
            <Input
              value={repoUrl} onChange={(e) => setRepoUrl(e.target.value)}
              placeholder="https://github.com/user/repo.git"
              onKeyDown={(e) => { if (e.key === "Enter" && name.trim() && repoUrl.trim()) handleCreate(); }}
            />
            <p className="text-[10px] text-nb-gray mt-1">HTTPS or SSH clone URL. Place <code className="bg-nb-bg px-1 rounded">.jetrun/pipeline.yaml</code> in your repo root.</p>
          </div>
          <div className="mb-4">
            <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Credential</label>
            <select
              value={credentialId}
              onChange={(e) => setCredentialId(e.target.value)}
              className="w-full px-4 py-3 bg-white border-2 border-nb-black rounded-xl text-[13px] font-medium focus:outline-none focus:shadow-neo-yellow focus:border-nb-yellow"
            >
              <option value="">None (public repository)</option>
              {secretNames.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name} ({s.type === "ssh_key" ? "SSH Key" : s.type === "token" ? "Token" : "Password"})
                </option>
              ))}
            </select>
            <p className="text-[10px] text-nb-gray mt-1">For private repos. Manage secrets in Settings → Secrets.</p>
          </div>
          <div className="flex justify-end">
            <Button onClick={handleCreate} disabled={creating || !name.trim() || !repoUrl.trim()}>
              {creating ? "Creating..." : "Create Project"}
            </Button>
          </div>
        </Card>
      )}

      {/* Search */}
      <div className="relative mb-6">
        <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-4 h-4 text-nb-gray" />
        <Input placeholder="Search projects..." value={search} onChange={(e) => setSearch(e.target.value)} className="pl-11" />
      </div>

      {/* Projects list */}
      {loading ? (
        <p className="text-center py-16 text-nb-gray text-[14px]">Loading projects...</p>
      ) : filtered.length === 0 ? (
        <div className="text-center py-16">
          <Zap className="w-12 h-12 text-nb-light mx-auto mb-4" />
          <p className="text-nb-gray text-[14px] font-bold mb-2">No projects yet</p>
          <p className="text-nb-gray text-[12px]">Create your first project to start building.</p>
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-5">
          {filtered.map((project) => (
            <Link key={project.id} href={`/pipelines/${project.id}`}>
              <Card className="hover:shadow-neo-lg hover:-translate-y-0.5 transition-all cursor-pointer">
                <div className="flex items-start justify-between mb-2">
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 bg-nb-yellow border-2 border-nb-black rounded-xl shadow-neo-sm flex items-center justify-center">
                      <Zap className="w-5 h-5 text-nb-black" />
                    </div>
                    <div>
                      <h3 className="font-black text-[14px] text-nb-black">{project.name}</h3>
                      {project.description && <p className="text-[12px] text-nb-gray mt-0.5">{project.description}</p>}
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-4 text-[11px] text-nb-gray mt-3 pt-3 border-t border-nb-light">
                  {project.repo_url && (
                    <span className="flex items-center gap-1 truncate max-w-[200px]">
                      <ExternalLink className="w-3 h-3 shrink-0" />
                      {project.repo_url.replace("https://github.com/", "").replace(".git", "")}
                    </span>
                  )}
                  {(project.branch || project.default_branch) && (
                    <span className="flex items-center gap-1">
                      <GitBranch className="w-3 h-3" />
                      {project.branch || project.default_branch}
                    </span>
                  )}
                </div>
              </Card>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
