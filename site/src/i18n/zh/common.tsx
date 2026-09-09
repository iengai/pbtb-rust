import { common as en } from "../en/common";

export const common: typeof en = {
  nav: {
    bots: "机器人",
    configs: "配置",
    account: "账户",
    signIn: "登录",
  },
  phase: {
    running: "运行中",
    starting: "启动中",
    stopping: "停止中",
    stopped: "已停止",
    unknown: "未知",
  },
  retry: "重试",
  dismiss: "关闭",
  cancel: "取消",
  close: "关闭",
  back: "返回",
  save: "保存",
  confirm: "确认",
  delete: "删除",
  loading: "加载中…",
  loadingWhat: (what) => `正在加载${what}…`,
  networkError: "网络错误，无法连接到 API。",
  notFound: "这里什么都没有。",
  backToBots: "返回机器人列表",
  busyConflict: (phase) => `机器人当前${phase}，请稍后重试。`,
  insufficientLevel: (required, current) => `这个配置需要 VIP ${required}，你的账户是 VIP ${current}。`,
  quotaExceeded: (limit) => `你的等级同时最多运行 ${limit} 个机器人，请先停掉一个。`,
};
