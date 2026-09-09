import { bots as en } from "../en/bots";

export const bots: typeof en = {
  enabled: "已启用",
  disabled: "已禁用",
  long: "多",
  short: "空",
  continueLabel: "继续",

  // --- Bots：列表页 ---
  list: {
    title: "机器人",
    summary: (total, running, starting) =>
      `${total} 个机器人 · ${running} 个运行中` +
      (starting ? ` · ${starting} 个启动中` : "") +
      " · 时间加权收益率，已归一化",
    loadingWhat: "机器人列表",
    addBot: "添加机器人",
    th: {
      bot: "机器人",
      trend: "走势",
      ret: "收益率",
      actual: "实际",
      desired: "期望",
      config: "配置",
      runtime: "运行时",
    },
    empty: "还没有机器人，添加一个即可开始。",
    note:
      "“期望”是你设定的状态，“实际”是 ECS 最近上报的任务状态。" +
      "已启用但已停止的机器人会自动重启。",
  },

  // --- BotDetail：头部、图表、配置与危险操作 ---
  detail: {
    loadingWhat: "机器人",
    loadingChart: "图表",
    startRequested: "已请求启动，任务稍后会上报为运行中。",
    alreadyRunning: "机器人已在运行中。",
    alreadyStarting: "机器人正在启动中。",
    desiredRuntime: (enabled, runtime) => (
      <>
        期望：{enabled}
        {" · "}运行时：{runtime}
      </>
    ),
    taskObserved: (when) => `上次观测到任务：${when}`,
    noTaskObserved: "尚未观测到任务",
    stopBot: "停止机器人",
    runBot: "启动机器人",
    returnTile: (window) => `收益率 · ${window}`,
    maxDrawdownTile: (window) => `最大回撤 · ${window}`,
    leverage: "杠杆",
    configSwitches: "切换配置次数",
    cumulativeReturn: "累计收益率",
    noReturnData: "该机器人还没有收益数据。",
    noReturnDataHint: "机器人开始交易后，每日采集任务会发布收益序列。",
    switchDot: "橙色圆点表示切换配置。",
    configuration: "配置",
    template: "模板",
    tunedOn: "调优于",
    tunedOnValue: (source) => `${source} 数据`,
    strategy: "策略",
    strategyEntry: (name, side) => `${name}（${side}）`,
    sides: "方向",
    sidePillLong: (on) => `多 ${on ? "开启" : "关闭"}`,
    sidePillShort: (on) => `空 ${on ? "开启" : "关闭"}`,
    riskLevel: "风险等级",
    riskValue: (long, short) => `多 ${long.toFixed(2)} · 空 ${short.toFixed(2)}`,
    coins: "币种",
    noConfig: "还没有配置，选择一个后机器人才能运行。",
    changeConfig: "更换配置",
    runtime: "运行时",
    configHint: "改动会在机器人下次启动时生效。正在运行的任务仍使用启动时的配置。",
    balance: "余额",
    balanceHint: "余额查询尚未提供。",
    dangerZone: "危险操作",
    unstuck: "解套",
    deleteBot: "删除机器人和 API Key",
    deleteHint: "删除时需要输入机器人 ID，这会移除已存储的 API Key 和配置，但不会改动交易所账户。",
  },

  // --- BotDetail：停止确认 ---
  stopModal: {
    title: "停止这个机器人？",
    body: "任务会停止，机器人被标记为已禁用，不再自动重启。已有持仓按原样留在交易所。",
    stopped: "已请求停止。",
    notRunning: "机器人当时未在运行，现已标记为已禁用。",
    alreadyStopping: "机器人正在停止中。",
  },

  // --- BotDetail：删除确认，需要照抄机器人 ID ---
  deleteModal: {
    body: (name) => (
      <>
        这会删除 <b>{name}</b>{" "}
        及其配置和已存储的交易所密钥，且无法撤销。输入机器人 ID 以确认：
      </>
    ),
    placeholder: "机器人 ID",
  },

  // --- BotDetail：应用模板 ---
  templateModal: {
    choose: "选择一个模板…",
    currentSuffix: "（当前）",
    levelSuffix: (level) => ` · VIP ${level}+`,
    warning: (name) => (
      <>
        切换到 <span className="mono">{name}</span>{" "}
        会用该模板的策略、方向、币种和风险设置覆盖当前设置，并在下次启动时生效。
      </>
    ),
    apply: (name) => `应用 ${name}`,
    applied: (name) => `配置 ${name} 已应用，将在下次启动时生效。`,
  },

  // --- BotDetail：每个方向的钱包敞口 ---
  riskModal: {
    hint: "每个方向的钱包敞口上限。杠杆按 max(多, 空) + 1 推导。下次启动时生效。",
    saved: (long, short) => `风险等级已设为 多 ${long.toFixed(2)} · 空 ${short.toFixed(2)}。`,
  },

  // --- BotDetail：启用或禁用某个方向 ---
  sidesModal: {
    hint: "启用或禁用策略的某个方向。下次启动时生效。",
    saved: (long, short) => `方向已设置：多 ${long ? "开启" : "关闭"} · 空 ${short ? "开启" : "关闭"}。`,
  },

  // --- BotDetail：机器人启动所用的镜像 ---
  runtimeModal: {
    hint: "机器人在其引擎线内启动时使用的镜像。正在运行的任务仍使用启动时的二进制。",
    saved: (runtime) => `运行时已设为 ${runtime}。`,
  },

  // --- AddBot：名称、API Key、API Secret ---
  add: {
    title: "添加机器人",
    lead: "每个交易所子账户对应一个机器人。密钥加密存储，不会再次显示。",
    steps: {
      name: "名称",
      apiKey: "API Key",
      secret: "API Secret",
    },
    nameLabel: "机器人名称",
    keyLabel: "Bybit API Key",
    keyPlaceholder: "粘贴 API Key",
    keyHint: (ip) => <>需要读取 + 交易权限。IP 白名单：{ip}。</>,
    egressFallback: "账户页面上显示的出口 IP",
    secretLabel: "API Secret",
    secretPlaceholder: "粘贴 API Secret",
    secretHint: "通过 TLS 一次性发送给 API，加密存储，不会回显。",
    replaceKey: "替换已存储的密钥",
    conflict: (name) => (
      <>
        <b>已存在名为“{name}”的机器人。</b>
        继续将替换它已存储的 API Key，配置和历史记录保留。
      </>
    ),
  },
};
