# 方案草案：成员 VIP 分级 + Web 自主注册

状态：裁决已定（2026-09-09），按第 8 节分期开工。技术细节在开发时由实现者与同级模型 review 后定，这里只定边界和不可逆的选择。

已定的裁决：
- 等级 0–9。0 级默认；运营者现有账户为 9 级。0 级可同时启动 1 个 bot，9 级不限；1–8 级为 `level + 1` 个（一个纯函数表，随时可调）。
- 收益曲线不再公开：每个用户只看自己 bot 的曲线（登录后通过 API 读）。
- WorkOS 先留在 Staging。
- 只允许 Google 登录（WorkOS 侧配置）；VIP 管理先只做 ops 脚本；Telegram 与用户 1:1；未绑 Telegram 的 web 用户可用全部功能。
- **身份的主从关系：Google（WorkOS identity）是主身份，不可解绑；Telegram 是从身份，可解绑、可换绑。**

## 0. 现状（决定方案的几个事实）

- 系统里没有「用户」实体。一个用户就是 allowlist 上的一个 Telegram 数字 id，作为 `String` 的 `user_id` 贯穿 DynamoDB pk（`user_id#<id>`）、S3 前缀、ECS 任务的 `USER_ID` 环境变量。Rust 代码里没有任何地方要求它是数字；只有 Terraform 的 `telegram_allowed_user_ids` / `mcp_user_id` 有 `^[0-9]+$` 校验。
- WorkOS 登录已经接好：`identity#workos#<sub>` 行把 WorkOS 的 `sub` 映射到 `user_id`，但**只允许映射到已存在的租户**，未映射的 `sub` 一律 403，不会自动开户。映射由 Telegram 侧的「Link account」发起（bot 出票 → 浏览器登录 → 回调写行）。
- 授权模型只有两层：allowlist 决定能不能进，`bots:read` / `bots:write` 决定能读能写。Telegram 侧连 scope 都没有，进了就是全权限。没有任何「这个功能/模板需要什么资格」的表达位置。
- 模板元数据放在模板 JSON 的 `pbtb` 块里（`name` / `exchange` / `description` / `strategies`），S3 只存 `predefined/<name>.json`。web 与 MCP 对模板只「描述」不返回参数（硬规则）。
- 收益曲线站点的公开 id 是 `sha256(bot_id)[..6]`，**不含 user_id**；bot id 只在租户内唯一。今天只有一个租户所以没炸，多租户后两个用户各有一个叫 `alpha` 的 bot 会互相覆盖对方的公开曲线。

## 1. 身份模型：user_id 不切到 WorkOS id，切到「我们自己的」id

问题是「user_id 要不要从 Telegram id 换成 WorkOS user id」。结论：**都不用，引入内部 opaque id；Telegram id 和 WorkOS sub 都降为 identity。**

| 选项 | 问题 |
| --- | --- |
| 继续用 Telegram id | web 注册时还没有 Telegram id，租户无从建立；与「Google 账号是主标识」直接冲突 |
| 换成 WorkOS `sub` | `sub` 是**按 WorkOS 环境**发的：Staging 切 Production 时每个用户的 `sub` 都变，等于所有租户的 pk 都要迁一遍（docs/workos.md 已记录这个坑）；用户在 WorkOS 侧被删除/合并也会连带失去数据 |
| 内部 id（推荐） | 我们控制生命周期；换 IdP、换 WorkOS 环境、用户换登录方式都只改 identity 行，租户数据不动 |

Google 账号仍然是「用户主标识」——它是唯一的开户入口和登录方式，只是不拿它当存储主键。

**现有租户零迁移。** 内部 id 是 opaque 字符串，现有运营者的 `user_id = "5351347639"` 直接保留为他的内部 id；新用户发 ULID。要补的只是 `identity#telegram#5351347639 → 5351347639` 一行和一行用户资料。DynamoDB 行、S3 前缀、正在跑的 ECS 任务（`USER_ID` 环境变量）、重启 lambda 全部不动。

Telegram 与用户的关系：**1:1**。一个 Telegram 账号只能属于一个用户（条件写，沿用现有 `ClaimedByAnother` 语义）；一个用户最多绑一个 Telegram，换绑先解绑。

## 2. 数据模型

新增两种行，都**不落在 `user_id#` 分区下**，所以不触发 `find_by_user_id` / `find_all` 的形状陷阱（`is_identity_row` 那条 🔴）。

```
用户行     pk = "user#<user_id>", sk = "profile"
           vip_level (N, 默认 0), status (active | suspended), email,
           display_name, created_at, updated_at, created_via ("web")

Telegram   pk = "identity#telegram#<tg_user_id>", sk = "profile"
identity   user_id, linked_at
           （反向行 pk = user_id#<user_id>, sk = identity#telegram#<tg_id>，
            复用现有 identity 反向行，is_identity_row 已按前缀跳过）
```

`IdentityRepository` 已经是 `(provider, subject)` 抽象，`provider = "telegram"` 直接落进去，不改 trait。新增 `UserRepository { get, create, set_vip_level, set_status }`。

绑定票据复用 `LinkTicket` + `purpose = "telegram-bind"`（现有 ticket 表有 `purpose` 隔离和 TTL）。`LinkTicket.chat_id` 目前是写了不读的死字段，这次顺手在绑定成功后用它给用户发确认消息，或者删掉。

模板元数据：模板 JSON `pbtb.min_vip_level`（整数，缺省 0）。放这里是因为模板已经没有别的元数据位置，运营者改一个 S3 文件即可调整门槛，不需要部署。

## 3. 注册流程（仅 web）

```
浏览器 Google 登录（现有 PKCE 流程，不变）
  → 拿到 access token，sub 未 link → 今天是 403 "unlinked"
  → 新增 POST /api/v1/signup（需要 bearer；不需要 scope；幂等）
      服务端：验 token → sub 未 link → 用 access token 调 issuer userinfo 拿 email
             → 生成 ULID → 写用户行(vip_level=0) + identity#workos#<sub> 两行
             → 返回 {user_id, vip_level}
  → 站点把「Access is by invitation」页换成「用 Google 注册」按钮
```

- 只从 web 进：MCP 没有 signup 工具，Telegram 没有开户。「访问未 link 的 sub 一律 403」这条现有原则不变，只是多了一个显式、需要用户点击的开户动作，不是登录即开户。
- 是否只允许 Google：这是 WorkOS 侧配置（AuthKit 认证方式里只开 Google），代码不管登录方式。
- 现有 Telegram 侧「Link account」按钮与 `DELETE /me/identities`（解绑全部身份）**下线**：WorkOS identity 在注册时建立，之后不可解绑；bot→web 方向的 link 流程没有用户了。运营者已有 `identity#workos` 行，不受影响。

## 4. Telegram 绑定流程（web 发起，bot 完成）

方向与现有 link 流程相反，结构对称，复用票据机制：

```
web「绑定 Telegram」 → POST /api/v1/me/telegram/bind-ticket
   服务端为 token 的 user_id 出一张一次性票（sha256 入库，10 分钟），
   返回 https://t.me/<bot>?start=<token>
→ 用户点开，Telegram 发 /start <token>
→ bot：redeem(purpose="telegram-bind") → 得到 user_id
       → link("telegram", msg.from.id, user_id)；ClaimedByAnother 则拒绝并提示
       → 回复「已绑定」
```

解绑与换绑：web 账户页「解绑 Telegram」→ `DELETE /api/v1/me/telegram`；bot 里 `/unlink` 改为解绑调用者自己的 Telegram 身份（之后 bot 不再认识他，可在 web 重新绑定另一个 Telegram）。两者都只删 `provider = telegram` 的行，`workos` 行没有任何删除路径。

选 deep link 而不是 Telegram Login Widget：不需要在 BotFather 配域名、不需要校验 widget 的 HMAC、票据代码已经存在。租户身份仍然在服务端建立（票据），bot 收到的 `/start` 只是把 Telegram id 挂上去，不由浏览器或 Telegram 消息指定租户。

## 5. telebot 鉴权：allowlist → 身份解析

- `reject_unauthorized` 改为：`msg.from.id` → `find_link("telegram", id)`（强一致读，一次 get）→ 有且用户 `status = active` 才放行，并把 `user_id` 注入 `DependencyMap`。
- 十几处 `msg.from().map(|u| u.id.to_string()).unwrap_or("unknown")` 全部换成注入的 `user_id`；`"unknown"` 这个租户桶随之消失。
- 未绑定的人收到的不再是「limited to authorized users」，而是「请在 <站点> 用 Google 注册并绑定 Telegram」。**例外**：`/start <token>` 必须在拦截之前处理，否则绑定流程进不来。
- `APP__TELEGRAM__ALLOWED_USER_IDS` 语义收窄为**管理员**列表（能改别人的 VIP 等级、封禁），不再是准入门槛。`APP__MCP__USER_ID` 在 OAuth 模式下本来就是摆设，只用于开机断言，跟着一起清理。Terraform 那两个数字正则删掉。

## 6. VIP 分级

- 领域层：`User { vip_level: u8, status }`；`Entitlement` 是纯函数表：`max_running_bots(level)`（0→1，1–8→level+1，9→不限）和模板的 `min_vip_level`。分级只比大小，没有名字、没有过期，先不做付费/订阅（YAGNI）。
- 启动配额：`StartBot` use case 在拿 start lock **之前**数租户内 `enabled = true` 的其他 bot（desired state，用户自己控制的那个），达到上限则拒绝 `DomainError::QuotaExceeded { limit }`。重启 lambda 只重启已 enabled 的 bot，本来就在配额内，不重复检查。
- 门禁位置：**use case 层**，三个入口共用。`ApplyTemplateUseCase::preview/execute` 取用户等级与模板 `min_vip_level` 比较，不够返回新增的 `DomainError::InsufficientLevel { required, current }`。读模板（列表、描述）不限等级——列表里带上 `min_vip_level`，界面标锁。
- 各表面的呈现：
  - Telegram：模板键盘上加 🔒 与所需等级，点了提示「需要 VIP n」。
  - API：`403 {error: "insufficient_level", required, current}`；`GET /me` 增加 `vip_level`。
  - MCP：工具错误文本同上；`get_bot_config` 仍只对运营者。
- 管理入口：先做 `scripts/ops/pbtb_ops.py set-vip <user_id> <level>` / `suspend-user`（AWS CLI 直写用户行），Telegram 管理命令或 web 管理页以后再说。

## 7. 开放注册前必须先修的东西

这些不是本需求的一部分，但开放注册会把它们从「潜在」变成「必现」：

1. **收益曲线私有化**：收集器改写到 chart 桶的 `{user_id}/{bot_id}.json`（碰撞随之消失），新增 `GET /api/v1/bots/{id}/returns` 带租户检查读它；站点的 `/returns` 页改为登录后读 API；`pages-publish.yml` 不再把 `charts/` 同步进站点，`site/data/` 里已发布的公开曲线删除。
2. **资源边界**：0 级 1 个并发 bot（第 6 节的配额）。开放注册意味着任何 Google 账号都能在你的 ECS 集群、走你的 NAT 出口 IP 跑真盘任务，费用在你这边；配额是唯一的闸。
3. **WorkOS 环境**：先留 Staging（共享的 Google 测试凭据）。以后切 Production 时，因为用了内部 id，只影响 `identity#workos#*` 行；但 WorkOS identity 不可解绑，所以切环境需要一个运营侧的「按 email 重挂 identity」脚本，不走用户自助。

## 8. 分期（每期一个 PR，可独立合并、独立回滚）

| 期 | 内容 | 依赖 |
| --- | --- | --- |
| P0 | 用户行 + `UserRepository` + `identity#telegram`；ops 脚本为现有租户补行；`find_all` / `find_by_user_id` 形状测试 | 无 |
| P1 | telebot 由 allowlist 改为身份解析 + 注入 `user_id`；`/start <token>` 绑定；未绑定提示 | P0 |
| P2 | `POST /api/v1/signup`、`bind-ticket`、`GET /me` 带 `vip_level`；站点注册页与账户页「绑定 Telegram」 | P0 |
| P3 | VIP：`Entitlement`、启动配额、模板 `min_vip_level`、三表面门禁与呈现、`set-vip` 脚本 | P0 |
| P4 | 收益曲线私有化：收集器按 `{user_id}/{bot_id}` 写、API 读、站点页改登录后读、下线公开同步 | 无，但要在放开注册前合并 |
| 上线 | 部署 mcp-http + telebot → 站点 publish → 关掉「invitation only」文案 | P1–P4 |

P1 与 P2 之间有一个过渡窗口：P1 上线后现有运营者必须先有 `identity#telegram` 行（P0 的脚本），否则 bot 把他当陌生人。部署顺序：P0 脚本先跑，再发 telebot。

## 9. 测试

- DynamoDB Local（`tests/identity_link_test.rs` 扩）：telegram identity 1:1 条件写、bind 票据一次性、用户行不打坏 `find_by_user_id` / `find_all`。
- `tests/e2e_telegram.rs`：陌生 Telegram 用户拿到注册提示而非静默；`/start <token>` 绑定成功/被别人占用/过期；绑定后拿到的 `user_id` 是内部 id 而不是 Telegram id。
- `tests/web_api.rs`：signup 幂等、未 link 的 sub 除 signup 外仍 403、`insufficient_level` 的 403 形状、模板列表带 `min_vip_level` 且仍不含参数。
- `tests/mcp_tools.rs`：注册表里没有 signup / 改等级的工具（沿用「断言不存在」的写法）。
- 领域单测：`Entitlement` 比较、`min_vip_level` 缺省为 0。
