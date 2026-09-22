"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Cpu, HardDrive, Database, Zap, Save, Loader2 } from "lucide-react";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";

type WorkerSettings = Record<string, string>;

const defaults: WorkerSettings = {
  "worker.max_parallel": "0",
  "worker.tmpfs_enabled": "true",
  "worker.tmpfs_size_mb": "4096",
  "worker.dep_cache_enabled": "true",
  "worker.cpu_pinning_enabled": "true",
  "worker.memory_limit_mb": "2048",
};

export default function WorkerSettingsPage() {
  const [settings, setSettings] = useState<WorkerSettings>(defaults);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.getWorkerSettings().then((res) => {
      setSettings({ ...defaults, ...res.settings } as WorkerSettings);
    }).catch(() => {}).finally(() => setLoading(false));
  }, []);

  const handleSave = async () => {
    setSaving(true);
    setSaved(false);
    try {
      await api.updateWorkerSettings(settings);
      setSaved(true);
      setTimeout(() => setSaved(false), 3000);
    } catch {}
    setSaving(false);
  };

  const toggle = (key: string) => {
    setSettings((s) => ({ ...s, [key]: s[key] === "true" ? "false" : "true" }));
  };

  const setNum = (key: string, value: string) => {
    setSettings((s) => ({ ...s, [key]: value }));
  };

  if (loading) return <div className="p-8 flex items-center justify-center h-64"><Loader2 className="w-8 h-8 text-nb-gray animate-spin" /></div>;

  return (
    <div className="p-8 max-w-3xl">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="font-black text-[24px] text-nb-black uppercase tracking-wider">Worker Settings</h1>
          <p className="text-[12px] text-nb-gray mt-1">Configure build worker performance. Changes apply on next build.</p>
        </div>
        <Button onClick={handleSave} disabled={saving}>
          {saving ? <Loader2 className="w-3.5 h-3.5 mr-1.5 animate-spin" /> : <Save className="w-3.5 h-3.5 mr-1.5" />}
          {saved ? "Saved!" : "Save"}
        </Button>
      </div>

      {saved && (
        <div className="rounded-xl px-4 py-3 mb-6 text-[12px] font-bold border-2 bg-nb-green/10 border-nb-green text-nb-green">
          Settings saved. Workers will pick up changes on next build.
        </div>
      )}

      <div className="space-y-6">
        {/* Concurrency */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-4"><Cpu className="w-4 h-4" />Concurrency</CardTitle>
          <CardContent>
            <div className="space-y-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Max Parallel Builds</label>
                <div className="flex items-center gap-3">
                  <input
                    type="number"
                    min={0}
                    value={settings["worker.max_parallel"]}
                    onChange={(e) => setNum("worker.max_parallel", e.target.value)}
                    className="w-24 px-3 py-2 rounded-xl border-2 border-nb-black text-[13px] font-bold text-center"
                  />
                  <span className="text-[11px] text-nb-gray">
                    {settings["worker.max_parallel"] === "0" ? "Auto-detect (= CPU cores)" : `Fixed: ${settings["worker.max_parallel"]} parallel`}
                  </span>
                </div>
                <p className="text-[10px] text-nb-gray mt-1">0 = auto-detect from available CPU cores. Reduces context switching.</p>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* tmpfs */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-4"><Zap className="w-4 h-4" />RAM Workspace (tmpfs)</CardTitle>
          <CardContent>
            <div className="space-y-4">
              <ToggleRow
                label="Build in RAM"
                description="Mount build workspace on tmpfs. Eliminates disk I/O during compilation. 2-5x faster for I/O-heavy builds."
                enabled={settings["worker.tmpfs_enabled"] === "true"}
                onToggle={() => toggle("worker.tmpfs_enabled")}
              />
              {settings["worker.tmpfs_enabled"] === "true" && (
                <div>
                  <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">tmpfs Size (MB)</label>
                  <input
                    type="number"
                    min={512}
                    step={512}
                    value={settings["worker.tmpfs_size_mb"]}
                    onChange={(e) => setNum("worker.tmpfs_size_mb", e.target.value)}
                    className="w-32 px-3 py-2 rounded-xl border-2 border-nb-black text-[13px] font-bold text-center"
                  />
                  <p className="text-[10px] text-nb-gray mt-1">Recommended: 2x your largest repository size.</p>
                </div>
              )}
            </div>
          </CardContent>
        </Card>

        {/* Dependency Cache */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-4"><Database className="w-4 h-4" />Dependency Cache</CardTitle>
          <CardContent>
            <ToggleRow
              label="Persistent Dependency Cache"
              description="Cache Go modules, npm packages, Cargo crates between builds. Skips download on subsequent builds."
              enabled={settings["worker.dep_cache_enabled"] === "true"}
              onToggle={() => toggle("worker.dep_cache_enabled")}
            />
          </CardContent>
        </Card>

        {/* CPU Pinning */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-4"><HardDrive className="w-4 h-4" />Resource Limits</CardTitle>
          <CardContent>
            <div className="space-y-4">
              <ToggleRow
                label="CPU Pinning (cgroups)"
                description="Pin build processes to dedicated CPU cores. Eliminates cache thrashing between parallel builds."
                enabled={settings["worker.cpu_pinning_enabled"] === "true"}
                onToggle={() => toggle("worker.cpu_pinning_enabled")}
              />
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Memory Limit per Build (MB)</label>
                <input
                  type="number"
                  min={256}
                  step={256}
                  value={settings["worker.memory_limit_mb"]}
                  onChange={(e) => setNum("worker.memory_limit_mb", e.target.value)}
                  className="w-32 px-3 py-2 rounded-xl border-2 border-nb-black text-[13px] font-bold text-center"
                />
                <p className="text-[10px] text-nb-gray mt-1">Prevents a single build from consuming all system memory.</p>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function ToggleRow({ label, description, enabled, onToggle }: {
  label: string; description: string; enabled: boolean; onToggle: () => void;
}) {
  return (
    <div className="flex items-center justify-between">
      <div className="flex-1 mr-4">
        <p className="text-[13px] font-bold text-nb-black">{label}</p>
        <p className="text-[10px] text-nb-gray mt-0.5">{description}</p>
      </div>
      <button
        onClick={onToggle}
        className={cn(
          "w-12 h-7 rounded-full border-2 border-nb-black transition-colors relative shrink-0",
          enabled ? "bg-nb-yellow" : "bg-nb-light"
        )}
      >
        <span className={cn(
          "absolute top-0.5 w-5 h-5 rounded-full bg-nb-black transition-transform",
          enabled ? "translate-x-5" : "translate-x-0.5"
        )} />
      </button>
    </div>
  );
}
