export type BuildStatus =
  | "queued"
  | "running"
  | "success"
  | "failed"
  | "cancelled"
  | "skipped";

export type BuildTrigger =
  | "push"
  | "pull_request"
  | "webhook"
  | "manual"
  | "schedule";

export type LogStream = "stdout" | "stderr" | "system";

export interface Project {
  id: string;
  name: string;
  slug: string;
  repo_url: string;
  default_branch: string;
  config_path: string;
  created_at: string;
  updated_at: string;
}

export interface Pipeline {
  id: string;
  project_id: string;
  name: string;
  description: string | null;
  config_path: string;
  active: boolean;
  created_at: string;
  updated_at: string;
  last_build?: Build;
}

export interface Build {
  id: string;
  pipeline_id: string;
  number: number;
  status: BuildStatus;
  trigger: BuildTrigger;
  commit_sha: string | null;
  branch: string | null;
  stages: BuildStage[];
  started_at: string | null;
  finished_at: string | null;
  created_at: string;
}

export interface BuildStage {
  id: string;
  build_id: string;
  name: string;
  status: BuildStatus;
  steps: BuildStep[];
  started_at: string | null;
  finished_at: string | null;
}

export interface BuildStep {
  id: string;
  stage_id: string;
  name: string;
  status: BuildStatus;
  exit_code: number | null;
  duration_ms: number | null;
  cache_hit: boolean;
  started_at: string | null;
  finished_at: string | null;
}

export interface BuildLog {
  step_id: string;
  line_number: number;
  timestamp: string;
  stream: LogStream;
  content: string;
}

export interface CacheStats {
  total_entries: number;
  total_size_bytes: number;
  hit_count: number;
  miss_count: number;
  hit_rate: number;
  eviction_count: number;
}

// Auth types

export type AuthProviderType = "local" | "google" | "github" | "gitlab" | "saml";

export interface User {
  id: string;
  email: string;
  username: string;
  display_name: string | null;
  avatar_url: string | null;
  auth_provider: AuthProviderType;
  role: string;
  permissions: string[];
  org_id: string | null;
  is_active: boolean;
  email_verified: boolean;
  last_login_at: string | null;
  created_at: string;
}

export interface Organization {
  id: string;
  name: string;
  slug: string;
  owner_id: string;
  created_at: string;
}

export interface Team {
  id: string;
  org_id: string;
  name: string;
  slug: string;
  description: string | null;
  created_at: string;
}

export interface Role {
  id: string;
  name: string;
  display_name: string;
  description: string | null;
  is_builtin: boolean;
  permissions: string[];
  created_at: string;
}

export interface ApiKeyInfo {
  id: string;
  name: string;
  prefix: string;
  scopes: string[];
  created_at: string;
  last_used_at: string | null;
  expires_at: string | null;
  revoked_at: string | null;
}

export interface SessionInfo {
  id: string;
  user_agent: string | null;
  ip_address: string | null;
  created_at: string;
  last_used_at: string;
  expires_at: string;
}
