"use client";

import { createContext, useContext, useEffect, useState, useCallback, ReactNode } from "react";
import { authApi, AuthResponse } from "@/lib/auth";

interface User {
  id: string;
  email: string;
  username: string;
  display_name: string | null;
  avatar_url: string | null;
  role: string;
  permissions: string[];
}

interface Org {
  id: string;
  name: string;
  slug: string;
  role: string;
  is_owner: boolean;
}

interface AuthContextType {
  user: User | null;
  orgs: Org[];
  currentOrg: Org | null;
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  register: (data: { email: string; username: string; password: string; org_name?: string }) => Promise<void>;
  logout: () => void;
  switchOrg: (orgId: string) => Promise<void>;
  hasPermission: (permission: string) => boolean;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [orgs, setOrgs] = useState<Org[]>([]);
  const [currentOrg, setCurrentOrg] = useState<Org | null>(null);
  const [loading, setLoading] = useState(true);

  const loadOrgs = useCallback(async () => {
    try {
      const res = await authApi.listMyOrgs();
      setOrgs(res.orgs || []);

      // Set current org from JWT
      const token = localStorage.getItem("jetrun_token");
      if (token) {
        try {
          const payload = JSON.parse(atob(token.split(".")[1]));
          const orgId = payload.org_id;
          if (orgId && res.orgs) {
            const found = res.orgs.find((o) => o.id === orgId);
            if (found) setCurrentOrg(found);
          }
        } catch {}
      }
    } catch {}
  }, []);

  useEffect(() => {
    const token = localStorage.getItem("jetrun_token");
    if (token) {
      authApi
        .getMe()
        .then((data) => {
          setUser(data as unknown as User);
          loadOrgs();
        })
        .catch(() => {
          localStorage.removeItem("jetrun_token");
          localStorage.removeItem("jetrun_refresh_token");
        })
        .finally(() => setLoading(false));
    } else {
      setLoading(false);
    }
  }, [loadOrgs]);

  const handleAuthResponse = useCallback((res: AuthResponse) => {
    localStorage.setItem("jetrun_token", res.access_token);
    localStorage.setItem("jetrun_refresh_token", res.refresh_token);
    setUser(res.user);
  }, []);

  const login = useCallback(
    async (email: string, password: string) => {
      const res = await authApi.login(email, password);
      handleAuthResponse(res);
      await loadOrgs();
    },
    [handleAuthResponse, loadOrgs]
  );

  const register = useCallback(
    async (data: { email: string; username: string; password: string; org_name?: string }) => {
      const res = await authApi.register(data);
      handleAuthResponse(res);
      await loadOrgs();
    },
    [handleAuthResponse, loadOrgs]
  );

  const switchOrg = useCallback(
    async (orgId: string) => {
      const res = await authApi.switchOrg(orgId);
      handleAuthResponse(res);
      const found = orgs.find((o) => o.id === orgId);
      if (found) setCurrentOrg(found);
    },
    [handleAuthResponse, orgs]
  );

  const logout = useCallback(() => {
    const refreshToken = localStorage.getItem("jetrun_refresh_token");
    if (refreshToken) {
      authApi.logout(refreshToken).catch(() => {});
    }
    localStorage.removeItem("jetrun_token");
    localStorage.removeItem("jetrun_refresh_token");
    setUser(null);
    setOrgs([]);
    setCurrentOrg(null);
  }, []);

  const hasPermission = useCallback(
    (permission: string) => {
      return user?.permissions.includes(permission) ?? false;
    },
    [user]
  );

  return (
    <AuthContext.Provider value={{ user, orgs, currentOrg, loading, login, register, logout, switchOrg, hasPermission }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
