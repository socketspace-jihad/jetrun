const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

async function fetchApi<T>(path: string, options?: RequestInit): Promise<T> {
  const token = typeof window !== "undefined" ? localStorage.getItem("jetrun_token") : null;

  const res = await fetch(`${API_BASE}${path}`, {
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options?.headers,
    },
    ...options,
  });

  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `API error: ${res.status}`);
  }

  return res.json();
}

function getOrgIdFromToken(): string {
  if (typeof window === "undefined") return "";
  const token = localStorage.getItem("jetrun_token");
  if (!token) return "";
  try { return JSON.parse(atob(token.split(".")[1])).org_id || ""; } catch { return ""; }
}

export const api = {
  // Projects
  listProjects: () => fetchApi<{ projects: unknown[] }>("/api/v1/projects"),
  getProject: (id: string) => fetchApi<Record<string, unknown>>(`/api/v1/projects/${id}`),
  createProject: (data: { name: string; repo_url: string; branch?: string; config_path?: string; org_id?: string }) =>
    fetchApi<{ id: string; webhook_url: string; created: boolean }>("/api/v1/projects", {
      method: "POST",
      body: JSON.stringify(data),
    }),
  deleteProject: (id: string) =>
    fetchApi<{ deleted: boolean }>(`/api/v1/projects/${id}`, { method: "DELETE" }),
  triggerBuild: (projectId: string) =>
    fetchApi<{ status: string; message: string }>(`/api/v1/projects/${projectId}/trigger`, { method: "POST" }),

  // Secrets
  listSecrets: () => fetchApi<{ secrets: unknown[] }>(`/api/v1/secrets?org_id=${getOrgIdFromToken()}`),
  listSecretNames: () => fetchApi<{ secrets: { id: string; name: string; type: string }[] }>(`/api/v1/secrets/names?org_id=${getOrgIdFromToken()}`),
  createSecret: (data: { name: string; secret_type: string; value?: string; generate?: boolean; org_id?: string; created_by?: string }) =>
    fetchApi<{ id: string; name: string; ssh_public_key?: string; created: boolean }>("/api/v1/secrets", { method: "POST", body: JSON.stringify(data) }),
  getSecret: (id: string) => fetchApi<Record<string, unknown>>(`/api/v1/secrets/${id}`),
  deleteSecret: (id: string) => fetchApi<{ deleted: boolean }>(`/api/v1/secrets/${id}`, { method: "DELETE" }),

  // Pipelines (legacy — gateway)
  listPipelines: () => fetchApi<{ pipelines: unknown[] }>("/api/v1/pipelines"),
  getPipeline: (id: string) => fetchApi<{ pipeline: unknown }>(`/api/v1/pipelines/${id}`),

  // Builds
  listBuilds: () => fetchApi<{ builds: unknown[] }>("/api/v1/builds"),
  getBuild: (id: string) => fetchApi<{ build: unknown }>(`/api/v1/builds/${id}`),
  cancelBuild: (id: string) =>
    fetchApi(`/api/v1/builds/${id}/cancel`, { method: "POST" }),
  retryBuild: (id: string) =>
    fetchApi(`/api/v1/builds/${id}/retry`, { method: "POST" }),

  // Cache
  getCacheStats: () => fetchApi<unknown>("/api/v1/cache/stats"),
  purgeCache: () => fetchApi("/api/v1/cache", { method: "DELETE" }),
};
