import { auth as en } from "../en/auth";

export const auth: typeof en = {
  signIn: "登录",
  lead: "在浏览器里管理你的 Passivbot 机器人。",
  expiredNotice: "会话已过期，请重新登录后继续。",
  scopeNotice: () => (
    <>
      上次登录没有同时授予 <span className="mono">bots:read</span> 和{" "}
      <span className="mono">bots:write</span>。请重新登录并同时接受这两项权限。
    </>
  ),
  notConfigured: () => (
    <>
      控制台尚未配置：请设置 <span className="mono">VITE_API_URL</span>、
      <span className="mono">VITE_OAUTH_ISSUER</span> 和 <span className="mono">VITE_OAUTH_CLIENT_ID</span>
      （参见 <span className="mono">.env.example</span>）。
    </>
  ),
  continueWithGoogle: "使用 Google 登录",
  newHere: "第一次来？用 Google 登录后，在下一页创建账户。Telegram 之后在账户页绑定。",
  callbackFailed: "登录未完成。",
  backToSignIn: "返回登录",
  completing: "正在完成登录…",
  signup: {
    title: "创建账户",
    lead: (who) => (
      <>
        你当前以 <b>{who}</b> 登录，这个 Google 账户在这里还没有账户。
      </>
    ),
    whatTitle: "你将获得",
    whatBody:
      "一个属于你自己的空间，初始为 0 级：用自己的交易所 API Key 添加机器人、选择配置，同一时间运行一个机器人。绑定 Telegram 是可选的，之后在账户页完成。",
    create: "创建账户",
    notYou: "不是你？",
    signOut: "退出登录",
    andSignIn: "然后换一个 Google 账户登录。",
    thisAccount: "这个 Google 账户",
  },
};
