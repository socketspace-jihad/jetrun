"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Nav } from "@/components/nav";
import { useAuth } from "@/components/auth-provider";
import { authApi } from "@/lib/auth";
import { DEMO_ENABLED } from "@/lib/demo";
import { Loader2 } from "lucide-react";

export default function AppLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const router = useRouter();
  const { user, loading: authLoading } = useAuth();
  const [checking, setChecking] = useState(!DEMO_ENABLED);

  useEffect(() => {
    if (DEMO_ENABLED) return;
    if (authLoading) return;

    // User is logged in — good to go
    if (user) {
      setChecking(false);
      return;
    }

    // Not logged in — check if platform is set up
    authApi
      .setupStatus()
      .then((res) => {
        if (!res.setup_completed) {
          router.replace("/setup");
        } else {
          router.replace("/login");
        }
      })
      .catch(() => {
        // API unreachable — send to setup (might be first boot)
        router.replace("/setup");
      });
  }, [user, authLoading, router]);

  // Demo mode — no auth checks
  if (DEMO_ENABLED) {
    return (
      <div className="flex h-screen">
        <Nav />
        <main className="flex-1 overflow-y-auto custom-scrollbar">{children}</main>
      </div>
    );
  }

  // Still loading auth state or checking setup
  if (authLoading || checking) {
    return (
      <div className="flex h-screen items-center justify-center bg-nb-bg">
        <Loader2 className="w-8 h-8 text-nb-gray animate-spin" />
      </div>
    );
  }

  // Not logged in — will redirect (handled by useEffect above)
  if (!user) {
    return (
      <div className="flex h-screen items-center justify-center bg-nb-bg">
        <Loader2 className="w-8 h-8 text-nb-gray animate-spin" />
      </div>
    );
  }

  // Logged in — render app
  return (
    <div className="flex h-screen">
      <Nav />
      <main className="flex-1 overflow-y-auto custom-scrollbar">{children}</main>
    </div>
  );
}
