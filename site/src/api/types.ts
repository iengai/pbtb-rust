// Response shapes of `/api/v1` (docs/web-api.md). No route ever carries a
// strategy's parameters or exchange keys, so none of these do.

export type Phase = "starting" | "running" | "stopping" | "stopped";
export type Runtime = "py" | "rs";

export type BotSummary = {
  bot_id: string;
  name: string;
  exchange: string;
  enabled: boolean;
  runtime: Runtime;
  phase: Phase | null;
  created_at: number;
  updated_at: number;
};

export type ConfigDescription = {
  template_name: string;
  template_version: string | null;
  description: string | null;
  tuned_on: string | null;
  config_version: number | string | null;
  strategies: { name: string; side: string }[];
  sides: { long: boolean; short: boolean };
  risk: { long: number; short: number } | null;
  leverage: number | null;
  coins: { long: string[]; short: string[] } | null;
  updated_at: number;
};

export type BotDetail = BotSummary & {
  task_id: string | null;
  observed_at: number | null;
  restarts: number | null;
  config: ConfigDescription | null;
};

export type Me = {
  user_id: string;
  scopes: string[];
  identities: { provider: string; subject: string }[];
};

export type TemplateDescription = {
  name: string;
  version: string | null;
  description: string | null;
} & Partial<Omit<ConfigDescription, "risk" | "leverage" | "updated_at">>;

export type StartStatus = "started" | "already_running" | "already_starting";
export type StopStatus = "stopped" | "not_running" | "already_stopping";
