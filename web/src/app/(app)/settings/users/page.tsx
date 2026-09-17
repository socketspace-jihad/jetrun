"use client";

import { useEffect, useState } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { UserAvatar } from "@/components/user-avatar";
import { UserPlus, Search, X, Check } from "lucide-react";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED, demoUsers } from "@/lib/demo";

const roleColors: Record<string, "danger" | "default" | "info" | "muted"> = {
  super_admin: "danger", "Super Admin": "danger",
  admin: "default", Admin: "default",
  developer: "info", Developer: "info",
  viewer: "muted", Viewer: "muted",
};

export default function UsersPage() {
  const [search, setSearch] = useState("");
  const [users, setUsers] = useState<any[]>(DEMO_ENABLED ? demoUsers : []);
  const [loading, setLoading] = useState(!DEMO_ENABLED);

  // Invite modal
  const [showInvite, setShowInvite] = useState(false);
  const [inviteEmail, setInviteEmail] = useState("");
  const [inviting, setInviting] = useState(false);
  const [inviteMsg, setInviteMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);

  // Roles for dropdown
  const [roles, setRoles] = useState<any[]>([]);

  useEffect(() => {
    if (DEMO_ENABLED) return;
    authApi.listUsers()
      .then((res) => setUsers(res.users as any))
      .catch(() => {})
      .finally(() => setLoading(false));
    authApi.listRoles()
      .then((res) => setRoles(res.roles as any))
      .catch(() => {});
  }, []);

  const getOrgId = (): string => {
    const token = localStorage.getItem("jetrun_token");
    if (!token) return "";
    try {
      const payload = JSON.parse(atob(token.split(".")[1]));
      return payload.org_id || "";
    } catch { return ""; }
  };

  const handleInvite = async () => {
    if (!inviteEmail.trim()) return;
    const orgId = getOrgId();
    if (!orgId) {
      setInviteMsg({ type: "error", text: "No organization context. Please re-login." });
      return;
    }
    setInviting(true);
    setInviteMsg(null);
    try {
      await authApi.inviteMember(orgId, inviteEmail);
      setInviteMsg({ type: "success", text: `${inviteEmail} invited!` });
      setInviteEmail("");
      const updated = await authApi.listUsers();
      setUsers(updated.users as any);
    } catch (err) {
      setInviteMsg({ type: "error", text: err instanceof Error ? err.message : "Failed to invite" });
    }
    setInviting(false);
  };

  const filtered = users.filter((u) =>
    u.username?.toLowerCase().includes(search.toLowerCase()) ||
    u.email?.toLowerCase().includes(search.toLowerCase()) ||
    (u.display_name || "").toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">Users</h1>
          <p className="text-[13px] text-nb-gray mt-1">
            {users.length} users{users.filter((u) => u.is_active !== false).length < users.length && `, ${users.filter((u) => u.is_active !== false).length} active`}
            {DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}
          </p>
        </div>
        <Button onClick={() => { setShowInvite(!showInvite); setInviteMsg(null); }}>
          <UserPlus className="w-4 h-4 mr-2" />Invite User
        </Button>
      </div>

      {/* Invite panel */}
      {showInvite && (
        <Card className="mb-6 border-nb-yellow">
          <div className="flex items-center justify-between mb-3">
            <p className="font-black text-[13px] uppercase tracking-wider">Invite to Organization</p>
            <button onClick={() => setShowInvite(false)} className="text-nb-gray hover:text-nb-black"><X className="w-4 h-4" /></button>
          </div>
          {inviteMsg && (
            <div className={`rounded-xl px-4 py-2 mb-3 text-[12px] font-bold border-2 ${inviteMsg.type === "success" ? "bg-nb-green/10 border-nb-green text-nb-green" : "bg-nb-red/10 border-nb-red text-nb-red"}`}>
              {inviteMsg.text}
            </div>
          )}
          <div className="flex gap-3">
            <Input
              value={inviteEmail}
              onChange={(e) => setInviteEmail(e.target.value)}
              placeholder="Email address of existing user"
              className="flex-1"
              onKeyDown={(e) => { if (e.key === "Enter") handleInvite(); }}
            />
            <Button onClick={handleInvite} disabled={inviting || !inviteEmail.trim()}>
              {inviting ? "Inviting..." : "Send Invite"}
            </Button>
          </div>
          <p className="text-[11px] text-nb-gray mt-2">User must have an account first. They&apos;ll be added as Developer by default.</p>
        </Card>
      )}

      <div className="relative mb-6 max-w-[400px]">
        <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-4 h-4 text-nb-gray" />
        <Input placeholder="Search users..." value={search} onChange={(e) => setSearch(e.target.value)} className="pl-11" />
      </div>

      <Card>
        <CardContent>
          {loading ? (
            <p className="text-center py-8 text-nb-gray text-[13px]">Loading users...</p>
          ) : filtered.length === 0 ? (
            <p className="text-center py-8 text-nb-gray text-[13px]">No users found</p>
          ) : (
            <div className="space-y-1">
              <div className="grid grid-cols-[40px_1fr_120px_100px_100px_120px] gap-4 px-3 py-2 text-[10px] font-black uppercase tracking-widest text-nb-gray">
                <span /><span>User</span><span>Role</span><span>Provider</span><span>Status</span><span>Last Login</span>
              </div>
              {filtered.map((user) => (
                <div key={user.id} className="grid grid-cols-[40px_1fr_120px_100px_100px_120px] gap-4 px-3 py-2.5 rounded-xl hover:bg-nb-bg transition-colors items-center">
                  <UserAvatar name={user.display_name || user.username} size="sm" />
                  <div>
                    <p className="font-black text-[13px]">{user.display_name || user.username}</p>
                    <p className="text-[11px] text-nb-gray">{user.email}</p>
                  </div>
                  <Badge variant={roleColors[user.role] || "muted"}>{(user.role || "").replace("_", " ")}</Badge>
                  <span className="text-[11px] text-nb-gray font-medium capitalize">{user.auth_provider}</span>
                  <Badge variant={user.is_active !== false ? "success" : "muted"}>{user.is_active !== false ? "Active" : "Inactive"}</Badge>
                  <span className="text-[11px] text-nb-gray">{user.last_login_at ? new Date(user.last_login_at).toLocaleDateString() : "Never"}</span>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
