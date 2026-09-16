"use client";

import { useEffect, useState } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { UserAvatar } from "@/components/user-avatar";
import { UserPlus, Search } from "lucide-react";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED, demoUsers } from "@/lib/demo";

const roleColors: Record<string, "danger" | "default" | "info" | "muted"> = {
  super_admin: "danger",
  "Super Admin": "danger",
  admin: "default",
  Admin: "default",
  developer: "info",
  Developer: "info",
  viewer: "muted",
  Viewer: "muted",
};

export default function UsersPage() {
  const [search, setSearch] = useState("");
  const [users, setUsers] = useState<any[]>(DEMO_ENABLED ? demoUsers : []);
  const [loading, setLoading] = useState(!DEMO_ENABLED);

  useEffect(() => {
    if (DEMO_ENABLED) return;
    authApi.listUsers()
      .then((res) => setUsers(res.users as any))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const filtered = users.filter(
    (u) =>
      u.username.toLowerCase().includes(search.toLowerCase()) ||
      u.email.toLowerCase().includes(search.toLowerCase()) ||
      (u.display_name || "").toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
            Users
          </h1>
          <p className="text-[13px] text-nb-gray mt-1">
            {users.length} users, {users.filter((u) => u.is_active).length} active
            {DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}
          </p>
        </div>
        <Button>
          <UserPlus className="w-4 h-4 mr-2" />
          Invite User
        </Button>
      </div>

      <div className="relative mb-6 max-w-[400px]">
        <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-4 h-4 text-nb-gray" />
        <Input
          placeholder="Search users..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-11"
        />
      </div>

      <Card>
        <CardContent>
          {loading ? (
            <p className="text-center py-8 text-nb-gray text-[13px]">Loading users...</p>
          ) : (
            <div className="space-y-1">
              <div className="grid grid-cols-[40px_1fr_120px_100px_100px_120px_80px] gap-4 px-3 py-2 text-[10px] font-black uppercase tracking-widest text-nb-gray">
                <span />
                <span>User</span>
                <span>Role</span>
                <span>Provider</span>
                <span>Status</span>
                <span>Last Login</span>
                <span />
              </div>
              {filtered.map((user) => (
                <div
                  key={user.id}
                  className="grid grid-cols-[40px_1fr_120px_100px_100px_120px_80px] gap-4 px-3 py-2.5 rounded-xl hover:bg-nb-bg transition-colors items-center"
                >
                  <UserAvatar name={user.display_name} size="sm" />
                  <div>
                    <p className="font-black text-[13px]">{user.display_name || user.username}</p>
                    <p className="text-[11px] text-nb-gray">{user.email}</p>
                  </div>
                  <Badge variant={roleColors[user.role] || "muted"}>
                    {(user.role || "").replace("_", " ")}
                  </Badge>
                  <span className="text-[11px] text-nb-gray font-medium capitalize">{user.auth_provider}</span>
                  <Badge variant={user.is_active ? "success" : "muted"}>
                    {user.is_active ? "Active" : "Inactive"}
                  </Badge>
                  <span className="text-[11px] text-nb-gray">
                    {user.last_login_at ? new Date(user.last_login_at).toLocaleDateString() : "Never"}
                  </span>
                  <Button variant="ghost" size="sm">Edit</Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
