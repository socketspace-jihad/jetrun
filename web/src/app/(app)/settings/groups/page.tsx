"use client";

import { useEffect, useState, useRef } from "react";
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
  const [orgUsers, setOrgUsers] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [selectedGroup, setSelectedGroup] = useState<any>(null);

  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [newRoleId, setNewRoleId] = useState("");
  const [creating, setCreating] = useState(false);

  // Add member with autocomplete
  const [searchQuery, setSearchQuery] = useState("");
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [adding, setAdding] = useState(false);
  const [memberMsg, setMemberMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const searchRef = useRef<HTMLDivElement>(null);

  const reload = async () => {
    try {
      const [g, r, u] = await Promise.all([authApi.listGroups(), authApi.listRoles(), authApi.listUsers()]);
      setGroups(g.groups as any);
      setRoles(r.roles as any);
      setOrgUsers((u.users as any) || []);
    } catch {}
  };

  useEffect(() => {
    if (DEMO_ENABLED) { setLoading(false); return; }
    reload().finally(() => setLoading(false));
  }, []);

  // Close suggestions when clicking outside
  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (searchRef.current && !searchRef.current.contains(e.target as Node)) {
        setShowSuggestions(false);
      }
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, []);

  // Filter users for autocomplete — exclude users already in the group
  const memberIds = new Set((selectedGroup?.members || []).map((m: any) => m.user_id));
  const filteredUsers = orgUsers.filter((u) => {
    if (memberIds.has(u.id)) return false;
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return u.email?.toLowerCase().includes(q) ||
      u.username?.toLowerCase().includes(q) ||
      (u.display_name || "").toLowerCase().includes(q);
  });

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
      setSearchQuery("");
    } catch {}
  };

  const handleAddMember = async (email: string) => {
    if (!email.trim() || !selectedGroup) return;
    setAdding(true); setMemberMsg(null);
    try {
      await authApi.addGroupMember(selectedGroup.id, email);
      setMemberMsg({ type: "success", text: `Added!` });
      setSearchQuery("");
      setShowSuggestions(false);
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
                <select value={newRoleId} onChange={(e) => setNewRoleId(e.target.value)}
                  className="w-full px-4 py-3 bg-white border-2 border-nb-black rounded-xl text-[13px] font-medium focus:outline-none focus:shadow-neo-yellow focus:border-nb-yellow">
                  <option value="">No role (assign later)</option>
                  {roles.map((r: any) => <option key={r.id} value={r.id}>{r.display_name}</option>)}
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
                <Card key={g.id}
                  className={`cursor-pointer transition-all ${selectedGroup?.id === g.id ? "border-nb-yellow shadow-neo-yellow" : "hover:shadow-neo-lg hover:-translate-y-0.5"}`}
                  onClick={() => handleSelectGroup(g)}>
                  <div className="flex items-start justify-between">
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <h3 className="font-black text-[14px]">{g.name}</h3>
                        <span className="text-[11px] text-nb-gray">{g.member_count} members</span>
                      </div>
                      {g.description && <p className="text-[12px] text-nb-gray mt-0.5">{g.description}</p>}
                      {g.role ? (
                        <div className="mt-2"><Badge variant="info"><Shield className="w-2.5 h-2.5 mr-1" />{g.role.display_name}</Badge></div>
                      ) : (
                        <p className="text-[10px] text-nb-gray mt-2">No role assigned</p>
                      )}
                      {/* Member avatars */}
                      {g.members && g.members.length > 0 && (
                        <div className="flex items-center mt-3 pt-3 border-t border-nb-light">
                          <div className="flex -space-x-2">
                            {g.members.slice(0, 5).map((m: any, i: number) => (
                              <div key={m.user_id} className="relative" style={{ zIndex: 5 - i }} title={m.display_name || m.username}>
                                <UserAvatar name={m.display_name || m.username} avatarUrl={m.avatar_url} size="sm" className="ring-2 ring-white" />
                              </div>
                            ))}
                          </div>
                          {g.members.length > 5 && (
                            <span className="ml-2 text-[10px] font-bold text-nb-gray">+{g.members.length - 5} more</span>
                          )}
                          <div className="ml-auto flex flex-col gap-0.5">
                            {g.members.slice(0, 3).map((m: any) => (
                              <span key={m.user_id} className="text-[10px] text-nb-gray truncate max-w-[120px]">{m.display_name || m.username}</span>
                            ))}
                          </div>
                        </div>
                      )}
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
              <CardTitle className="flex items-center gap-2 mb-2"><Users className="w-4 h-4" />{selectedGroup.name}</CardTitle>

              {/* Role selector */}
              <div className="mb-4">
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Group Role</label>
                <select value={selectedGroup.role?.id || ""}
                  onChange={(e) => { if (e.target.value) handleSetRole(selectedGroup.id, e.target.value); }}
                  className="w-full px-4 py-2.5 bg-white border-2 border-nb-black rounded-xl text-[13px] font-medium focus:outline-none focus:shadow-neo-yellow focus:border-nb-yellow">
                  <option value="">No role</option>
                  {roles.map((r: any) => <option key={r.id} value={r.id}>{r.display_name}</option>)}
                </select>
                <p className="text-[10px] text-nb-gray mt-1">All members inherit this role&apos;s permissions</p>
              </div>

              {/* Add member — autocomplete search */}
              <div className="mb-4">
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Add Member</label>
                {memberMsg && (
                  <div className={`rounded-xl px-3 py-2 mb-2 text-[11px] font-bold border ${memberMsg.type === "success" ? "bg-nb-green/10 border-nb-green text-nb-green" : "bg-nb-red/10 border-nb-red text-nb-red"}`}>
                    {memberMsg.text}
                  </div>
                )}
                <div ref={searchRef} className="relative">
                  <Input
                    value={searchQuery}
                    onChange={(e) => { setSearchQuery(e.target.value); setShowSuggestions(true); setMemberMsg(null); }}
                    onFocus={() => setShowSuggestions(true)}
                    placeholder="Search by name or email..."
                  />

                  {/* Dropdown suggestions */}
                  {showSuggestions && searchQuery.trim() && (
                    <div className="absolute top-full left-0 right-0 mt-1 bg-nb-white border-2 border-nb-black rounded-xl shadow-neo-lg max-h-[240px] overflow-y-auto z-50">
                      {filteredUsers.length === 0 ? (
                        <p className="px-4 py-3 text-[12px] text-nb-gray">No users found</p>
                      ) : (
                        filteredUsers.slice(0, 8).map((u) => (
                          <button
                            key={u.id}
                            onClick={() => handleAddMember(u.email)}
                            disabled={adding}
                            className="flex items-center gap-3 w-full px-4 py-2.5 hover:bg-nb-bg transition-colors text-left"
                          >
                            <UserAvatar name={u.display_name || u.username} size="sm" />
                            <div className="flex-1 min-w-0">
                              <p className="font-bold text-[12px] text-nb-black truncate">{u.display_name || u.username}</p>
                              <p className="text-[10px] text-nb-gray truncate">{u.email}</p>
                            </div>
                            <UserPlus className="w-3.5 h-3.5 text-nb-gray shrink-0" />
                          </button>
                        ))
                      )}
                    </div>
                  )}

                  {/* Show all users when focused but no query */}
                  {showSuggestions && !searchQuery.trim() && filteredUsers.length > 0 && (
                    <div className="absolute top-full left-0 right-0 mt-1 bg-nb-white border-2 border-nb-black rounded-xl shadow-neo-lg max-h-[240px] overflow-y-auto z-50">
                      <p className="px-4 py-2 text-[10px] font-black uppercase tracking-widest text-nb-gray border-b border-nb-light">
                        Org members ({filteredUsers.length})
                      </p>
                      {filteredUsers.slice(0, 8).map((u) => (
                        <button
                          key={u.id}
                          onClick={() => handleAddMember(u.email)}
                          disabled={adding}
                          className="flex items-center gap-3 w-full px-4 py-2.5 hover:bg-nb-bg transition-colors text-left"
                        >
                          <UserAvatar name={u.display_name || u.username} size="sm" />
                          <div className="flex-1 min-w-0">
                            <p className="font-bold text-[12px] text-nb-black truncate">{u.display_name || u.username}</p>
                            <p className="text-[10px] text-nb-gray truncate">{u.email}</p>
                          </div>
                          <UserPlus className="w-3.5 h-3.5 text-nb-gray shrink-0" />
                        </button>
                      ))}
                    </div>
                  )}
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
