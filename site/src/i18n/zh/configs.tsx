import { configs as en } from "../en/configs";

export const configs: typeof en = {
  liquidatedBadge: "回测中爆仓",
  wipedOut: "已爆仓",
  retiredBadge: "已退役",
  style: { grid: "网格", martingale: "马丁格尔", ema_anchor: "EMA 锚定" },
  generation: (n) => `第 ${n} 代`,

  list: {
    title: "配置",
    lead: (shown, total, exchanges) =>
      `共 ${total} 个策略模板，显示 ${shown} 个 · 基于 ${exchanges || "交易所"} 数据回测 · 按收益排序`,
    tabs: { published: "已发布", retired: "已退役" },
    allEngines: "全部",
    templates: "模板列表",
    empty: { published: "还没有发布任何模板。", retired: "没有已退役的模板。" },
    maxDd: "最大回撤",
    backtestRange: (start, end, exchange) => `回测 ${start} → ${end} · ${exchange}`,
  },

  detail: {
    template: "模板",
    lead: (exchange, start, end, coins) => `基于 ${exchange} 数据调优 · 回测 ${start} → ${end} · ${coins} 个币种`,
    liquidatedNote: "账户在回测窗口结束前爆仓，以下指标只描述爆仓之前的表现",
    retiredNote: "已退役，仅运营者账户可用",
    applyCta: "应用到机器人…",
    aboutTitle: "关于该策略",
    noDescription: "暂无描述。",
    setupTitle: "设置",
    sides: "方向",
    coins: "币种",
    style: "策略基调",
    generation: "进化代数",
    engine: "引擎",
    engineValue: (version) => `passivbot ${version} · 可在 py 或 rs 上运行`,
    metricsTitle: "回测指标",
    metricsHint: "USD 数值来自 analysis.json",
  },

  chart: {
    title: "回测与实盘",
    aria: "回测与实盘收益曲线",
    brushAria: "期间",
    hint: "每条曲线都从所选期间内自己的第一个点起算 0%。点选预设期间，或在图下方的缩略条上拖动选取任意区间。",
    backtest: "回测权益",
    balance: "回测余额",
    notEnough: "回测数据点不足，暂时画不出曲线。",
  },

  apply: {
    title: (template) => `应用 ${template}`,
    botLabel: "机器人",
    chooseBot: "选择一个机器人…",
    warning: (bot) => (
      <>
        这会用模板的策略、方向、币种和风险设置覆盖 <b>{bot}</b> 现有的配置，并在该机器人下次启动时生效。
      </>
    ),
    proceed: "继续",
    applyTo: (bot) => `应用到 ${bot}`,
    done: (template, bot) => `已把 ${template} 应用到 ${bot}，下次启动时生效。`,
  },

  metric: {
    gain: "收益",
    adg: "日均收益 (ADG)",
    adg_w: "日均收益（加权）",
    drawdown_worst: "最大回撤",
    sharpe_ratio: "夏普比率",
    sortino_ratio: "索提诺比率",
    calmar_ratio: "卡玛比率",
    positions_held_per_day: "每日持仓数",
    position_held_hours_mean: "平均持仓时长（小时）",
    loss_profit_ratio: "亏损/盈利比",
    backtest_completion_ratio: "回测完成度",
  },
};
