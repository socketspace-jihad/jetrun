import type { BuildStatus } from "@/types";

/**
 * Demo mode data — spawned when NEXT_PUBLIC_DEMO=true
 * By default the app hits real APIs. Pass --demo flag or set the env var to use this.
 */

export const DEMO_ENABLED = process.env.NEXT_PUBLIC_DEMO === "true";

// ── Dashboard ──

export const demoRecentBuilds = [
  {
    id: "1",
    pipeline: "api-service",
    number: 142,
    status: "success" as BuildStatus,
    branch: "main",
    commit: "a3f9c1d",
    duration: "1m 23s",
    time: "2m ago",
  },
  {
    id: "2",
    pipeline: "web-frontend",
    number: 89,
    status: "running" as BuildStatus,
    branch: "feat/auth",
    commit: "e7b2f4a",
    duration: "—",
    time: "just now",
  },
  {
    id: "3",
    pipeline: "worker-service",
    number: 67,
    status: "failed" as BuildStatus,
    branch: "fix/timeout",
    commit: "9d1c3e8",
    duration: "45s",
    time: "15m ago",
  },
  {
    id: "4",
    pipeline: "api-service",
    number: 141,
    status: "success" as BuildStatus,
    branch: "main",
    commit: "f2a8b7c",
    duration: "1m 18s",
    time: "1h ago",
  },
  {
    id: "5",
    pipeline: "deploy-prod",
    number: 23,
    status: "queued" as BuildStatus,
    branch: "main",
    commit: "a3f9c1d",
    duration: "—",
    time: "just now",
  },
];

export const demoStats = [
  { label: "Total Builds", value: "1,247", change: "+12%", color: "bg-nb-yellow" },
  { label: "Success Rate", value: "94.2%", change: "+2.1%", color: "bg-nb-green" },
  { label: "Avg Duration", value: "1m 34s", change: "-18%", color: "bg-nb-blue" },
  { label: "Cache Hit Rate", value: "87.5%", change: "+5.3%", color: "bg-nb-purple" },
];

// ── Pipelines ──

export const demoPipelines = [
  {
    id: "1",
    project_id: "p1",
    name: "api-service",
    description: "Main API service — build, test, deploy",
    config_path: ".jetrun/pipeline.yml",
    active: true,
    created_at: "2024-01-15T10:00:00Z",
    updated_at: "2024-03-10T14:30:00Z",
    lastBuild: {
      id: "b1",
      pipeline_id: "1",
      number: 142,
      status: "success" as BuildStatus,
      trigger: "push" as const,
      commit_sha: "a3f9c1d82e",
      branch: "main",
      stages: [],
      started_at: "2024-03-10T14:28:00Z",
      finished_at: "2024-03-10T14:29:23Z",
      created_at: "2024-03-10T14:28:00Z",
    },
  },
  {
    id: "2",
    project_id: "p1",
    name: "web-frontend",
    description: "Next.js frontend build and preview deploy",
    config_path: ".jetrun/pipeline.yml",
    active: true,
    created_at: "2024-02-01T10:00:00Z",
    updated_at: "2024-03-10T15:00:00Z",
    lastBuild: {
      id: "b2",
      pipeline_id: "2",
      number: 89,
      status: "running" as BuildStatus,
      trigger: "push" as const,
      commit_sha: "e7b2f4a91c",
      branch: "feat/auth",
      stages: [],
      started_at: "2024-03-10T15:00:00Z",
      finished_at: null,
      created_at: "2024-03-10T15:00:00Z",
    },
  },
  {
    id: "3",
    project_id: "p1",
    name: "worker-service",
    description: "Background worker service",
    config_path: ".jetrun/pipeline.yml",
    active: true,
    created_at: "2024-02-15T10:00:00Z",
    updated_at: "2024-03-10T13:00:00Z",
    lastBuild: {
      id: "b3",
      pipeline_id: "3",
      number: 67,
      status: "failed" as BuildStatus,
      trigger: "push" as const,
      commit_sha: "9d1c3e8b4f",
      branch: "fix/timeout",
      stages: [],
      started_at: "2024-03-10T12:59:15Z",
      finished_at: "2024-03-10T13:00:00Z",
      created_at: "2024-03-10T12:59:15Z",
    },
  },
  {
    id: "4",
    project_id: "p2",
    name: "deploy-prod",
    description: "Production deployment pipeline",
    config_path: ".jetrun/deploy.yml",
    active: true,
    created_at: "2024-01-20T10:00:00Z",
    updated_at: "2024-03-09T10:00:00Z",
  },
];

// ── Users ──

export const demoUsers = [
  {
    id: "u1",
    email: "admin@jetrun.local",
    username: "admin",
    display_name: "Super Admin",
    role: "super_admin",
    role_id: "r1",
    is_active: true,
    auth_provider: "local",
    last_login_at: "2024-03-10T15:30:00Z",
    joined_at: "2024-01-01T00:00:00Z",
  },
  {
    id: "u2",
    email: "dev@company.com",
    username: "jdoe",
    display_name: "John Doe",
    role: "Developer",
    role_id: "r3",
    is_active: true,
    auth_provider: "github",
    last_login_at: "2024-03-10T12:00:00Z",
    joined_at: "2024-01-15T00:00:00Z",
  },
  {
    id: "u3",
    email: "viewer@company.com",
    username: "janesmith",
    display_name: "Jane Smith",
    role: "Viewer",
    role_id: "r4",
    is_active: true,
    auth_provider: "google",
    last_login_at: "2024-03-09T10:00:00Z",
    joined_at: "2024-02-01T00:00:00Z",
  },
  {
    id: "u4",
    email: "old@company.com",
    username: "olduser",
    display_name: "Old User",
    role: "Developer",
    role_id: "r3",
    is_active: false,
    auth_provider: "local",
    last_login_at: "2024-01-15T10:00:00Z",
    joined_at: "2024-01-01T00:00:00Z",
  },
];

// ── Account ──

export const demoCurrentUser = {
  id: "u1",
  email: "admin@jetrun.local",
  username: "admin",
  display_name: "Super Admin",
  avatar_url: null as string | null,
  role: "super_admin",
  auth_provider: "local",
  email_verified: true,
  created_at: "2024-01-15T10:00:00Z",
};

export const demoSessions = [
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

// ── Settings ──

export const demoCacheStats = {
  total_entries: 1247,
  total_size_bytes: 2_147_483_648,
  hit_count: 8934,
  miss_count: 1203,
  hit_rate: 0.881,
  eviction_count: 342,
};

export const demoServices = [
  { name: "gateway", status: "healthy", port: 8080, uptime: "3d 14h" },
  { name: "engine", status: "healthy", port: 9001, uptime: "3d 14h" },
  { name: "worker", status: "healthy", port: 9002, uptime: "3d 14h" },
  { name: "cache", status: "healthy", port: 9003, uptime: "3d 14h" },
  { name: "auth", status: "healthy", port: 9004, uptime: "3d 14h" },
];

// ── Nav ──

export const demoNavUser = {
  display_name: "Super Admin",
  email: "admin@jetrun.local",
  role: "super_admin",
};
