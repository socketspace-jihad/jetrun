"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { PasswordInput } from "@/components/ui/password-input";
import { Badge } from "@/components/ui/badge";
import { UserAvatar } from "@/components/user-avatar";
import { User, Shield, Key, Monitor, LogOut, Check } from "lucide-react";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED, demoCurrentUser, demoSessions } from "@/lib/demo";

export default function AccountSettingsPage() {
  const [user, setUser] = useState<any>(DEMO_ENABLED ? demoCurrentUser : null);
  const [sessions, setSessions] = useState<any[]>(DEMO_ENABLED ? demoSessions : []);
  const [displayName, setDisplayName] = useState("");
  const [loading, setLoading] = useState(!DEMO_ENABLED);
  const [profileSaving, setProfileSaving] = useState(false);
  const [profileSaved, setProfileSaved] = useState(false);
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [passwordSaving, setPasswordSaving] = useState(false);
  const [passwordMsg, setPasswordMsg] = useState<{ type: "success" | "error"; text: string } | null>(null);

  useEffect(() => {
    if (DEMO_ENABLED) { setDisplayName(demoCurrentUser.display_name || ""); return; }
    Promise.all([authApi.getMe(), authApi.listSessions()])
      .then(([me, sess]) => {
        setUser(me);
        setDisplayName((me as any).display_name || "");
        setSessions((sess.sessions as any) || []);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const handleSaveProfile = async () => {
    setProfileSaving(true);
    try {
      await authApi.updateMe({ display_name: displayName });
      setProfileSaved(true);
      setTimeout(() => setProfileSaved(false), 3000);
    } catch (_) {}
    setProfileSaving(false);
  };

  const handleChangePassword = async () => {
    setPasswordMsg(null);
    if (newPassword !== confirmPassword) { setPasswordMsg({ type: "error", text: "Passwords do not match" }); return; }
    if (newPassword.length < 8) { setPasswordMsg({ type: "error", text: "Min 8 characters" }); return; }
    setPasswordSaving(true);
    try {
      await authApi.changePassword(currentPassword, newPassword);
      setPasswordMsg({ type: "success", text: "Password changed" });
      setCurrentPassword(""); setNewPassword(""); setConfirmPassword("");
    } catch (err) {
      setPasswordMsg({ type: "error", text: err instanceof Error ? err.message : "Failed" });
    }
    setPasswordSaving(false);
  };

  const handleRevokeSession = async (id: string) => {
    try { await authApi.revokeSession(id); setSessions((s) => s.filter((x) => x.id !== id)); } catch (_) {}
  };

  if (loading) return <div className="p-8"><p className="text-nb-gray">Loading...</p></div>;
  if (!user) return <div className="p-8"><p className="text-nb-gray">Not logged in.</p></div>;

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">Account Settings</h1>
        <p className="text-[13px] text-nb-gray mt-1">Manage your profile, security, and sessions{DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}</p>
      </div>
      <div className="grid grid-cols-2 gap-6 max-w-[960px]">
        <Card>
          <CardTitle className="flex items-center gap-2 mb-5"><User className="w-4 h-4" />Profile</CardTitle>
          <CardContent>
            <div className="flex items-center gap-4 mb-6">
              <UserAvatar name={displayName || user.username} size="lg" />
              <div>
                <p className="font-black text-[14px]">{user.username}</p>
                <Badge variant={user.role === "super_admin" ? "danger" : "default"}>{(user.role || "").replace("_", " ")}</Badge>
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
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Auth Provider</label>
                <Badge variant="info">{user.auth_provider}</Badge>
              </div>
            </div>
            <div className="mt-5 flex items-center justify-end gap-2">
              {profileSaved && <span className="text-[11px] text-nb-green font-bold flex items-center gap-1"><Check className="w-3 h-3" />Saved</span>}
              <Button size="sm" onClick={handleSaveProfile} disabled={profileSaving}>{profileSaving ? "Saving..." : "Save Profile"}</Button>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardTitle className="flex items-center gap-2 mb-5"><Shield className="w-4 h-4" />Security</CardTitle>
          <CardContent>
            {passwordMsg && (
              <div className={`rounded-xl px-4 py-3 mb-4 text-[12px] font-bold border-2 ${passwordMsg.type === "success" ? "bg-nb-green/10 border-nb-green text-nb-green" : "bg-nb-red/10 border-nb-red text-nb-red"}`}>{passwordMsg.text}</div>
            )}
            <div className="space-y-4">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Current Password</label>
                <PasswordInput value={currentPassword} onChange={(e) => setCurrentPassword(e.target.value)} placeholder="Enter current password" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">New Password</label>
                <PasswordInput value={newPassword} onChange={(e) => setNewPassword(e.target.value)} placeholder="Min 8 characters" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">Confirm New Password</label>
                <PasswordInput value={confirmPassword} onChange={(e) => setConfirmPassword(e.target.value)} placeholder="Repeat new password" />
              </div>
            </div>
            <div className="mt-5 flex justify-end">
              <Button variant="danger" size="sm" onClick={handleChangePassword} disabled={passwordSaving || !currentPassword || !newPassword}>
                <Key className="w-3 h-3 mr-1.5" />{passwordSaving ? "Changing..." : "Change Password"}
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card className="col-span-2">
          <CardTitle className="flex items-center gap-2 mb-5"><Monitor className="w-4 h-4" />Active Sessions</CardTitle>
          <CardContent>
            <div className="space-y-3">
              {sessions.map((s: any, i: number) => (
                <div key={s.id} className="flex items-center justify-between bg-nb-bg border border-nb-light rounded-xl px-4 py-3">
                  <div>
                    <div className="flex items-center gap-2">
                      <p className="font-bold text-[12px]">
                        {(s.user_agent || "").includes("Mac") ? "macOS" : (s.user_agent || "").includes("iPhone") ? "iOS" : (s.user_agent || "").includes("Win") ? "Windows" : "Browser"}
                      </p>
                      {i === 0 && <Badge variant="success">Current</Badge>}
                    </div>
                    <p className="text-[11px] text-nb-gray mt-0.5">IP: {s.ip_address || "—"} · {new Date(s.last_used_at).toLocaleDateString()}</p>
                  </div>
                  {i !== 0 && <Button variant="ghost" size="sm" onClick={() => handleRevokeSession(s.id)}><LogOut className="w-3 h-3 mr-1" />Revoke</Button>}
                </div>
              ))}
              {sessions.length === 0 && <p className="text-[12px] text-nb-gray text-center py-4">No active sessions</p>}
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
