import { auth as en } from "../en/auth";

export const auth: typeof en = {
  signIn: "登录",
  lead: "在浏览器里管理你的 Passivbot 机器人。",
  unlinkedTitle: "这个 Google 账户还没有绑定到机器人账户。",
  unlinkedBody: () => (
    <>
      打开 Telegram 机器人，点击 <b>Link account</b>（绑定账户），用同一个 Google 账户登录，然后回到这里重新登录。
    </>
  ),
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
  invitationOnly:
    "本控制台仅限受邀使用：你的 Google 账户需要先在 Telegram 机器人里完成绑定，这里不提供注册。",
  callbackFailed: "登录未完成。",
  backToSignIn: "返回登录",
  completing: "正在完成登录…",
};
