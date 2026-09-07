const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:9004";

async function authFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const token = typeof window !== "undefined" ? localStorage.getItem("jetrun_token") : null;

  const res = await fetch(`${API_BASE}/api/v1${path}`, {
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options?.headers,
    },
    ...options,
  });

  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `Auth error: ${res.status}`);
  }

  return res.json();
}

export interface AuthResponse {
  access_token: string;
  refresh_token: string;
  user: {
    id: string;
    email: string;
    username: string;
    display_name: string | null;
    avatar_url: string | null;
    role: string;
    permissions: string[];
  };
}

export const authApi = {
  register: (data: { email: string; username: string; password: string; display_name?: string }) =>
    authFetch<AuthResponse>("/auth/register", {
      method: "POST",
      body: JSON.stringify(data),
    }),

  login: (email: string, password: string) =>
    authFetch<AuthResponse>("/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),

  refresh: (refresh_token: string) =>
    authFetch<{ access_token: string; refresh_token: string }>("/auth/refresh", {
      method: "POST",
      body: JSON.stringify({ refresh_token }),
    }),

  logout: (refresh_token: string) =>
    authFetch("/auth/logout", {
      method: "POST",
      body: JSON.stringify({ refresh_token }),
    }),

  getMe: () => authFetch<Record<string, unknown>>("/auth/me"),
  updateMe: (data: { display_name?: string; avatar_url?: string }) =>
    authFetch("/auth/me", { method: "PATCH", body: JSON.stringify(data) }),

  changePassword: (current_password: string, new_password: string) =>
    authFetch("/auth/me/password", {
      method: "PUT",
      body: JSON.stringify({ current_password, new_password }),
    }),

  listSessions: () => authFetch<{ sessions: unknown[] }>("/auth/me/sessions"),
  revokeSession: (id: string) =>
    authFetch(`/auth/me/sessions/${id}`, { method: "DELETE" }),

  // API Keys
  listApiKeys: () => authFetch<{ api_keys: unknown[] }>("/auth/api-keys"),
  createApiKey: (data: { name: string; scopes?: string[]; expires_in_days?: number }) =>
    authFetch<{ id: string; key: string }>("/auth/api-keys", {
      method: "POST",
      body: JSON.stringify(data),
    }),
  revokeApiKey: (id: string) =>
    authFetch(`/auth/api-keys/${id}`, { method: "DELETE" }),

  // Admin
  listUsers: () => authFetch<{ users: unknown[] }>("/auth/users"),
  listRoles: () => authFetch<{ roles: unknown[] }>("/auth/roles"),
  listPermissions: () => authFetch<{ permissions: unknown[] }>("/auth/permissions"),
};
