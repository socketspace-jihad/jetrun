"use client";

import { useEffect, useState } from "react";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Lock, Plus, Trash2, X, Copy, Check, Key, KeyRound } from "lucide-react";
import { api } from "@/lib/api";
import { DEMO_ENABLED } from "@/lib/demo";

export default function SecretsPage() {
  const [secrets, setSecrets] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);

  const [name, setName] = useState("");
  const [secretType, setSecretType] = useState("ssh_key");
  const [value, setValue] = useState("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");

  // Created SSH key display
  const [createdKey, setCreatedKey] = useState<{ name: string; public_key: string } | null>(null);
  const [copied, setCopied] = useState(false);

  const reload = async () => {
    try {
      const res = await api.listSecrets();
      setSecrets(res.secrets as any);
    } catch {}
  };

  useEffect(() => {
    if (DEMO_ENABLED) { setLoading(false); return; }
    reload().finally(() => setLoading(false));
  }, []);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setCreating(true);
    setError("");
    try {
      const data: any = { name, secret_type: secretType };
      if (secretType === "ssh_key") {
        data.generate = true;
      } else {
        if (!value.trim()) { setError("Value is required for token/password"); setCreating(false); return; }
        data.value = value;
      }
      const res = await api.createSecret(data);
      if (res.ssh_public_key) {
        setCreatedKey({ name, public_key: res.ssh_public_key });
      }
      setName(""); setValue(""); setShowCreate(false);
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create secret");
    }
    setCreating(false);
  };

  const handleDelete = async (id: string) => {
    try { await api.deleteSecret(id); await reload(); } catch {}
  };

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const typeLabel = (t: string) => {
    switch (t) { case "ssh_key": return "SSH Key"; case "token": return "Token"; case "password": return "Password"; default: return t; }
  };

  const typeBadge = (t: string) => {
    switch (t) { case "ssh_key": return "info"; case "token": return "warning"; case "password": return "muted"; default: return "default" as const; }
  };

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">Secrets</h1>
          <p className="text-[13px] text-nb-gray mt-1">Manage credentials for repository access. Engineers can use these when creating projects.</p>
        </div>
        <Button onClick={() => { setShowCreate(!showCreate); setCreatedKey(null); setError(""); }}>
          <Plus className="w-4 h-4 mr-2" />New Secret
        </Button>
      </div>

      {/* Created SSH key banner */}
      {createdKey && (
        <Card className="mb-6 border-nb-green">
          <div className="flex items-center justify-between mb-3">
            <p className="font-black text-[13px] text-nb-green uppercase tracking-wider">SSH Key Generated — {createdKey.name}</p>
            <button onClick={() => setCreatedKey(null)} className="text-nb-gray hover:text-nb-black"><X className="w-4 h-4" /></button>
          </div>
          <p className="text-[12px] text-nb-gray mb-3">Add this public key as a <strong>Deploy Key</strong> in your repository settings:</p>
          <div className="flex items-center gap-2">
            <code className="flex-1 font-mono text-[11px] bg-nb-black text-nb-green px-4 py-2.5 rounded-lg break-all">{createdKey.public_key}</code>
            <Button size="sm" variant="secondary" onClick={() => handleCopy(createdKey.public_key)}>
              {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
            </Button>
          </div>
          <p className="text-[10px] text-nb-gray mt-2">The private key is encrypted and stored securely. It will never be displayed.</p>
        </Card>
      )}

      {/* Create form */}
      {showCreate && !createdKey && (
        <Card className="mb-6 border-nb-yellow">
          <div className="flex items-center justify-between mb-4">
            <CardTitle className="flex items-center gap-2"><Lock className="w-4 h-4" />New Secret</CardTitle>
            <button onClick={() => setShowCreate(false)} className="text-nb-gray hover:text-nb-black"><X className="w-4 h-4" /></button>
          </div>
          <CardContent>
            {error && <div className="bg-nb-red/10 border-2 border-nb-red text-nb-red rounded-xl px-4 py-3 mb-4 text-[12px] font-bold">{error}</div>}

            <div className="grid grid-cols-2 gap-4 mb-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Name</label>
                <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. github-deploy-key" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Type</label>
                <select value={secretType} onChange={(e) => setSecretType(e.target.value)}
                  className="w-full px-4 py-3 bg-white border-2 border-nb-black rounded-xl text-[13px] font-medium focus:outline-none focus:shadow-neo-yellow focus:border-nb-yellow">
                  <option value="ssh_key">SSH Deploy Key (auto-generate)</option>
                  <option value="token">Personal Access Token</option>
                  <option value="password">Password / Secret</option>
                </select>
              </div>
            </div>

            {secretType !== "ssh_key" && (
              <div className="mb-4">
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  {secretType === "token" ? "Token Value" : "Password"}
                </label>
                <Input
                  type="password"
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
                  placeholder={secretType === "token" ? "ghp_xxxx... or glpat-xxxx..." : "Enter password"}
                />
                <p className="text-[10px] text-nb-gray mt-1">Encrypted at rest. Will never be displayed after saving.</p>
              </div>
            )}

            {secretType === "ssh_key" && (
              <div className="bg-nb-bg border border-nb-light rounded-xl px-4 py-3 mb-4">
                <p className="text-[12px] text-nb-black font-bold mb-1">Auto-Generate Ed25519 Key</p>
                <p className="text-[11px] text-nb-gray">A new SSH keypair will be generated. The public key will be shown once — add it as a Deploy Key in your repo. The private key is encrypted and stored securely.</p>
              </div>
            )}

            <div className="flex justify-end">
              <Button onClick={handleCreate} disabled={creating || !name.trim()}>
                {creating ? "Creating..." : secretType === "ssh_key" ? "Generate Key" : "Save Secret"}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Secrets list */}
      {loading ? (
        <p className="text-nb-gray text-[13px] text-center py-8">Loading secrets...</p>
      ) : secrets.length === 0 && !showCreate ? (
        <Card>
          <div className="text-center py-12">
            <KeyRound className="w-12 h-12 text-nb-light mx-auto mb-4" />
            <p className="text-nb-gray text-[14px] font-bold mb-2">No secrets yet</p>
            <p className="text-nb-gray text-[12px]">Create an SSH key or token to access private repositories.</p>
          </div>
        </Card>
      ) : (
        <div className="space-y-3">
          {secrets.map((s: any) => (
            <Card key={s.id}>
              <div className="flex items-start justify-between">
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    <h3 className="font-black text-[14px]">{s.name}</h3>
                    <Badge variant={typeBadge(s.secret_type) as any}>{typeLabel(s.secret_type)}</Badge>
                  </div>
                  {s.description && <p className="text-[12px] text-nb-gray">{s.description}</p>}

                  {/* SSH public key display */}
                  {s.ssh_public_key && (
                    <div className="mt-2 flex items-center gap-2">
                      <code className="font-mono text-[10px] bg-nb-bg px-3 py-1.5 rounded-lg text-nb-gray truncate max-w-[500px]">{s.ssh_public_key}</code>
                      <button onClick={() => handleCopy(s.ssh_public_key)} className="text-nb-gray hover:text-nb-black shrink-0">
                        <Copy className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  )}

                  <p className="text-[10px] text-nb-gray mt-2">Created {new Date(s.created_at).toLocaleDateString()}</p>
                </div>
                <Button variant="ghost" size="sm" onClick={() => handleDelete(s.id)}>
                  <Trash2 className="w-3.5 h-3.5 text-nb-red" />
                </Button>
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
