import { account as en } from "../en/account";

export const account: typeof en = {
  title: "账户",
  loading: "账户",
  identities: {
    title: "已绑定的身份",
    lead: () => (
      <>
        {"映射到这个 Telegram 账户的登录方式。解绑其中一个，所有登录都会退出。要绑定新的登录方式，请在 Telegram 机器人里点击 "}
        <b>Link account</b>
        {"（绑定账户），这里没有绑定入口。"}
      </>
    ),
    none: "还没有绑定任何身份。",
    via: (provider, current) => `Google 登录 · ${provider}${current ? " · 当前会话" : ""}`,
    unlink: "解绑",
  },
  telegram: {
    title: "Telegram 账户",
    userId: "用户 ID",
    allowlist: "白名单",
    allowed: "已允许",
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
  unlinkAll: {
    title: "解绑全部身份？",
    body: "绑定到这个 Telegram 账户的所有登录方式都会被释放，包括你正在使用的这个。你会在这里退出登录，之后可以从 Telegram 机器人重新绑定。",
  },
};
