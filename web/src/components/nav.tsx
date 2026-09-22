"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";
import {
  Zap,
  LayoutDashboard,
  GitBranch,
  Settings,
  Database,
  Users,
  Key,
  User,
  LogOut,
  Shield,
  Building2,
  ChevronRight,
  Check,
  Cpu,
} from "lucide-react";
import { UserAvatar } from "@/components/user-avatar";
import { useState } from "react";
import { useAuth } from "@/components/auth-provider";
import { DEMO_ENABLED, demoNavUser } from "@/lib/demo";

const navItems = [
  { href: "/", icon: LayoutDashboard, label: "Dashboard" },
  { href: "/pipelines", icon: GitBranch, label: "Pipelines" },
  { href: "/settings", icon: Settings, label: "Settings" },
];

const settingsSubNav = [
  { href: "/settings", icon: Settings, label: "General" },
  { href: "/settings/account", icon: User, label: "Account" },
  { href: "/settings/users", icon: Users, label: "Users" },
  { href: "/settings/groups", icon: Users, label: "Groups" },
  { href: "/settings/roles", icon: Shield, label: "Roles & Permissions" },
  { href: "/settings/secrets", icon: Key, label: "Secrets" },
  { href: "/settings/api-keys", icon: Key, label: "API Keys" },
  { href: "/settings/workers", icon: Cpu, label: "Workers" },
];

export function Nav() {
  const pathname = usePathname();
  const [showUserMenu, setShowUserMenu] = useState(false);
  const [showOrgPicker, setShowOrgPicker] = useState(false);
  const isSettingsPage = pathname.startsWith("/settings");
  const { user: authUser, orgs, currentOrg, logout, switchOrg } = useAuth();

  const navUser = DEMO_ENABLED
    ? demoNavUser
    : authUser
      ? { display_name: authUser.display_name || authUser.username, email: authUser.email, role: authUser.role }
      : null;

  const handleSwitchOrg = async (orgId: string) => {
    try {
      await switchOrg(orgId);
      setShowOrgPicker(false);
      setShowUserMenu(false);
      window.location.reload();
    } catch {}
  };

  return (
    <div className="flex shrink-0">
      {/* Main Rail */}
      <nav className="w-[68px] bg-nb-black flex flex-col items-center py-4 gap-2 shrink-0">
        <Link
          href="/"
          className="w-11 h-11 bg-nb-yellow border-2 border-nb-black rounded-xl shadow-neo-sm flex items-center justify-center mb-4 hover:brightness-110 transition-all active:translate-y-0.5 active:shadow-none"
        >
          <Zap className="w-6 h-6 text-nb-black" />
        </Link>

        {navItems.map((item) => {
          const isActive = item.href === "/" ? pathname === "/" : pathname.startsWith(item.href);
          const Icon = item.icon;
          return (
            <Link
              key={item.href}
              href={item.href}
              className={cn(
                "w-11 h-11 rounded-xl flex items-center justify-center transition-all",
                isActive ? "bg-nb-yellow text-nb-black" : "text-white/50 hover:text-white hover:bg-white/10"
              )}
              title={item.label}
            >
              <Icon className="w-5 h-5" />
            </Link>
          );
        })}

        {/* User avatar */}
        <div className="mt-auto relative">
          <Link href="/settings" className="w-11 h-11 rounded-xl flex items-center justify-center text-white/30 hover:text-white/60 transition-colors mb-2" title="Cache">
            <Database className="w-4 h-4" />
          </Link>

          <button onClick={() => { setShowUserMenu(!showUserMenu); setShowOrgPicker(false); }} className="relative">
            <UserAvatar name={navUser?.display_name || "User"} size="md" className="cursor-pointer hover:ring-2 hover:ring-nb-yellow transition-all" />
          </button>

          {/* User dropdown */}
          {showUserMenu && (
            <div className="absolute bottom-full left-full ml-2 mb-2 w-56 bg-nb-white border-2 border-nb-black rounded-xl shadow-neo-lg p-2 z-50">
              {/* User info */}
              <div className="px-3 py-2 border-b border-nb-light mb-1">
                <p className="font-black text-[12px] text-nb-black">{navUser?.display_name || "User"}</p>
                <p className="text-[10px] text-nb-gray">{navUser?.email || ""}</p>
              </div>

              {/* Current org + switcher */}
              {(currentOrg || orgs.length > 0) && (
                <div className="border-b border-nb-light mb-1 pb-1">
                  <button
                    onClick={() => setShowOrgPicker(!showOrgPicker)}
                    className="flex items-center justify-between w-full px-3 py-2 rounded-lg text-[12px] font-bold text-nb-black hover:bg-nb-bg transition-colors"
                  >
                    <span className="flex items-center gap-2">
                      <Building2 className="w-3.5 h-3.5" />
                      {currentOrg?.name || "Select Org"}
                    </span>
                    <ChevronRight className={cn("w-3 h-3 transition-transform", showOrgPicker && "rotate-90")} />
                  </button>

                  {/* Org list */}
                  {showOrgPicker && orgs.length > 0 && (
                    <div className="ml-3 mt-1 space-y-0.5">
                      {orgs.map((org) => (
                        <button
                          key={org.id}
                          onClick={() => handleSwitchOrg(org.id)}
                          className={cn(
                            "flex items-center justify-between w-full px-3 py-1.5 rounded-lg text-[11px] transition-colors",
                            currentOrg?.id === org.id
                              ? "bg-nb-yellow/15 font-bold text-nb-black"
                              : "text-nb-gray hover:bg-nb-bg hover:text-nb-black"
                          )}
                        >
                          <span>{org.name}</span>
                          {currentOrg?.id === org.id && <Check className="w-3 h-3 text-nb-yellow" />}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              )}

              <Link href="/settings/account" className="flex items-center gap-2 px-3 py-2 rounded-lg text-[12px] font-bold text-nb-black hover:bg-nb-bg transition-colors" onClick={() => setShowUserMenu(false)}>
                <User className="w-3.5 h-3.5" />Profile
              </Link>
              <Link href="/settings/api-keys" className="flex items-center gap-2 px-3 py-2 rounded-lg text-[12px] font-bold text-nb-black hover:bg-nb-bg transition-colors" onClick={() => setShowUserMenu(false)}>
                <Key className="w-3.5 h-3.5" />API Keys
              </Link>
              <button
                onClick={() => { logout(); setShowUserMenu(false); }}
                className="flex items-center gap-2 px-3 py-2 rounded-lg text-[12px] font-bold text-nb-red hover:bg-nb-red/10 transition-colors w-full text-left"
              >
                <LogOut className="w-3.5 h-3.5" />Sign Out
              </button>
            </div>
          )}
        </div>
      </nav>

      {/* Settings Sub-Nav */}
      {isSettingsPage && (
        <div className="w-[200px] bg-nb-white border-r border-nb-light py-4 px-3">
          <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray px-3 mb-3">Settings</p>
          {settingsSubNav.map((item) => {
            const isActive = pathname === item.href;
            const Icon = item.icon;
            return (
              <Link
                key={item.href}
                href={item.href}
                className={cn(
                  "flex items-center gap-2.5 px-3 py-2 rounded-xl text-[12px] font-bold transition-all mb-0.5",
                  isActive ? "bg-nb-yellow/15 text-nb-black border border-nb-yellow/40" : "text-nb-gray hover:bg-nb-bg border border-transparent"
                )}
              >
                <Icon className="w-4 h-4" />{item.label}
              </Link>
            );
          })}
        </div>
      )}
    </div>
  );
}
