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

interface AuthContextType {
  user: User | null;
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  register: (data: { email: string; username: string; password: string }) => Promise<void>;
  logout: () => void;
  hasPermission: (permission: string) => boolean;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const token = localStorage.getItem("jetrun_token");
    if (token) {
      authApi
        .getMe()
        .then((data) => {
          setUser(data as unknown as User);
        })
        .catch(() => {
          localStorage.removeItem("jetrun_token");
          localStorage.removeItem("jetrun_refresh_token");
        })
        .finally(() => setLoading(false));
    } else {
      setLoading(false);
    }
  }, []);

  const handleAuthResponse = useCallback((res: AuthResponse) => {
    localStorage.setItem("jetrun_token", res.access_token);
    localStorage.setItem("jetrun_refresh_token", res.refresh_token);
    setUser(res.user);
  }, []);

  const login = useCallback(
    async (email: string, password: string) => {
      const res = await authApi.login(email, password);
      handleAuthResponse(res);
    },
    [handleAuthResponse]
  );

  const register = useCallback(
    async (data: { email: string; username: string; password: string }) => {
      const res = await authApi.register(data);
      handleAuthResponse(res);
    },
    [handleAuthResponse]
  );

  const logout = useCallback(() => {
    const refreshToken = localStorage.getItem("jetrun_refresh_token");
    if (refreshToken) {
      authApi.logout(refreshToken).catch(() => {});
    }
    localStorage.removeItem("jetrun_token");
    localStorage.removeItem("jetrun_refresh_token");
    setUser(null);
  }, []);

  const hasPermission = useCallback(
    (permission: string) => {
      return user?.permissions.includes(permission) ?? false;
    },
    [user]
  );

  return (
    <AuthContext.Provider value={{ user, loading, login, register, logout, hasPermission }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
