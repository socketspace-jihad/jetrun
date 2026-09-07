"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { UserAvatar } from "@/components/user-avatar";
import { User, Shield, Key, Monitor, LogOut } from "lucide-react";

// Mock user data
const mockUser = {
  id: "u1",
  email: "admin@jetrun.local",
  username: "admin",
  display_name: "Super Admin",
  avatar_url: null,
  role: "super_admin",
  auth_provider: "local",
  email_verified: true,
  created_at: "2024-01-15T10:00:00Z",
};

const mockSessions = [
  {
    id: "s1",
    user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
    ip_address: "127.0.0.1",
    created_at: "2024-03-10T14:00:00Z",
    last_used_at: "2024-03-10T15:30:00Z",
    current: true,
  },
  {
    id: "s2",
    user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)",
    ip_address: "192.168.1.42",
    created_at: "2024-03-09T09:00:00Z",
    last_used_at: "2024-03-10T12:00:00Z",
    current: false,
  },
];

export default function AccountSettingsPage() {
  const [displayName, setDisplayName] = useState(mockUser.display_name);
  const [email] = useState(mockUser.email);

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
          Account Settings
        </h1>
        <p className="text-[13px] text-nb-gray mt-1">
          Manage your profile, security, and active sessions
        </p>
      </div>

      <div className="grid grid-cols-2 gap-6 max-w-[960px]">
        {/* Profile */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-5">
            <User className="w-4 h-4" />
            Profile
          </CardTitle>
          <CardContent>
            <div className="flex items-center gap-4 mb-6">
              <UserAvatar name={displayName} size="lg" />
              <div>
                <p className="font-black text-[14px]">{mockUser.username}</p>
                <Badge
                  variant={
                    mockUser.role === "super_admin" ? "danger" : "default"
                  }
                >
                  {mockUser.role.replace("_", " ")}
                </Badge>
              </div>
            </div>

            <div className="space-y-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Display Name
                </label>
                <Input
                  value={displayName || ""}
                  onChange={(e) => setDisplayName(e.target.value)}
                />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Email
                </label>
                <Input value={email} disabled />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Username
                </label>
                <Input value={mockUser.username} disabled />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Auth Provider
                </label>
                <Badge variant="info">{mockUser.auth_provider}</Badge>
              </div>
            </div>

            <div className="mt-5 flex justify-end">
              <Button size="sm">Save Profile</Button>
            </div>
          </CardContent>
        </Card>

        {/* Security */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-5">
            <Shield className="w-4 h-4" />
            Security
          </CardTitle>
          <CardContent>
            <div className="space-y-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Current Password
                </label>
                <Input type="password" placeholder="Enter current password" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  New Password
                </label>
                <Input type="password" placeholder="Min 8 characters" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Confirm New Password
                </label>
                <Input type="password" placeholder="Repeat new password" />
              </div>
            </div>
            <div className="mt-5 flex justify-end">
              <Button variant="danger" size="sm">
                <Key className="w-3 h-3 mr-1.5" />
                Change Password
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* Active Sessions */}
        <Card className="col-span-2">
          <CardTitle className="flex items-center gap-2 mb-5">
            <Monitor className="w-4 h-4" />
            Active Sessions
          </CardTitle>
          <CardContent>
            <div className="space-y-3">
              {mockSessions.map((session) => (
                <div
                  key={session.id}
                  className="flex items-center justify-between bg-nb-bg border border-nb-light rounded-xl px-4 py-3"
                >
                  <div>
                    <div className="flex items-center gap-2">
                      <p className="font-bold text-[12px] text-nb-black">
                        {session.user_agent.includes("Macintosh")
                          ? "macOS"
                          : session.user_agent.includes("iPhone")
                            ? "iOS"
                            : "Unknown"}
                      </p>
                      {session.current && (
                        <Badge variant="success">Current</Badge>
                      )}
                    </div>
                    <p className="text-[11px] text-nb-gray mt-0.5">
                      IP: {session.ip_address} &middot; Last active:{" "}
                      {new Date(session.last_used_at).toLocaleDateString()}
                    </p>
                  </div>
                  {!session.current && (
                    <Button variant="ghost" size="sm">
                      <LogOut className="w-3 h-3 mr-1" />
                      Revoke
                    </Button>
                  )}
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
