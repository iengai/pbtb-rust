import { returns as en } from "../en/returns";

export const returns: typeof en = {
  title: "收益曲线",
  lead: "你的机器人的时间加权收益率和已实现净利，每日从 Bybit 采集。不展示账户余额。",
  noBots: "你还没有机器人。",
  noData: "这个机器人还没有收益数据——它有交易后，每日采集器会生成曲线。",
  botLabel: "机器人",
  loadingBots: "机器人",
  loadingCurve: "收益曲线",
  tile: {
    return: (range) => `收益率 · ${range}`,
    peak: "峰值",
    days: "天数",
    pnl: (range) => `净利 · ${range}`,
    totalPnl: "累计净利",
  },
  range: {
    sinceRefunding: "自重新注资起",
    total: "全部",
  },
  legend: {
    cumulative: "累计收益率",
    configSwitch: "切换配置",
  },
  chart: {
    aria: "收益曲线",
    returnRow: "收益率",
    pnlRow: "当日净利",
    configRow: "配置",
    switchTitle: (template, date) => `切换配置：${template} · ${date}`,
    periodTitle: (template, from, to) => `${template} · ${from} → ${to}`,
    axisDate: (month, day) => `${month}${day}日`,
    caption: (c) =>
      [
        c.exchange,
        `显示 ${c.days} 天`,
        "时间加权、已剔除出入金",
        ...(c.resetAt ? [`指数于 ${c.resetAt} 重新起算，此前账户爆仓后重新注资`] : []),
        `更新于 ${c.updatedAt} UTC`,
      ].join(" · "),
    empty: {
      noData: "数据还不够，暂时画不出曲线。",
      refunded: (date) => `账户于 ${date} 重新注资，此后的数据还不够，暂时画不出曲线。`,
      refundedHint: "该日期之前的收益，来自已经不存在的本金。",
      wipedOut: (range) => `本窗口（${range}）开始时账户已归零，无法计算收益率。`,
      wipedOutHint: "本金在这个机器人更早的历史里已经亏光。",
    },
  },
};
