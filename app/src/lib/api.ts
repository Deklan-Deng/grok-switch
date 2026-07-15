import { invoke } from "@tauri-apps/api/core";

export type TokenProfile = {
  id: string;
  name: string;
  modelId: string;
  apiModel?: string | null;
  configPath: string;
  baseUrl?: string | null;
  modelAlias?: string | null;
  description?: string | null;
  envKey?: string | null;
  apiBackend?: string | null;
  contextWindow?: number | null;
  maxCompletionTokens?: number | null;
  setAsDefault?: boolean;
  tokenSaved?: boolean | null;
  updatedAt: number;
};

export type CommandResult = {
  status: string;
  profiles: TokenProfile[];
  selectedId?: string | null;
  currentId?: string | null;
  discoveredModels: unknown[];
  defaultModelId?: string | null;
  availableModelIds: string[];
  preview?: string | null;
  token?: string | null;
  configText?: string | null;
  configPath?: string | null;
  busy: boolean;
};

export type ProfilePatch = {
  id?: string;
  name?: string | null;
  modelId?: string | null;
  apiModel?: string | null;
  modelAlias?: string | null;
  description?: string | null;
  baseUrl?: string | null;
  envKey?: string | null;
  apiBackend?: string | null;
  contextWindow?: number | null;
  maxCompletionTokens?: number | null;
  setAsDefault?: boolean | null;
  configPath?: string | null;
};

export type CreateProviderInput = {
  name: string;
  modelId: string;
  apiModel?: string | null;
  modelAlias?: string | null;
  description?: string | null;
  baseUrl?: string | null;
  envKey?: string | null;
  apiBackend?: string | null;
  contextWindow?: number | null;
  maxCompletionTokens?: number | null;
  configPath?: string | null;
  setAsDefault?: boolean;
  token?: string | null;
  enable?: boolean;
};

export type HealthResult = {
  profileId: string;
  name: string;
  ok: boolean;
  category: string;
  title: string;
  detail: string;
  hint: string;
  latencyMs?: number | null;
  statusCode?: number | null;
  url?: string | null;
  checkedAt: number;
};

export type SpeedTestResult = {
  profileId: string;
  name: string;
  ok: boolean;
  category: string;
  title: string;
  detail: string;
  hint: string;
  modelsMs?: number | null;
  ttftMs?: number | null;
  totalMs?: number | null;
  statusCode?: number | null;
  is403: boolean;
  isCfBlock: boolean;
  backend?: string | null;
  model?: string | null;
  url?: string | null;
  preview?: string | null;
  streamed: boolean;
  checkedAt: number;
};

export type UsageError = {
  id: string;
  at: number;
  /** Short label, e.g. "网关超时 (524)". */
  title: string;
  /** One-line summary. */
  message: string;
  /** Full raw text for copy. */
  detail: string;
  model?: string | null;
  /** rate_limit | cancelled | api_error | error */
  kind: string;
  logMsg: string;
  sid?: string | null;
};

export type ModelUsage = {
  model: string;
  calls: number;
  promptTokens: number;
  completionTokens: number;
  reasoningTokens: number;
  cachedPromptTokens: number;
};

export type UsageSummary = {
  windowHours: number;
  totalCalls: number;
  promptTokens: number;
  completionTokens: number;
  reasoningTokens: number;
  cachedPromptTokens: number;
  totalTokens: number;
  /** prompt − cached (approx. non-cached input). */
  freshPromptTokens: number;
  avgTokensPerSec?: number | null;
  avgLatencyMs?: number | null;
  avgTtftMs?: number | null;
  /** Real failures (API / parse / rate limit), not user cancel. */
  errorCount: number;
  rateLimitCount: number;
  cancelledCount: number;
  recentErrors: UsageError[];
  byModel: ModelUsage[];
  source: string;
  updatedAt: number;
  hasData: boolean;
};

export function callApi(cmd: string, args?: Record<string, unknown>) {
  return invoke<CommandResult>(cmd, args);
}

export function checkHealth(id: string) {
  return invoke<HealthResult>("check_health", { id });
}


export function runSpeedTest(id: string) {
  return invoke<SpeedTestResult>("run_speed_test", { id });
}

export function lastSpeedTests() {
  return invoke<SpeedTestResult[]>("last_speed_tests");
}

export function getUsageSummary(windowHours = 24, force = false) {
  return invoke<UsageSummary>("usage_summary", { windowHours, force });
}

/** In-process health results from the last probes (no network). */
export function lastHealth() {
  return invoke<HealthResult[]>("last_health");
}
