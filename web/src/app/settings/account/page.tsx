"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { UserAvatar } from "@/components/user-avatar";
import { User, Shield, Key, Monitor, LogOut } from "lucide-react";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED, demoCurrentUser, demoSessions } from "@/lib/demo";

export default function AccountSettingsPage() {
  const [user, setUser] = useState<any>(DEMO_ENABLED ? demoCurrentUser : null);
  const [sessions, setSessions] = useState<any[]>(DEMO_ENABLED ? demoSessions : []);
  const [displayName, setDisplayName] = useState("");
  const [loading, setLoading] = useState(!DEMO_ENABLED);

  useEffect(() => {
    if (DEMO_ENABLED) {
      setDisplayName(demoCurrentUser.display_name || "");
      return;
    }
    Promise.all([authApi.getMe(), authApi.listSessions()])
      .then(([me, sess]) => {
        setUser(me);
        setDisplayName((me as any).display_name || "");
        setSessions((sess.sessions as any) || []);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="p-8">
        <p className="text-nb-gray text-[13px]">Loading account...</p>
      </div>
    );
  }

  if (!user) {
    return (
      <div className="p-8">
        <p className="text-nb-gray text-[13px]">Not logged in. Please sign in first.</p>
      </div>
    );
  }

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
          Account Settings
        </h1>
        <p className="text-[13px] text-nb-gray mt-1">
          Manage your profile, security, and active sessions
          {DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}
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
                <p className="font-black text-[14px]">{user.username}</p>
                <Badge variant={user.role === "super_admin" ? "danger" : "default"}>
                  {(user.role || "").replace("_", " ")}
                </Badge>
              </div>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Display Name</label>
                <Input value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Email</label>
                <Input value={user.email} disabled />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Username</label>
                <Input value={user.username} disabled />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Auth Provider</label>
                <Badge variant="info">{user.auth_provider}</Badge>
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
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Current Password</label>
                <Input type="password" placeholder="Enter current password" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">New Password</label>
                <Input type="password" placeholder="Min 8 characters" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Confirm New Password</label>
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
              {sessions.map((session: any, i: number) => (
                <div
                  key={session.id}
                  className="flex items-center justify-between bg-nb-bg border border-nb-light rounded-xl px-4 py-3"
                >
                  <div>
                    <div className="flex items-center gap-2">
                      <p className="font-bold text-[12px] text-nb-black">
                        {(session.user_agent || "").includes("Macintosh")
                          ? "macOS"
                          : (session.user_agent || "").includes("iPhone")
                            ? "iOS"
                            : "Browser"}
                      </p>
                      {(session.current || i === 0) && <Badge variant="success">Current</Badge>}
                    </div>
                    <p className="text-[11px] text-nb-gray mt-0.5">
                      IP: {session.ip_address || "—"} &middot; Last active:{" "}
                      {new Date(session.last_used_at).toLocaleDateString()}
                    </p>
                  </div>
                  {!(session.current || i === 0) && (
                    <Button variant="ghost" size="sm">
                      <LogOut className="w-3 h-3 mr-1" />
                      Revoke
                    </Button>
                  )}
                </div>
              ))}
              {sessions.length === 0 && (
                <p className="text-[12px] text-nb-gray text-center py-4">No active sessions</p>
              )}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
