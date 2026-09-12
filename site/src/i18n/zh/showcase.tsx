import { showcase as en } from "../en/showcase";

export const showcase: typeof en = {
  title: "公开展示",
  lead: "运营者自己的机器人的实盘收益曲线，每日从 Bybit 采集。只展示收益率；配置所用本金按量级取整。",
  disclaimer: "真实账户的实盘结果。过往收益不代表未来收益。",
  nothing: "还没有公开的机器人。",
  notFound: "没有这个公开机器人。",
  loading: "公开展示",
  loadingBot: "机器人",
  onBybit: "在 Bybit 查看",
  col: { bot: "机器人", trend: "近 30 天", current: "收益率" },
  currentReturn: "当前收益率",
  updated: (date) => `更新于 ${date} UTC`,
  runs: {
    title: "实盘运行",
    lead: (min, max) => `公开机器人使用该配置连续运行至少 ${min} 天的每一段，从起点重新起算；超过 ${max} 段时只显示最近的 ${max} 段。`,
    ongoing: "进行中",
    caption: (c) => `本金 ${c.cap} · ${c.start} → ${c.end} · ${c.days} 天 · ${c.ret}`,
  },
};
