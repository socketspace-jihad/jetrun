"use client";

import { useEffect, useState } from "react";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Key, Plus, Copy, Trash2, Check } from "lucide-react";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED } from "@/lib/demo";

const demoKeys = [
  { id: "k1", name: "CI Pipeline Key", prefix: "jr_live_aBcDeFgH", scopes: ["build:trigger", "build:read"], created_at: "2024-02-15T10:00:00Z", last_used_at: "2024-03-10T14:30:00Z", expires_at: null, revoked_at: null },
  { id: "k2", name: "Read-Only", prefix: "jr_live_xYzWvUtS", scopes: ["build:read", "pipeline:read"], created_at: "2024-03-01T10:00:00Z", last_used_at: null, expires_at: "2024-06-01T00:00:00Z", revoked_at: null },
];

export default function ApiKeysPage() {
  const [keys, setKeys] = useState<any[]>(DEMO_ENABLED ? demoKeys : []);
  const [loading, setLoading] = useState(!DEMO_ENABLED);
  const [showCreate, setShowCreate] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [creating, setCreating] = useState(false);
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (DEMO_ENABLED) return;
    authApi.listApiKeys()
      .then((res) => setKeys(res.api_keys as any))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const handleCreate = async () => {
    if (!newKeyName.trim()) return;
    setCreating(true);
    try {
      const res = await authApi.createApiKey({ name: newKeyName });
      setCreatedKey(res.key);
      setNewKeyName("");
      // Reload key list
      const updated = await authApi.listApiKeys();
      setKeys(updated.api_keys as any);
    } catch (_) {}
    setCreating(false);
  };

  const handleRevoke = async (id: string) => {
    try {
      await authApi.revokeApiKey(id);
      setKeys((prev) => prev.map((k) => k.id === id ? { ...k, revoked_at: new Date().toISOString() } : k));
    } catch (_) {}
  };

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">API Keys</h1>
          <p className="text-[13px] text-nb-gray mt-1">Manage API keys for CI/CD integrations{DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}</p>
        </div>
        <Button onClick={() => { setShowCreate(!showCreate); setCreatedKey(null); }}>
          <Plus className="w-4 h-4 mr-2" />Create Key
        </Button>
      </div>

      {/* Created key banner */}
      {createdKey && (
        <Card className="mb-6 border-nb-green">
          <div className="flex items-start justify-between">
            <div>
              <p className="font-black text-[13px] text-nb-green uppercase tracking-wider mb-2">Key Created — Copy It Now</p>
              <code className="font-mono text-[13px] bg-nb-black text-nb-green px-4 py-2 rounded-lg block">{createdKey}</code>
              <p className="text-[11px] text-nb-gray mt-2">This key will not be shown again. Save it securely.</p>
            </div>
            <Button size="sm" variant="secondary" onClick={() => handleCopy(createdKey)}>
              {copied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
            </Button>
          </div>
        </Card>
      )}

      {/* Create form */}
      {showCreate && !createdKey && (
        <Card className="mb-6 border-nb-yellow">
          <CardTitle className="flex items-center gap-2 mb-4"><Key className="w-4 h-4" />New API Key</CardTitle>
          <CardContent>
            <div className="flex gap-4">
              <div className="flex-1">
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Key Name</label>
                <Input
                  value={newKeyName}
                  onChange={(e) => setNewKeyName(e.target.value)}
                  placeholder="e.g., CI Pipeline Key"
                  onKeyDown={(e) => { if (e.key === "Enter") handleCreate(); }}
                />
              </div>
              <div className="flex items-end">
                <Button onClick={handleCreate} disabled={creating || !newKeyName.trim()}>
                  {creating ? "Creating..." : "Generate"}
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Keys list */}
      <Card>
        <CardContent>
          {loading ? (
            <p className="text-center py-8 text-nb-gray text-[13px]">Loading keys...</p>
          ) : keys.length === 0 ? (
            <p className="text-center py-8 text-nb-gray text-[13px]">No API keys yet. Create one to get started.</p>
          ) : (
            <div className="space-y-3">
              {keys.map((key) => {
                const isRevoked = !!key.revoked_at;
                const isExpired = key.expires_at && new Date(key.expires_at) < new Date();

                return (
                  <div key={key.id} className={`flex items-center justify-between bg-nb-bg border rounded-xl px-4 py-3 ${isRevoked || isExpired ? "border-nb-light opacity-60" : "border-nb-light"}`}>
                    <div className="flex-1">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="font-black text-[13px]">{key.name}</span>
                        {isRevoked && <Badge variant="danger">Revoked</Badge>}
                        {isExpired && !isRevoked && <Badge variant="warning">Expired</Badge>}
                        {!isRevoked && !isExpired && <Badge variant="success">Active</Badge>}
                      </div>
                      <div className="flex items-center gap-3 text-[11px] text-nb-gray">
                        <code className="font-mono bg-nb-white px-2 py-0.5 rounded border border-nb-light">{key.prefix}...</code>
                        <span>Created: {new Date(key.created_at).toLocaleDateString()}</span>
                        {key.last_used_at && <span>Last used: {new Date(key.last_used_at).toLocaleDateString()}</span>}
                      </div>
                      {key.scopes?.length > 0 && (
                        <div className="flex gap-1 mt-1.5">
                          {key.scopes.map((scope: string) => (
                            <Badge key={scope} variant="muted" className="text-[8px]">{scope}</Badge>
                          ))}
                        </div>
                      )}
                    </div>
                    {!isRevoked && (
                      <button onClick={() => handleRevoke(key.id)} className="w-8 h-8 rounded-lg flex items-center justify-center text-nb-red hover:bg-nb-red/10 transition-colors">
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
