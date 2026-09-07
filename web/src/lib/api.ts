const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

async function fetchApi<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: {
      "Content-Type": "application/json",
      ...options?.headers,
    },
    ...options,
  });

  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${res.statusText}`);
  }

  return res.json();
}

export const api = {
  // Pipelines
  listPipelines: () => fetchApi<{ pipelines: unknown[] }>("/api/v1/pipelines"),
  getPipeline: (id: string) => fetchApi<{ pipeline: unknown }>(`/api/v1/pipelines/${id}`),
  createPipeline: (config: unknown) =>
    fetchApi<{ pipeline: unknown }>("/api/v1/pipelines", {
      method: "POST",
      body: JSON.stringify(config),
    }),
  deletePipeline: (id: string) =>
    fetchApi(`/api/v1/pipelines/${id}`, { method: "DELETE" }),

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
