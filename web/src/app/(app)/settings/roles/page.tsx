"use client";

import { useEffect, useState } from "react";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Shield, Plus, Trash2, X, Check } from "lucide-react";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED } from "@/lib/demo";

export default function RolesPage() {
  const [roles, setRoles] = useState<any[]>([]);
  const [permissions, setPermissions] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);

  // Create form
  const [newRoleName, setNewRoleName] = useState("");
  const [newRoleDisplay, setNewRoleDisplay] = useState("");
  const [newRolePerms, setNewRolePerms] = useState<string[]>([]);
  const [creating, setCreating] = useState(false);
  const [msg, setMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);

  useEffect(() => {
    if (DEMO_ENABLED) { setLoading(false); return; }
    Promise.all([authApi.listRoles(), authApi.listPermissions()])
      .then(([r, p]) => {
        setRoles(r.roles as any);
        setPermissions(p.permissions as any);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const handleCreate = async () => {
    if (!newRoleName.trim() || !newRoleDisplay.trim()) return;
    setCreating(true);
    setMsg(null);
    try {
      await authApi.createRole({
        name: newRoleName.toLowerCase().replace(/\s+/g, "_"),
        display_name: newRoleDisplay,
        permissions: newRolePerms,
      });
      setMsg({ type: "success", text: "Role created" });
      setNewRoleName(""); setNewRoleDisplay(""); setNewRolePerms([]);
      setShowCreate(false);
      const updated = await authApi.listRoles();
      setRoles(updated.roles as any);
    } catch (err) {
      setMsg({ type: "error", text: err instanceof Error ? err.message : "Failed" });
    }
    setCreating(false);
  };

  const handleDelete = async (id: string) => {
    try {
      await authApi.deleteRole(id);
      setRoles((prev) => prev.filter((r) => r.id !== id));
    } catch {}
  };

  const togglePerm = (perm: string) => {
    setNewRolePerms((prev) =>
      prev.includes(perm) ? prev.filter((p) => p !== perm) : [...prev, perm]
    );
  };

  // Group permissions by resource
  const permsByResource: Record<string, any[]> = {};
  permissions.forEach((p) => {
    if (!permsByResource[p.resource]) permsByResource[p.resource] = [];
    permsByResource[p.resource].push(p);
  });

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">Roles & Permissions</h1>
          <p className="text-[13px] text-nb-gray mt-1">Manage RBAC for your organization{DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}</p>
        </div>
        <Button onClick={() => { setShowCreate(!showCreate); setMsg(null); }}>
          <Plus className="w-4 h-4 mr-2" />Create Role
        </Button>
      </div>

      {msg && (
        <div className={`rounded-xl px-4 py-3 mb-6 text-[12px] font-bold border-2 ${msg.type === "success" ? "bg-nb-green/10 border-nb-green text-nb-green" : "bg-nb-red/10 border-nb-red text-nb-red"}`}>
          {msg.text}
        </div>
      )}

      {/* Create Role Form */}
      {showCreate && (
        <Card className="mb-6 border-nb-yellow">
          <div className="flex items-center justify-between mb-4">
            <CardTitle className="flex items-center gap-2"><Shield className="w-4 h-4" />New Custom Role</CardTitle>
            <button onClick={() => setShowCreate(false)} className="text-nb-gray hover:text-nb-black"><X className="w-4 h-4" /></button>
          </div>
          <CardContent>
            <div className="grid grid-cols-2 gap-4 mb-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Display Name</label>
                <Input value={newRoleDisplay} onChange={(e) => setNewRoleDisplay(e.target.value)} placeholder="e.g. QA Engineer" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Slug</label>
                <Input value={newRoleName} onChange={(e) => setNewRoleName(e.target.value)} placeholder="e.g. qa_engineer" />
              </div>
            </div>

            <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-3">Permissions</label>
            <div className="space-y-3 mb-4">
              {Object.entries(permsByResource).map(([resource, perms]) => (
                <div key={resource}>
                  <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray mb-1.5">{resource}</p>
                  <div className="flex flex-wrap gap-1.5">
                    {perms.map((p: any) => (
                      <button
                        key={p.name}
                        onClick={() => togglePerm(p.name)}
                        className={`px-2.5 py-1 rounded-lg text-[10px] font-bold border transition-all ${
                          newRolePerms.includes(p.name)
                            ? "bg-nb-yellow/20 border-nb-yellow text-nb-black"
                            : "bg-nb-bg border-nb-light text-nb-gray hover:border-nb-black"
                        }`}
                      >
                        {newRolePerms.includes(p.name) && <Check className="w-2.5 h-2.5 inline mr-1" />}
                        {p.action}
                      </button>
                    ))}
                  </div>
                </div>
              ))}
            </div>

            <div className="flex justify-end">
              <Button onClick={handleCreate} disabled={creating || !newRoleName.trim() || !newRoleDisplay.trim()}>
                {creating ? "Creating..." : "Create Role"}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Roles List */}
      {loading ? (
        <p className="text-center py-8 text-nb-gray">Loading roles...</p>
      ) : (
        <div className="space-y-4">
          {roles.map((role) => (
            <Card key={role.id}>
              <div className="flex items-start justify-between mb-3">
                <div>
                  <div className="flex items-center gap-2">
                    <h3 className="font-black text-[14px]">{role.display_name}</h3>
                    <Badge variant={role.is_builtin ? "info" : "default"}>
                      {role.is_builtin ? "Built-in" : "Custom"}
                    </Badge>
                    <code className="text-[10px] text-nb-gray font-mono bg-nb-bg px-2 py-0.5 rounded">{role.name}</code>
                  </div>
                  {role.description && <p className="text-[12px] text-nb-gray mt-1">{role.description}</p>}
                </div>
                {!role.is_builtin && (
                  <Button variant="ghost" size="sm" onClick={() => handleDelete(role.id)}>
                    <Trash2 className="w-3.5 h-3.5 text-nb-red" />
                  </Button>
                )}
              </div>

              <div className="flex flex-wrap gap-1.5">
                {(role.permissions || []).map((perm: string) => {
                  const [resource, action] = perm.split(":");
                  return (
                    <span key={perm} className="px-2 py-0.5 rounded-md text-[9px] font-bold uppercase tracking-wider bg-nb-bg border border-nb-light text-nb-gray">
                      <span className="text-nb-black">{resource}</span>:{action}
                    </span>
                  );
                })}
                {(!role.permissions || role.permissions.length === 0) && (
                  <span className="text-[11px] text-nb-gray">No permissions</span>
                )}
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
