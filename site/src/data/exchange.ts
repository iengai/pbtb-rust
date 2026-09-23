// The exchanges a bot can trade on, as the server spells them. Mirrors
// `Exchange` in src/domain/exchange.rs: the label a person reads, the coin
// money is counted in, and the site a bot's public link may point at.
export type ExchangeId = "bybit" | "hyperliquid";

export const EXCHANGES: readonly ExchangeId[] = ["bybit", "hyperliquid"];

const INFO: Record<ExchangeId, { label: string; quote: string; host: string }> = {
  bybit: { label: "Bybit", quote: "USDT", host: "bybit.com" },
  hyperliquid: { label: "Hyperliquid", quote: "USDC", host: "hyperliquid.xyz" },
};

// Series and bots written before the field existed carry no exchange; every
// one of them was Bybit's.
function info(exchange: string | null | undefined) {
  return INFO[(exchange || "bybit") as ExchangeId] ?? null;
}

export const exchangeLabel = (exchange: string | null | undefined): string =>
  info(exchange)?.label ?? String(exchange);

export const quoteOf = (exchange: string | null | undefined): string => info(exchange)?.quote ?? "";

// A bot's public link as a page may put it in an href: an https URL on its
// exchange's site or a subdomain of it, or null. The link is typed in by the
// operator, and an href runs whatever scheme it is given.
export function exchangeLink(url: string | null | undefined, exchange: string | null | undefined): string | null {
  const site = info(exchange)?.host;
  if (!url || !site) return null;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  const host = parsed.hostname.toLowerCase();
  const onSite = host === site || host.endsWith(`.${site}`);
  return parsed.protocol === "https:" && onSite ? parsed.href : null;
}
