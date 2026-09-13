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
  title: string | null;
  title_zh: string | null;
  style: string | null;
  generation: number | null;
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
  vip_level: number;
  /** `operator` may apply and is listed the operator-only templates; every other account is a member. */
  role: "member" | "operator";
  scopes: string[];
  /** The bound Telegram user id, or null. */
  telegram: string | null;
  identities: { provider: string; subject: string }[];
};

/** A template as the chooser lists it: `min_vip_level` is the lowest level that may apply it,
 *  `audience` says whether everyone or the operator's account alone is offered it. */
export type TemplateListing = {
  name: string;
  title: string | null;
  min_vip_level: number;
  audience: "everyone" | "operator";
};

export type TemplateDescription = {
  name: string;
  version: string | null;
  description: string | null;
  min_vip_level: number;
} & Partial<Omit<ConfigDescription, "risk" | "leverage" | "updated_at">>;

export type StartStatus = "started" | "already_running" | "already_starting";
export type StopStatus = "stopped" | "not_running" | "already_stopping";
