"use client";

import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { UserAvatar } from "@/components/user-avatar";
import { Users, UserPlus, Search } from "lucide-react";
import { useState } from "react";

const mockUsers = [
  {
    id: "u1",
    email: "admin@jetrun.local",
    username: "admin",
    display_name: "Super Admin",
    role: "super_admin",
    is_active: true,
    auth_provider: "local",
    last_login_at: "2024-03-10T15:30:00Z",
  },
  {
    id: "u2",
    email: "dev@company.com",
    username: "jdoe",
    display_name: "John Doe",
    role: "developer",
    is_active: true,
    auth_provider: "github",
    last_login_at: "2024-03-10T12:00:00Z",
  },
  {
    id: "u3",
    email: "viewer@company.com",
    username: "janesmith",
    display_name: "Jane Smith",
    role: "viewer",
    is_active: true,
    auth_provider: "google",
    last_login_at: "2024-03-09T10:00:00Z",
  },
  {
    id: "u4",
    email: "old@company.com",
    username: "olduser",
    display_name: "Old User",
    role: "developer",
    is_active: false,
    auth_provider: "local",
    last_login_at: "2024-01-15T10:00:00Z",
  },
];

const roleColors: Record<string, "danger" | "default" | "info" | "muted"> = {
  super_admin: "danger",
  admin: "default",
  developer: "info",
  viewer: "muted",
};

export default function UsersPage() {
  const [search, setSearch] = useState("");

  const filtered = mockUsers.filter(
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
            {mockUsers.length} users, {mockUsers.filter((u) => u.is_active).length} active
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
                  <p className="font-black text-[13px]">
                    {user.display_name || user.username}
                  </p>
                  <p className="text-[11px] text-nb-gray">{user.email}</p>
                </div>
                <Badge variant={roleColors[user.role] || "muted"}>
                  {user.role.replace("_", " ")}
                </Badge>
                <span className="text-[11px] text-nb-gray font-medium capitalize">
                  {user.auth_provider}
                </span>
                <Badge variant={user.is_active ? "success" : "muted"}>
                  {user.is_active ? "Active" : "Inactive"}
                </Badge>
                <span className="text-[11px] text-nb-gray">
                  {new Date(user.last_login_at).toLocaleDateString()}
                </span>
                <Button variant="ghost" size="sm">
                  Edit
                </Button>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
