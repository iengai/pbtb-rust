import { describe, expect, it } from "vitest";
import { bybitLink } from "./static";

describe("bybitLink", () => {
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
});
