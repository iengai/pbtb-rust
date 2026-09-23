import { afterEach, describe, expect, it, vi } from "vitest";
import { exchangeLink } from "./exchange";
import { isRetired, parsePublished, staticData } from "./static";

describe("exchangeLink", () => {
  const bybitLink = (url: string | null | undefined) => exchangeLink(url, "bybit");

  it("keeps an https link on bybit.com or a subdomain of it", () => {
    const url = "https://www.bybit.com/copyTrade/trade-center/detail?leaderMark=abc";
    expect(bybitLink(url)).toBe(url);
    expect(bybitLink("https://bybit.com/x")).toBe("https://bybit.com/x");
  });

  it("drops a link with another scheme", () => {
    expect(bybitLink("javascript:alert(1)")).toBeNull();
    expect(bybitLink("http://www.bybit.com/x")).toBeNull();
    expect(bybitLink("data:text/html,<script>alert(1)</script>")).toBeNull();
  });

  it("drops a link on a host that only looks like bybit.com", () => {
    expect(bybitLink("https://evilbybit.com/x")).toBeNull();
    expect(bybitLink("https://bybit.com.example.org/x")).toBeNull();
    expect(bybitLink("https://example.org/?to=bybit.com")).toBeNull();
  });

  it("drops a missing or unparseable link", () => {
    expect(bybitLink(null)).toBeNull();
    expect(bybitLink(undefined)).toBeNull();
    expect(bybitLink("")).toBeNull();
    expect(bybitLink("www.bybit.com/x")).toBeNull();
  });

  it("holds a link to the bot's own exchange", () => {
    const vault = "https://app.hyperliquid.xyz/vaults/0x1111111111111111111111111111111111111111";
    expect(exchangeLink(vault, "hyperliquid")).toBe(vault);
    expect(exchangeLink(vault, "bybit")).toBeNull();
    expect(exchangeLink("https://www.bybit.com/x", "hyperliquid")).toBeNull();
    expect(exchangeLink("https://evilhyperliquid.xyz/x", "hyperliquid")).toBeNull();
    expect(exchangeLink("https://www.bybit.com/x", "binance")).toBeNull();
  });

  it("reads a bot without an exchange as Bybit's", () => {
    expect(exchangeLink("https://www.bybit.com/x", undefined)).toBe("https://www.bybit.com/x");
  });
});

describe("the template audience overlay", () => {
  const published = { name: "tpl-a", audience: null };
  const retired = { name: "tpl-b", audience: "operator" as const };

  it("marks a snapshot template published when the overlay names it and retired when it does not", () => {
    const overlay = parsePublished({ generated_at: 1, published: ["tpl-b"] });
    expect(isRetired(published, overlay)).toBe(true);
    expect(isRetired(retired, overlay)).toBe(false);
  });

  it("leaves every template on the snapshot's mark without an overlay", () => {
    expect(isRetired(published, null)).toBe(false);
    expect(isRetired(retired, null)).toBe(true);
  });

  it("reads a malformed overlay as none", () => {
    for (const value of [null, "x", [], {}, { published: "tpl-a" }, { published: ["tpl-a", 1] }]) {
      expect(parsePublished(value)).toBeNull();
    }
    expect(parsePublished({ published: [] })).toEqual(new Set());
  });
});

describe("staticData.templatesPublished", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("reads the overlay's ids", async () => {
    vi.stubGlobal("fetch", async () => new Response(JSON.stringify({ generated_at: 1, published: ["tpl-a"] })));
    expect(await staticData.templatesPublished()).toEqual(new Set(["tpl-a"]));
  });

  it("gives null for a missing, failed or malformed overlay", async () => {
    for (const reply of [
      async () => new Response("not found", { status: 404 }),
      async () => {
        throw new TypeError("network down");
      },
      async () => new Response("{not json"),
    ]) {
      vi.stubGlobal("fetch", reply);
      expect(await staticData.templatesPublished()).toBeNull();
    }
  });

  it("gives null when the edge does not answer in time", async () => {
    vi.useFakeTimers();
    vi.stubGlobal(
      "fetch",
      (_url: string, init: RequestInit) =>
        new Promise((_resolve, reject) => {
          init.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        }),
    );
    const pending = staticData.templatesPublished();
    await vi.advanceTimersByTimeAsync(3000);
    expect(await pending).toBeNull();
  });
});
