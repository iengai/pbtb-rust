# Web 前端方案（草案）

目标：一个浏览器端的 bot 控制台，Google 登录，覆盖 telebot 的全部功能，外加一个
「配置库」介绍页（每个 predefined 模板一页，带回测曲线）。设计稿用 Claude Design
出，前端继续挂 GitHub Pages。

## 0. 现状盘点（决定了方案的形状）

| 已有 | 状态 | 对 web 的意义 |
|---|---|---|
| WorkOS AuthKit 作 OAuth issuer（Staging `growing-starlight-99-staging.authkit.app`） | 已上线（PR #50/#51） | 登录提供方直接复用，Google 只是 AuthKit 里再开一个社交登录 |
| `OAuthTokens` 验签 → `identity#workos#<sub>` → `user_id` → 允许名单 | 已上线 | web 用户的租户解析**零改动**，同一套三道门 |
| telebot「Link account」把 WorkOS 身份绑到 Telegram 租户 | 已上线 | web 的开户流程 = 先在 Telegram 里点 Link；未关联的 Google 账号登录后被拒，这正是想要的 |
| MCP 工具面（11 个工具） | 已上线 | 覆盖 telebot 功能的 8/12；缺 add bot（要输密钥）、delete API key、balance、unstuck、link/unlink |
| 回报曲线站 `site/`（无依赖 SVG，GitHub Pages，每日刷新） | 已上线 | 成为新 SPA 的一个路由；数据管线不动 |
| `mcp_http` lambda 同时服务 `/`（MCP）、`/link*`、`/.well-known/*` | 已上线，**不在 VPC 内** | REST API 加在同一个 lambda 上；但它没有 NAT 出口，拿不到 Bybit 白名单 IP |
| S3 `predefined/` 下 30 个模板，各自带 `backtest.{exchanges,coins,start_date,end_date}` 和 `pbtb.{name,exchange,description,strategies}` | 已有 | 每个模板的回测可**确定性重跑**，介绍页文案就是 `pbtb.description` |
| 本机 `E:/projects/passivbot` = v8.1.0；模板分两条引擎线（`-v712` 19 个 / `-v810` 6 个 / 旧 xrp 6 个） | — | v7 模板要一个 v7.12 的 worktree 才能回测 |
| WorkOS Staging 里 **没有配置 Google OAuth 凭据**（今天用 API 查过，`GoogleOAuth` 连接为空） | 待配 | 步骤 5 里做 |

telebot 里 **Balance 和 Unstuck 是占位实现**（写死 `$0.00` / "coming soon"）。web 端「对齐 telebot」
就是同样的占位；要做真的 Balance 需要 lambda 进 VPC 走 NAT（Bybit 密钥绑了 IP），单独立项。

## 1. 架构

```
Browser (SPA, GitHub Pages)
   │  1. OAuth 2.1 授权码 + PKCE，resource=<api url>   ──►  WorkOS AuthKit（Google 社交登录）
   │  2. Bearer <access token>
   ▼
mcp_http lambda (Function URL, 加 CORS)
   ├── /                         MCP（不动）
   ├── /link, /link/callback     账号关联（不动）
   ├── /.well-known/...          RFC 9728（不动）
   └── /api/v1/*                 新增：REST，同一个 TokenVerifier，同一批 use case
                                            │
                              DynamoDB / S3 / ECS（不动）
```

### 1.1 为什么是 REST 而不是让浏览器直接打 MCP

- 加 bot 必须收 API key/secret。MCP 面**故意**不收凭据（会进模型上下文），这条线不能破；
  浏览器 → TLS → lambda 没有这个问题。所以至少 `add_bot` 得走非 MCP 路由，
  既然要开 `/api`，其余功能一起放进去，SPA 只对一种协议。
- MCP 的返回是 tool content，REST 直接给 JSON 和 HTTP 状态码，前端省一层翻译。
- 两个入口共用 `Principal` seam 和 use case：租户隔离、start lock、允许名单都是同一段代码，
  不会出现「web 绕过了锁」。

### 1.2 认证细节（这是最大风险点，先做 spike）

- 用 **`/oauth2/authorize` 授权码流 + PKCE**，带 `resource=<function url>`。2026-09-09 已实测：
  这条路签出来的 token `aud` = resource，我们的 `set_audience` 通过；**设备码流不行**，
  `authkit-js` 那种走 AuthKit 自家 `/authorize` 的 SDK 也别用（`aud` 不是我们的资源）。
- SPA 是公开客户端，没有 secret。**待验证：Connect application 能否建成 public client**。
  - 能：SPA 自己换 code（`oauth4webapi` 或 ~100 行手写 PKCE）。
  - 不能：退回「lambda 代换」——`/api/v1/auth/callback` 用已有的 `OAuthClient`
    （`src/interface/link/oauth_client.rs`，它已经持有 client secret）换 code，再把 token 通过
    redirect fragment 交还 SPA。多 30 行，安全性不降。
- token 存 `sessionStorage`，过期用 `prompt=none` 静默续签；不引入 refresh token。
- scope 就是 permission slug（`bots:read bots:write`），给新 Connect app 挂同样两个 permission。
- Google：AuthKit → Authentication → Google OAuth。Staging 可用 WorkOS 测试凭据；
  Production 要自己的 Google Cloud OAuth client。想「只允许 Google」就在 AuthKit 关掉邮箱密码。
- 租户关系不变：Google 登录得到的 WorkOS user 就是 `sub`；这个 `sub` 必须已经从 telebot
  Link 过，否则 401。**Telegram 仍是开户入口**，web 不自助注册——和 `docs/mcp.md` 的原则一致。

### 1.3 REST 面（`/api/v1`）

| 路由 | use case | 对应 telebot 按钮 | scope |
|---|---|---|---|
| `GET /bots` | `list_bots` + `get_bot_runtime` | List | read |
| `GET /bots/{id}` | 同 State 视图的字段（config 摘要、desired/actual、runtime、risk、leverage、coins） | State | read |
| `POST /bots` `{name, api_key, secret_key, overwrite?}` | `add_bot` | Add bot | write |
| `DELETE /bots/{id}` `{confirm: id}` | `delete_bot` | Delete API key | write |
| `POST /bots/{id}/start` / `/stop` | `start_bot` / `stop_bot` | Run bot / Stop bot | write |
| `PUT /bots/{id}/risk` `{long, short}` | `update_risklevel` | Risk level | write |
| `PUT /bots/{id}/sides` `{long, short}` | `set_strategy_side` | Sides | write |
| `PUT /bots/{id}/runtime` `{py\|rs}` | `set_bot_runtime` | Runtime | write |
| `POST /bots/{id}/template` `{name}` | `apply_template` | Choose config | write |
| `GET /templates`, `GET /templates/{name}` | `list_templates` + 模板描述（不含参数） | Choose config 的列表 | read |
| `GET /me` | 身份行 + 允许名单状态 | Link account 的结果 | read |
| `DELETE /me/identities` | `unlink_identities` | /unlink | write |
| `GET /bots/{id}/balance` | 占位（同 telebot） | Balance | read |
| `POST /bots/{id}/unstuck` | 占位（同 telebot） | Unstuck | write |

约束：
- 🔴 `user_id` 只来自 `Principal`，任何路由不接 `user_id`；`POST /bots` 的 body 走
  `src/interface/redaction.rs`，密钥不进日志。
- CORS：Function URL 的 `cors` 块只放 `https://iengai.github.io`，`Authorization` 头，
  无 credentials。Terraform 加一个 `web_origin` 变量。
- 每个写路由和 MCP 一样记 principal / bot id / outcome。
- 测试照 `tests/mcp_http.rs` 的形状：无 token 401、无 scope 403、跨租户 404、密钥不出现在日志。

## 2. 前端

- 位置：`site/` 升级为 Vite + React + TypeScript 工程（`site/src`），保留「构建产物即静态文件」。
  `pages-publish.yml` 加一步 `npm ci && npm run build`，输出目录改为 `site/dist`，
  回报曲线的 `data/` 同步逻辑不变。
- 页面（也就是 Claude Design 要出的画板）：
  1. **登录页**：一个「用 Google 登录」按钮；未关联账号的错误态要解释「先去 Telegram 里点 Link account」。
  2. **Bot 列表**：名字、交易所、desired/actual 相位、runtime；这是 telebot 的 List。
  3. **Bot 详情**：State 视图 + 动作区（Run/Stop/Sides/Risk/Runtime/换模板/删除），
     嵌入该 bot 的回报曲线（复用 `app.js` 的 SVG 逻辑，改成组件）。
  4. **添加 bot**：名字 → key → secret → 同名覆盖确认，三步向导（对应 dialogue 的四个状态）。
  5. **配置库**：模板卡片（名字、引擎线、cap、交易所、币种、回测区间、关键指标）。
  6. **配置详情**：`pbtb.description`、策略侧、币种、引擎、**回测权益曲线** + 指标表。不显示任何策略参数。
  7. **账户**：已关联身份、解绑。
- 图表沿用现在的无依赖 SVG（已经处理了暗色主题、re-base、capital reset），不引入图表库。
- 危险动作（Stop、删除、换模板）二次确认，删除要求手输 bot id，和 MCP 的 `confirm` 语义一致。

### GitHub Pages 够不够

够。理由：SPA 纯静态；OAuth 回调 URI 可以注册成 `https://iengai.github.io/pbtb-rust/callback`；
API 在 lambda 上、有 CORS；仓库是公开的但 bundle 里只有 client_id（本来就是公开值）。
要换的触发条件只有三种：想要私有站点（没必要，API 已鉴权）、自定义域名（Pages 也支持）、
服务端渲染（用不上）。真要换，同一个 terraform 里加 S3 + CloudFront 即可，前端代码不变。

🔴 **配置参数永远不出服务器**（2026-09-09 定）：任何路由都不返回模板或 bot 的完整
config，介绍页只有描述、方向、币种、引擎、回测曲线和指标；产物放 `site/templates/`
（`site/data/` 会被 pages-publish 的 `--delete` 清掉）。注意模板 `pbtb.description`
是作者自己写的调参笔记，里面有参数提示，发布前自己过一遍。

## 3. 模板回测数据管线

- `scripts/backtest_templates.py`：
  1. `aws s3 sync predefined/` 到本地；
  2. 按模板名后缀选引擎（`-v810` → 本机 v8.1.0；`-v712` 和旧 `xrp-*` → 一个 v7.12 的 git worktree）；
  3. 对每个模板跑 `python src/backtest.py <cfg>`（区间、币种、交易所都在模板里，无需参数）；
  4. 从输出目录取 `balance_and_equity.csv.gz` 抽样到 ~500 个点 + `analysis.json` 挑一组指标
     （`adg`、`gain`、`drawdown_worst`、`sharpe/sortino`、`positions_held_per_day` 等），
     写成 `templates/<name>.json`（`{name, engine, exchange, coins, start, end, metrics, points}`）
     和 `templates/index.json`；
  5. 缓存键 = 模板内容 sha256 + 引擎版本，没变的不重跑。
- 现有的 `backtests/` 目录不用挖：它按 pid/时间戳命名，配置只能靠 hash 反推，重跑更可靠。
- 先本机跑（CPU 密集，一次性），产物随代码提交或传 S3；以后需要再上 CodeBuild。

## 4. 分期与 PR

| # | 内容 | 产出 | 依赖 |
|---|---|---|---|
| 0 | **Spike**（半天）：Connect app 公开客户端 + PKCE + `resource` 用 curl 走通；Function URL CORS 预检；Staging 开 Google | 一份 memory 记录结论 | — |
| 1 | **设计**：Claude Design 出 7 个画板 | 设计画布，用户过目 | — |
| 2 | **后端** `feat/web-api`：`src/interface/api/`（REST 路由 + 序列化），`mcp_http` 挂载，CORS，terraform `web_origin` | PR，`tests/web_api.rs` | 0 |
| 3 | **回测管线** `feat/template-backtests`：脚本 + 首批 30 个模板的产物 | PR | — |
| 4 | **前端** `feat/web-console`：Vite 工程、登录、7 个页面、回报曲线迁移 | PR，`pages-publish.yml` 改构建 | 1,2,3 |
| 5 | **上线**：WorkOS（Google、Connect app、redirect URI、resource）；`lambda-deploy.yml -f target=mcp-http`；terraform `-target` 同一组目标 + Function URL；`pages-publish` | deploy-audit 前后各一次 | 2,3,4 |

0/1/3 互不依赖，可以并行开。每个 PR 走 `verify` gate 和 `pbtb-ship`。

## 5. 明确不做 / 留给后面

- 真正的 Balance / Unstuck（telebot 也是占位；Balance 要 lambda 进 VPC）。
- web 自助注册：不做，开户仍从 Telegram 的 Link 开始。
- 多语言、移动端专用布局：设计稿按响应式做，不单独出 app。
- refresh token / 长会话。
