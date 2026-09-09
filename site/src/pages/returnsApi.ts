import { api, ApiError } from "../api/client";
import type { BotReturnSeries } from "../chart/returnCurve";

// A bot's return series, or null when the collector has not written one yet:
// the API answers 404 for that, which to a page is "no data", not an error.
export async function loadReturns(botId: string): Promise<BotReturnSeries | null> {
  try {
    return await api.botReturns(botId);
  } catch (e) {
    if (e instanceof ApiError && e.status === 404) return null;
    throw e;
  }
}
