import { account as en } from "../en/account";

export const account: typeof en = {
  title: "账户",
  loading: "账户",
  signIn: {
    title: "登录方式",
    lead: "创建这个账户时使用的 Google 账户。它就是账户的身份，不能更换或解绑。",
    via: "Google 登录 · WorkOS",
  },
  telegram: {
    title: "Telegram",
    lead: "可以在机器人聊天里操作你机器人的 Telegram 账户。每个账户只能绑一个；要换绑先解绑。",
    bound: "Telegram 用户 ID · 已绑定",
    unbind: "解绑",
    none: "还没有绑定 Telegram 账户。",
    bind: "绑定 Telegram 账户",
    ticketLead: (minutes) => `用你要绑定的 Telegram 账户打开下面的链接。链接只能用一次，${minutes} 分钟后失效。`,
    open: "在 Telegram 中打开",
    ifNotOpen: "如果链接打不开，把它携带的 /start 命令发给机器人。",
    sendThis: "在与机器人的私聊里发送这条命令。",
    done: "已完成 — 检查绑定",
  },
  summary: {
    title: "账户",
    id: "账户 ID",
    level: "等级",
    vip: (level) => `VIP ${level}`,
    bots: "机器人",
  },
  egress: {
    title: "交易所出口地址",
    unpublished: "未公布",
    hint: "在你添加的每个 Bybit API Key 上，把这个 IP 加入白名单。",
    askOperator: "向运维索取 NAT 出口 IP，并在你添加的每个 Bybit API Key 上加入白名单。",
  },
  session: {
    title: "会话",
    scopes: (scopes) => `权限范围：${scopes} · 会话在后台自动续期，直到你退出登录`,
    noScopes: "无",
    signOut: "退出登录",
  },
  unbind: {
    title: "解绑 Telegram？",
    body: "已绑定的 Telegram 账户将不能再操作你的机器人。你的账户、机器人和这个登录方式都不受影响，之后可以再绑定另一个 Telegram 账户。",
  },
};
