"use client";

import { useEffect, useState } from "react";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { UserAvatar } from "@/components/user-avatar";
import { Users, Plus, X, Trash2, Shield, UserPlus } from "lucide-react";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED } from "@/lib/demo";

export default function GroupsPage() {
  const [groups, setGroups] = useState<any[]>([]);
  const [roles, setRoles] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [selectedGroup, setSelectedGroup] = useState<any>(null);

  // Create form
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [newRoleId, setNewRoleId] = useState("");
  const [creating, setCreating] = useState(false);

  // Add member
  const [addEmail, setAddEmail] = useState("");
  const [adding, setAdding] = useState(false);
  const [memberMsg, setMemberMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);

  const reload = async () => {
    try {
      const [g, r] = await Promise.all([authApi.listGroups(), authApi.listRoles()]);
      setGroups(g.groups as any);
      setRoles(r.roles as any);
    } catch {}
  };

  useEffect(() => {
    if (DEMO_ENABLED) { setLoading(false); return; }
    reload().finally(() => setLoading(false));
  }, []);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    setCreating(true);
    try {
      await authApi.createGroup({ name: newName, description: newDesc || undefined, role_id: newRoleId || undefined });
      setNewName(""); setNewDesc(""); setNewRoleId(""); setShowCreate(false);
      await reload();
    } catch {}
    setCreating(false);
  };

  const handleDelete = async (id: string) => {
    try { await authApi.deleteGroup(id); await reload(); if (selectedGroup?.id === id) setSelectedGroup(null); } catch {}
  };

  const handleSelectGroup = async (group: any) => {
    try {
      const detail = await authApi.getGroup(group.id) as any;
      setSelectedGroup(detail);
      setMemberMsg(null);
    } catch {}
  };

  const handleAddMember = async () => {
    if (!addEmail.trim() || !selectedGroup) return;
    setAdding(true); setMemberMsg(null);
    try {
      await authApi.addGroupMember(selectedGroup.id, addEmail);
      setMemberMsg({ type: "success", text: `${addEmail} added!` });
      setAddEmail("");
      await handleSelectGroup(selectedGroup);
    } catch (err) {
      setMemberMsg({ type: "error", text: err instanceof Error ? err.message : "Failed" });
    }
    setAdding(false);
  };

  const handleRemoveMember = async (userId: string) => {
    if (!selectedGroup) return;
    try { await authApi.removeGroupMember(selectedGroup.id, userId); await handleSelectGroup(selectedGroup); } catch {}
  };

  const handleSetRole = async (groupId: string, roleId: string) => {
    try { await authApi.setGroupRole(groupId, roleId); await reload(); } catch {}
  };

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">Groups</h1>
          <p className="text-[13px] text-nb-gray mt-1">Organize users into groups and assign roles for pipeline access</p>
        </div>
        <Button onClick={() => setShowCreate(!showCreate)}>
          <Plus className="w-4 h-4 mr-2" />Create Group
        </Button>
      </div>

      {/* Create form */}
      {showCreate && (
        <Card className="mb-6 border-nb-yellow">
          <div className="flex items-center justify-between mb-4">
            <CardTitle className="flex items-center gap-2"><Users className="w-4 h-4" />New Group</CardTitle>
            <button onClick={() => setShowCreate(false)} className="text-nb-gray hover:text-nb-black"><X className="w-4 h-4" /></button>
          </div>
          <CardContent>
            <div className="grid grid-cols-2 gap-4 mb-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Name</label>
                <Input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="e.g. Backend Team" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Role</label>
                <select
                  value={newRoleId}
                  onChange={(e) => setNewRoleId(e.target.value)}
                  className="w-full px-4 py-3 bg-white border-2 border-nb-black rounded-xl text-[13px] font-medium focus:outline-none focus:shadow-neo-yellow focus:border-nb-yellow"
                >
                  <option value="">No role (assign later)</option>
                  {roles.map((r: any) => (
                    <option key={r.id} value={r.id}>{r.display_name}</option>
                  ))}
                </select>
              </div>
            </div>
            <div className="mb-4">
              <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Description</label>
              <Input value={newDesc} onChange={(e) => setNewDesc(e.target.value)} placeholder="Optional description" />
            </div>
            <div className="flex justify-end">
              <Button onClick={handleCreate} disabled={creating || !newName.trim()}>
                {creating ? "Creating..." : "Create Group"}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      <div className="grid grid-cols-[1fr_1fr] gap-6">
        {/* Groups list */}
        <div>
          {loading ? (
            <p className="text-nb-gray text-[13px]">Loading...</p>
          ) : groups.length === 0 ? (
            <Card><p className="text-nb-gray text-[13px] text-center py-8">No groups yet. Create one to organize your team.</p></Card>
          ) : (
            <div className="space-y-3">
              {groups.map((g: any) => (
                <Card
                  key={g.id}
                  className={`cursor-pointer transition-all ${selectedGroup?.id === g.id ? "border-nb-yellow shadow-neo-yellow" : "hover:shadow-neo-lg hover:-translate-y-0.5"}`}
                  onClick={() => handleSelectGroup(g)}
                >
                  <div className="flex items-start justify-between">
                    <div>
                      <div className="flex items-center gap-2">
                        <h3 className="font-black text-[14px]">{g.name}</h3>
                        <span className="text-[11px] text-nb-gray">{g.member_count} members</span>
                      </div>
                      {g.description && <p className="text-[12px] text-nb-gray mt-0.5">{g.description}</p>}
                      {g.role && (
                        <div className="mt-2">
                          <Badge variant="info">
                            <Shield className="w-2.5 h-2.5 mr-1" />{g.role.display_name}
                          </Badge>
                        </div>
                      )}
                      {!g.role && <p className="text-[10px] text-nb-gray mt-2">No role assigned</p>}
                    </div>
                    <Button variant="ghost" size="sm" onClick={(e) => { e.stopPropagation(); handleDelete(g.id); }}>
                      <Trash2 className="w-3.5 h-3.5 text-nb-red" />
                    </Button>
                  </div>
                </Card>
              ))}
            </div>
          )}
        </div>

        {/* Group detail */}
        <div>
          {selectedGroup ? (
            <Card>
              <CardTitle className="flex items-center gap-2 mb-2">
                <Users className="w-4 h-4" />{selectedGroup.name}
              </CardTitle>

              {/* Role selector */}
              <div className="mb-4">
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Group Role</label>
                <select
                  value={selectedGroup.role?.id || ""}
                  onChange={(e) => { if (e.target.value) handleSetRole(selectedGroup.id, e.target.value); }}
                  className="w-full px-4 py-2.5 bg-white border-2 border-nb-black rounded-xl text-[13px] font-medium focus:outline-none focus:shadow-neo-yellow focus:border-nb-yellow"
                >
                  <option value="">No role</option>
                  {roles.map((r: any) => (
                    <option key={r.id} value={r.id}>{r.display_name}</option>
                  ))}
                </select>
                <p className="text-[10px] text-nb-gray mt-1">All members inherit this role&apos;s permissions</p>
              </div>

              {/* Add member */}
              <div className="mb-4">
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Add Member</label>
                {memberMsg && (
                  <div className={`rounded-xl px-3 py-2 mb-2 text-[11px] font-bold border ${memberMsg.type === "success" ? "bg-nb-green/10 border-nb-green text-nb-green" : "bg-nb-red/10 border-nb-red text-nb-red"}`}>
                    {memberMsg.text}
                  </div>
                )}
                <div className="flex gap-2">
                  <Input value={addEmail} onChange={(e) => setAddEmail(e.target.value)} placeholder="user@email.com" className="flex-1"
                    onKeyDown={(e) => { if (e.key === "Enter") handleAddMember(); }} />
                  <Button size="sm" onClick={handleAddMember} disabled={adding || !addEmail.trim()}>
                    <UserPlus className="w-3.5 h-3.5" />
                  </Button>
                </div>
              </div>

              {/* Members */}
              <CardContent>
                <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Members ({selectedGroup.members?.length || 0})</p>
                <div className="space-y-2">
                  {(selectedGroup.members || []).map((m: any) => (
                    <div key={m.id} className="flex items-center justify-between bg-nb-bg border border-nb-light rounded-xl px-3 py-2">
                      <div className="flex items-center gap-2">
                        <UserAvatar name={m.display_name || m.username} size="sm" />
                        <div>
                          <p className="font-bold text-[12px]">{m.display_name || m.username}</p>
                          <p className="text-[10px] text-nb-gray">{m.email}</p>
                        </div>
                      </div>
                      <button onClick={() => handleRemoveMember(m.user_id)} className="text-nb-red hover:bg-nb-red/10 p-1 rounded">
                        <X className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  ))}
                  {(!selectedGroup.members || selectedGroup.members.length === 0) && (
                    <p className="text-[11px] text-nb-gray text-center py-4">No members yet</p>
                  )}
                </div>
              </CardContent>
            </Card>
          ) : (
            <Card>
              <p className="text-nb-gray text-[13px] text-center py-12">Select a group to manage members and role</p>
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}
