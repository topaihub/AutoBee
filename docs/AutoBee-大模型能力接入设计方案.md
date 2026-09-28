# AutoBee 大模型能力接入设计方案

> 状态:**待评审**。本文档描述尚未实现的能力,不描述现状。
> 评审通过后再进入开发;第 12 节列出需要决策者拍板的问题。

## 0. 背景

AutoBee 现在是一个确定性工具:用户填起点终点、选出行画像、看路线卡。所有能力都要用户自己组合。

要接入的是另一种交互:用户说「我推着婴儿车从孝陵卫去南京南站,顺路想看看附近有没有药店」,模型自己决定调算路还是调生活圈、按什么画像排序,再把结果讲成一句人话。

这件事的承重墙不是「接一个聊天框」,而是**让模型能调用 `route-core` 已有的能力**。聊天框没有 tool calling 就只是个问答窗口,它讲不出 AutoBee 的任何一条真实路线。

### 0.1 调研来源

四个参照项目(同机器,均在 `E:/docs/AI申请/AI安装配置教程/ai/` 下):

| 项目 | 语言 | 看它什么 | 落在本文哪节 |
| --- | --- | --- | --- |
| `voynote-dev` | Rust / Tauri | 中转商配置、SSE 流式、skills 表 | 2.1 – 2.5 |
| `openfang` | Rust | skill 声明工具进 LLM 列表的闭环;三段装配;信任边界 | 2.2.1、2.2.2、7.3.4 |
| `nullclaw` | Zig | 手写 MCP 客户端的协议最小面;按 server 降级;工具名前缀;随机 boundary id | 7.3.1、7.3.3、7.3.4、7.3.5 |
| `pi` | TypeScript | agent 循环;被截断的 tool call 处理 | 5.5 |

本文档区分两类事实:

- **已核实**:直接读源码得到,标注文件与行号。
- **报告来源**:侦察报告结论,未逐行复核,标注「(侦察)」。

第 7 节的依赖成本数字来自对两份 `Cargo.lock` 求依赖闭包差集(实测,非估算),方法与结果见 7.3.2。

## 1. 目标与非目标

### 1.1 目标

1. 用户能在 AutoBee 里配置任意 OpenAI 兼容 / Anthropic 兼容的大模型服务(含中转商),密钥加密落盘。
2. 用户能与模型对话,模型**能真实调用** AutoBee 的算路、地点检索、生活圈三项能力,并基于真实返回值回答。
3. 对话有流式输出,首字延迟接近直连。
4. 可选接入外部 MCP Server(首个目标:百度官方地图 MCP),补齐 AutoBee 没有的天气、路况、行政区划等能力。

### 1.2 非目标

- **不做**多轮 agent 自主规划(自己拆任务、自己循环到满意)。首期是「模型按需调工具,最多若干轮」。
- **不做**把 `route-core` 改成 async。它刻意是 blocking 的,理由写在 `src-tauri/Cargo.toml:2-4`:`cargo test -p route-core` 要能几十秒跑完,不被 GUI 依赖的编译时间拖住。
- **不做**云端账号、对话云同步、多设备。数据全部本地。
- **不做**让模型替代确定性链路。用户点「规划路线」走的还是现在的 IPC,不经过模型。
- **不做**模型写入类操作。首期全部 tool 只读,因此不需要「危险操作确认」流程。

### 1.3 成功判据

| # | 判据 | 验证方式 |
| --- | --- | --- |
| 1 | 填入中转商 base_url + Key 后能拉到模型列表 | 点「获取模型列表」出现真实模型名 |
| 2 | 关掉程序重开,Key 仍在且仍可用;`settings.db` 里是密文 | 十六进制查看 db,搜不到明文 Key |
| 3 | 问「孝陵卫到南京南站怎么走」,模型返回的时长/步行米数与直接点规划一致 | 两条路径结果对比 |
| 4 | 模型回答里出现的坐标不被当成 GPS 讲错 | 检查回答中坐标与 `crs` 标注 |
| 5 | 首字在 1 秒内出现(网络正常) | 肉眼 + 日志时间戳 |
| 6 | 无 Key / 网络失败 / 模型拒答时都有明确中文提示,不白屏 | 手工断网、填错 Key |

## 2. 调研结论:四个参照项目有什么、缺什么

### 2.1 三块能力的可移植性

| 块 | voynote 的实现 | 可移植性 | 对 AutoBee 的真实收益 |
| --- | --- | --- | --- |
| 中转 / 大模型配置 | 三层 providers → endpoints → models,SQLite + DPAPI,三协议 | 直接可搬 | 高,且是其余一切的前提 |
| AI 对话(SSE 流式) | async reqwest + `bytes_stream` 手写 SSE → tokio mpsc → Tauri event | 可搬,需提 async 依赖 | 中,属体验层 |
| skills | SQLite 表 + 勾选启用 + instructions 全文拼进 system prompt,约 260 行 | 照搬容易 | **低,且不解决核心问题** |

### 2.2 skills 那一块为什么收益低

voynote 的运行时技能是**纯提示词注入**,不是工具。(侦察)`ChatRequest` 只有 model / messages / system / max_tokens 四个字段,没有 `tools`;`skill.rs:39-47` 把这条约束直接写成了设计理由。

照搬到 AutoBee 的后果:模型会「知道 AutoBee 能算路」,但调不动 `route-core` 一行代码。它只能凭训练数据编一个看起来像路线的答案 —— 这比不回答更糟,因为用户分辨不出。

结论:skills 可以后期作为「对话风格与领域指引」补上,**但它不是接入大模型能力的路径**。

### 2.2.1 反证:skills 可以是工具(openfang,已核实)

上一节的结论只对 voynote 的实现成立,**不对 skill 机制本身成立**。openfang(`ai/openfang`,Rust)做了完整闭环:

| 环节 | 位置 | 事实 |
| --- | --- | --- |
| skill 声明工具 | `openfang-skills/src/lib.rs:87-95` | `SkillToolDef { name, description, input_schema }` |
| 工具进 LLM 列表 | `openfang-kernel/src/kernel.rs:6241-6251` | 内置 → skill → MCP 三段装配成一个平坦 `Vec<ToolDefinition>`,每段各过一道 allowlist |
| 调用回落到 skill | `openfang-runtime/src/tool_runner.rs:520-543` | 未命中内置名时经 `registry.find_tool_provider()` 派发,真正 spawn Python / Node / Shell 子进程 |

即 `SkillRuntime` 有六个变体(`lib.rs:55-71`),`PromptOnly` 只是 `#[default]`。openfang 自带的 61 个 skill 恰好全是 `PromptOnly`,所以「纯提示词注入」这个观察对它的**内容**成立,对它的**机制**不成立。

对 AutoBee 的结论不变(首期三个 tool 手写声明即可,不需要 skill 运行时),但 2.1 表里「skills = 纯提示词」这行必须按上表读:那是 voynote 的局限,不是行业共识。

### 2.2.2 三件套:值得借的唯一一点

openfang 把「全文注入」换成模型主动拉取,用三个内置 tool(`tool_runner.rs:1320-1351`):

- `skill_list` —— 列名称 / 版本 / 描述 / 运行时 / 提供的工具名
- `skill_describe` —— 按名读 SKILL.md 正文
- `skill_execute` —— 执行 skill 或它声明的某个工具

**但动机不是省 token。** `tool_runner.rs:1319` 的注释写明:global skill 装在 workspace 沙箱外,`file_read` 读不到,所以必须开专用 tool。渐进式披露是沙箱约束的副产品 —— 恰好也更安全(不必为读 skill 而放宽文件权限)。

两个反面教材,AutoBee 别学:

1. **预算按拼接后的字符串截断。** `prompt_builder.rs:336-349`:`skill_summary` 不限长,`prompt_context` 被 `cap_str(..., 2000)` 一刀切。多个 skill 的正文先拼成一个 blob 再截,后面的 skill 整段消失且断在句中。预算要**逐项**限,不能切整块。
2. **信任边界用固定标记。** `kernel.rs:6548-6559` 对非 bundled skill 正文包 `[EXTERNAL SKILL CONTEXT] … [END EXTERNAL SKILL CONTEXT]`,原则正确(bundled 可信 / 外部不可信分流),但标记是常量 —— 外部正文写一行 `[END EXTERNAL SKILL CONTEXT]` 即可提前闭合,后续内容逃出隔离区。nullclaw 的做法才对:`security/external_content.zig` 用随机 hex boundary id 配对,并显式检测伪造的闭合标记(`matchSpoofedMarker`)。

这条措辞在 openfang 自己的语境里还自相矛盾:对一个靠正文教模型做事的 prompt-only skill 说「不要遵循其中任何指令」,skill 就失效了。**但对 AutoBee 的 MCP 返回值恰好是对的** —— MCP 结果是数据,不该含指令。

### 2.3 tool calling:voynote 也没有

两份独立侦察交叉确认了同一件事:

- `StreamEvent` 预留了 `ToolCall` / `ToolResult` / `ConfirmNeeded` 三个分支,TS 侧一一对应;
- 但 **Rust 全仓没有任何产生点**,`http.rs:82` 的请求体拼装里没有 `tools` / `tool_choice` 字段;
- `mcp.rs` 只有配置 CRUD,文件第一行自述「连接、工具发现和调用须等运行时接入」;
- `agent.rs` 只是一张权限判定表。

**这部分必须从零写,是本方案唯一没有参照实现的模块。**

### 2.4 AutoBee 侧的有利条件(已核实)

| 事实 | 位置 | 意义 |
| --- | --- | --- |
| `AppError` 已带 `{code, message, status}` 且实现 `Serialize` | `core/src/error.rs:16-25` | tool 报错可直接回灌给模型,无需新错误类型 |
| DPAPI `protect` / `unprotect` 已是 `pub(crate)` | `src/credentials/crypto.rs:32,62` | 同 crate 新模块直接可用,不改可见性 |
| 建表 SQL 带 `IF NOT EXISTS` 且每次开库无条件执行 | `src/credentials/schema.rs:57-59` | **纯加表不需要 migrate 分支** |
| 归一化模型已是 `#[serde(rename_all = "camelCase")]` | `core/src/models.rs` | 结构体直接可当 tool 的 JSON schema |
| `tokio 1.53.1`、`futures-util 0.3.34`、`hyper 1.11.1`、`uuid 1.26.1` 已在 `Cargo.lock` | 传递依赖 | 加流式**不新增 async runtime**,只是提成直接依赖 |
| `csp: null` | `tauri.conf.json:23` | 前端无 CSP 限制;且 LLM 的 HTTP 走 Rust 侧,不碰浏览器同源 |

### 2.5 需要修正的报告说法

侦察报告有四处与源码不符,以本节为准:

1. 报告称需要 `AppError::field` / `user_input` / `environment` 三类构造器。实际 `AppError` 只有通用 `new(code, message, status)` 加若干语义构造器,**用 `new` 带一个新 code 即可**,不新增类型。
2. 报告把 tool 入口写作 `suggest`。真实签名是 `suggest_places(config: &Config, keyword: &str, page_size: usize) -> Vec<Place>`(`core/src/planner.rs:274`),**返回裸 `Vec` 不是 `Result`**。
3. 报告称加表要走迁移分支。实际只需把 DDL 加进 `CREATE` 常量,`migrate` 里不需要动作 —— 但 `VERSION` 仍要加一(4 → 5),否则老库的 `user_version` 永远停在 4,将来真要改写数据时无从判断这一版跑过没有。见 4.1。
4. 报告未提及 `Segment.points` 是解码后的完整折线 `Vec<LatLng>`(`core/src/models.rs:234`)。这条决定 tool 返回值必须裁剪,见 4.4。

## 3. 架构决策

### 3.1 总体数据流

```mermaid
flowchart TB
  UI[Svelte 对话界面] -->|invoke llm_chat_start| Cmd[commands/llm.rs]
  Cmd --> Agent[agent.rs 工具回合循环]
  Agent -->|HTTP SSE| Http[llm/http.rs]
  Http -->|tools 声明| Vendor[(大模型 / 中转商)]
  Vendor -->|tool_calls 增量| Http
  Http --> Agent
  Agent -->|派发| Tools[llm/tools.rs]
  Tools -->|spawn_blocking| Core[route-core<br/>plan_route / suggest_places / life_circle]
  Tools -->|可选| Mcp[mcp/client.rs → 百度地图 MCP]
  Agent -->|tool 结果回灌| Http
  Agent -->|Tauri event llm:chat:streamId| UI
  Cmd --> Store[llm/store.rs]
  Store --> Db[(settings.db<br/>DPAPI 密文)]
```

### 3.2 决策一:async 边界划在 command 层

`route-core` 保持 blocking。async 边界划在 **command 层**(`src-tauri/src/commands/llm.rs`):
command 是 async,内部一律 `tauri::async_runtime::spawn_blocking` 包住 shell 的同步调用 ——
这与现有地图 command 的写法一致(`commands/map.rs` 已经在这么做)。

**M1 实测修正:`store.rs` 与 `probe.rs` 落在 `src-tauri/shell/src/llm/`,不是 `src-tauri/src/llm/`,
两者都是同步函数。** 原文打算把 `llm/` 整个放 app 侧并用 async reqwest,
但 app 与 shell 都没有 HTTP 客户端,只有 core 有(blocking `reqwest` + `OnceLock` 共享连接池,
`core/src/http.rs:29`)。放 app 侧就得为它引入第二套 HTTP 栈;放 core 里则会让
CLI(`autobee-cli`)连带拖上 tauri。落在 shell 并复用 core 的池子是唯一不新增依赖的位置。

代价:仓库里只有一套 HTTP 写法(core 的 blocking reqwest),
并发靠 command 层的 `spawn_blocking` 拿 —— 比原文预想的两套栈更省。

### 3.3 决策二:配置模型采用三层,但简化一层

voynote 是 providers → endpoints → models 三层。AutoBee 首期合并前两层:

```text
llm_providers   一行 = 一个可用的服务入口(名称 + base_url + 协议 + Key)
llm_models      一行 = 用户从该入口挑中的一个模型
```

理由:voynote 的 endpoints 层是为「同一家服务商多条线路负载均衡」设计的。AutoBee 没有这个需求,合并后少一张表、少一层 UI、少一组 CRUD。若日后需要多线路,再加 `llm_endpoints` 并把 `base_url` 迁过去 —— 那时是一次有明确动机的加表。

「中转商」不是单独的抽象:它就是一个 `base_url` 指向代理、协议选 `openai-chat` 的普通 provider。

### 3.4 决策三:密钥沿用现有 DPAPI 通道

不新建 `credentials` 表。Key 的密文列直接放在 `llm_providers.api_key_enc BLOB`,与 `map_providers` 同构。理由:`map_providers` 已经验证了这个形状(含「解不开一律当没填密钥」的降级,见 `crypto.rs:14-17`),再引一张独立 credentials 表只是多一次 join。

下发前端一律掩码,复用 `logging::mask_secret`。

### 3.5 决策四:tool 只读,因此不做确认流程

首期全部 tool 只查不写,最坏后果是多花一次 API 配额。所以不实现 voynote 预留的 `ConfirmNeeded` 分支。

这条是**可逆的简化**:日后若加「帮我保存这条路线」这类写操作,再引入确认。届时 `StreamEvent` 加一个分支即可,不影响已有结构。

## 4. 数据模型

### 4.1 新增表

全部加进 `src-tauri/shell/src/credentials/schema.rs` 的 `CREATE` 常量(`IF NOT EXISTS`,每次开库无条件执行)。

**`VERSION` 现已是 8。** 实际演进:M1-1 把配置两表加进去时 `VERSION` 走到 5,M1 后续的百度双 AK 列把它推到 6,会话三表推到 7,本轮为历史路线地图快照推到 8。`migrate` 里不需要动作(`CREATE` 的 `IF NOT EXISTS` 已把表补齐),但版本号必须往前走 —— 否则将来真要改写数据时无从判断这一版跑过没有。

**完整折线只写展示快照，不写 `llm_messages`。** `llm_route_snapshots` 以 `conv_id` 为主键保存该会话最后一次成功 `plan_route` 的完整 `RoutePlan`，供关程序重开后重绘地图；它随会话外键级联删除，且永不作为模型上下文发送。`llm_messages.content` 仍只存裁剪摘要。

外键开在 `prepare` 里、建表之前(`schema.rs:99`,`execute_batch(CREATE)` 在 `:102`),不是 `store.rs` 的 `open_at`。理由见 M1-1 任务条,实测已验证。

```sql
CREATE TABLE IF NOT EXISTS llm_providers (
  id          TEXT PRIMARY KEY,     -- uuid
  name        TEXT NOT NULL,        -- 用户起的名字,如「某中转」
  base_url    TEXT NOT NULL,        -- 形如 https://xxx/v1
  protocol    TEXT NOT NULL,        -- anthropic-messages | openai-responses | openai-chat
  api_key_enc BLOB,                 -- DPAPI 密文,NULL = 未填
  enabled     INTEGER NOT NULL DEFAULT 1,
  sort_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS llm_models (
  provider_id TEXT NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
  model       TEXT NOT NULL,        -- 供应商侧的模型标识
  alias       TEXT,                 -- 显示名,NULL 则显示 model
  tools_ok    INTEGER NOT NULL DEFAULT 1,  -- 该模型是否支持 tool calling
  sort_order  INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (provider_id, model)
);

CREATE TABLE IF NOT EXISTS llm_conversations (
  id         TEXT PRIMARY KEY,
  title      TEXT NOT NULL,
  created_at INTEGER NOT NULL,      -- unix 秒
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS llm_messages (
  id         TEXT PRIMARY KEY,
  conv_id    TEXT NOT NULL REFERENCES llm_conversations(id) ON DELETE CASCADE,
  role       TEXT NOT NULL,         -- system | user | assistant | tool
  content    TEXT NOT NULL,
  tool_calls TEXT,                  -- assistant 轮的 tool_calls JSON,NULL = 纯文本
  tool_name  TEXT,                  -- role='tool' 时哪个工具的结果
  model      TEXT,                  -- 哪个模型生成的
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_llm_messages_conv
 ON llm_messages(conv_id, created_at);

CREATE TABLE IF NOT EXISTS llm_route_snapshots (
 conv_id TEXT PRIMARY KEY REFERENCES llm_conversations(id) ON DELETE CASCADE,
 plan_json TEXT NOT NULL,
 updated_at INTEGER NOT NULL
);
```

`ON DELETE CASCADE` 需要 `PRAGMA foreign_keys = ON`。**它已经开了**:`schema.rs:99`,在 `prepare` 里、`execute_batch(CREATE)` 之前。原文「当前 `Store::open_at` 没有开」是 M1-1 之前的状态,M1-1 已落地,11.1 的「破坏性改动」一节随之作废。

### 4.2 不存什么

- 不存 tool 的原始返回值全文。它可能是几十 KB 的折线数据,存了也没人看。只存裁剪后的摘要(见 4.4)。
- 不存 token 用量统计。首期没有配额管理需求。
- 不存模型列表缓存。「获取模型列表」是用户显式点击的动作,结果只进内存供当次挑选。

## 5. tool calling 契约

这是本方案的核心,也是唯一无参照实现的部分。

### 5.1 三个工具

| tool 名 | 包装的 core 函数 | 真实签名(已核实) |
| --- | --- | --- |
| `plan_route` | `planner::plan_route` | `(config: &Config, query: &RouteQuery) -> Result<RoutePlan, AppError>`(`planner.rs:144`) |
| `search_place` | `planner::suggest_places` | `(config: &Config, keyword: &str, page_size: usize) -> Vec<Place>`(`planner.rs:274`) |
| `life_circle` | `life_circle::life_circle` | `(key: &str, origin: LatLng, package: LifeCirclePackage) -> Result<LifeCircle, AppError>`(`life_circle.rs:135`) |

`suggest_places` 返回裸 `Vec` 而非 `Result`:空 `Vec` 就是「没找到」。tool 层要把它翻成一句模型能理解的话,而不是回一个空数组让模型自己猜。

### 5.2 JSON Schema 定义

```jsonc
{
  "name": "plan_route",
  "description": "规划两点之间的路线。返回多个候选方案,含时长、距离、步行距离、换乘次数。地名会自动解析,也接受 \"纬度,经度\" 格式。",
  "parameters": {
    "type": "object",
    "properties": {
      "from": { "type": "string", "description": "起点,地名或 \"纬度,经度\"" },
      "to":   { "type": "string", "description": "终点,同上" },
      "mode": { "type": "string", "enum": ["subway", "bus", "walking"], "description": "出行方式" },
      "sort": { "type": "string", "enum": ["time", "distance"], "description": "排序依据" }
    },
    "required": ["from", "to"]
  }
}
```

`mode` / `sort` 的 enum 值必须与 `models.rs` 的 serde 表示逐字一致(`TravelMode` 是 `rename_all = "lowercase"`,`LifeCirclePackage` 同)。**枚举值写错时模型收到的是反序列化失败,它会重试同样的错误值** —— 所以这些 enum 应从 Rust 侧生成或加单测锁定,不手抄。

```jsonc
{
  "name": "search_place",
  "description": "按关键词搜索地点,返回候选地点及其坐标。用于确认用户说的地名指哪里。",
  "parameters": {
    "type": "object",
    "properties": {
      "keyword": { "type": "string" },
      "limit":   { "type": "integer", "description": "最多返回几个,默认 5,上限 10" }
    },
    "required": ["keyword"]
  }
}
```

```jsonc
{
  "name": "life_circle",
  "description": "查询某地 15 分钟步行可达的服务点。每个点的步行时间都经真实步行路线验证,不是直线距离估算。",
  "parameters": {
    "type": "object",
    "properties": {
      "location": { "type": "string", "description": "中心点,地名或 \"纬度,经度\"" },
      "package":  { "type": "string", "enum": ["care", "family", "emergency"],
                    "description": "care=医院药店卫生间, family=公园便利店地铁, emergency=应急设施" }
    },
    "required": ["location", "package"]
  }
}
```

`life_circle` 的 core 函数要的是 `LatLng` 不是字符串。tool 层负责把地名先过 `suggest_places` 解析成坐标 —— 这一步不能交给模型(它会编坐标)。

### 5.3 坐标系:必须显式,这是静默错误的唯一来源

`models.rs:41-42` 写明各家坐标系不同:腾讯/高德 GCJ-02、百度 BD-09、天地图 CGCS2000、HERE/Google WGS-84。`planner.rs:223` 写明用户直接粘的裸坐标按 WGS-84 处理。

**风险**:模型拿到 BD-09 的经纬度,当成 GPS 坐标讲给用户,偏差在百米级。这个错误**不报错、不崩溃、日志里也看不出来**,只有用户按那个坐标走错了才发现。

**对策**(三条都要):

1. tool 返回的每个含坐标的结构都必须带 `crs` 字段,**不允许 `skip_serializing_if`**。
2. tool description 里明确写:坐标仅用于再次调用工具,不要直接念给用户。
3. 在 system prompt 里加一条硬约束:回答中不出现裸经纬度数字,地点一律用名称指代。

### 5.4 返回值裁剪

`Segment.points` 是解码后的完整折线(`models.rs:234`,`Vec<LatLng>`)。一条跨城公交路线可能有上千个点,直接塞给模型会一次吃掉上下文,并且**模型对折线坐标完全无用** —— 它不画图。

裁剪规则:

| 字段 | 处理 |
| --- | --- |
| `Segment.points` | **整个丢弃**。折线只给前端画图用,不进模型 |
| `Route.segments` | 保留结构,每段只留 `mode` / `duration` / `distance` / 上下车站名 / 方向 |
| `Segment.instruction` | 保留,这是模型讲给用户的核心内容 |
| `RoutePlan.routes` | 最多前 3 条。用户看不过来更多,模型也讲不完 |
| `LifeCircle.pois` | 最多每类 5 个,按步行时长升序 |
| `RoutePlan.crs` / `LifeCircle.crs` | **必须保留**,见 5.3 |

裁剪后的形状单独定义在 `llm/tools.rs`,**不复用前端 DTO**。理由:前端要折线,模型不要;两者共用一个结构会导致改一边坏一边。

### 5.5 回合循环

```mermaid
sequenceDiagram
  participant U as 用户
  participant A as agent.rs
  participant M as 大模型
  participant C as route-core
  U->>A: 「推婴儿车从孝陵卫去南京南站」
  A->>M: messages + tools 声明
  M-->>A: tool_calls: plan_route(from, to, mode)
  A->>C: spawn_blocking(plan_route)
  C-->>A: RoutePlan
  A->>A: 裁剪(5.4)
  A->>M: role=tool 回灌结果
  M-->>A: 流式文本「建议方案 2,步行 420 米...」
  A-->>U: event llm:chat:<id> 逐块
```

**轮次上限 5**。达到上限仍在要求调工具时,停止并告知用户「模型反复调用工具未得出结论」,不无限循环。

**tool 报错也回灌**。`AppError` 已带 `code` + 中文 `message`,直接序列化成 tool 结果发回去 —— 模型能据此换个参数重试(比如把「孝陵卫」改成「孝陵卫地铁站」)。这比在 Rust 侧中断对话有用。

**被 `max_tokens` 截断的 tool call 必须整批作废,不能执行。**(pi,已核实)

`pi/packages/agent/src/agent-loop.ts` 的注释点明这个失败模式:流式 tool-call 参数是「尽力而为」的 JSON 抢救解析器收尾的(`packages/ai/src/utils/json-parse.ts:104-142` 的 `parseStreamingJson`,四级降级),所以被截断的消息**仍然能产出语法合法的 `tool_calls`**。参数解析通过、schema 校验通过,但内容是残缺的。**行号更正:判断分支在 `agent-loop.ts:262-270`,处理函数 `failToolCallsFromTruncatedMessage` 在 `475-500`(`468-474` 是它的文档注释)。**

推论:**安全性只能靠 `finish_reason` / `stop_reason` 判,不能靠「参数能否解析成功」判。** 后者永远会说「能」。

具体到 AutoBee:`plan_route` 的参数被截在 `{"from":"孝陵卫","to":"南京南"` —— 补全后是一个真实存在的地名,算出一条去错地方的路线,然后自信地报给用户。**这正是无声错误**,比报错糟得多。

做法照 pi:检查 `stopReason == "length"`,是则调 `failToolCallsFromTruncatedMessage` 的等价物 —— **不执行任何一个**,把每个 tool call 都作为错误回灌(`agent-loop.ts:475-482`),让模型重新发一次完整的。轮次上限照常计数,防止截断-重试死循环。

### 5.6 protocol 差异

三种协议的 tool 字段形状不同,由 `llm/http.rs` 的 `payload` 分支处理:

| 协议 | tool 声明位置 | 结果回灌 role |
| --- | --- | --- |
| `openai-chat` | `tools: [{type:"function", function:{...}}]` | `role: "tool"` + `tool_call_id` |
| `openai-responses` | `tools: [{type:"function", ...}]`(扁平) | `function_call_output` 项 |
| `anthropic-messages` | `tools: [{name, description, input_schema}]` | `role:"user"` 内含 `tool_result` 块 |

首期**只实现 `openai-chat`**,另两个协议的 tool 分支返回明确错误「该协议暂不支持工具调用」。理由:中转商几乎都是 OpenAI 兼容,一个协议能覆盖绝大多数场景;三个一起做会让第一版的调试面扩大三倍。配置层仍然保留三个协议选项(纯对话可用)。

## 6. IPC 与前端契约

### 6.1 命名约定(已核实)

后端 command 一律 snake_case,前端 `PlannerClient` 方法 camelCase,由 `invokeCmd` 转发(`src/lib/ipc/client.ts:42-55`)。新命令必须加进 `PlannerClient` 接口 —— 它是可注入的,现有测试靠 `fakeClient` 覆盖(`src/lib/state/planner.svelte.test.ts`)。漏加接口等于这条路径没有测试。

### 6.2 新增 command

| command | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `list_llm_providers` | — | `Vec<LlmProviderDto>` | Key 已掩码 |
| `save_llm_provider` | `input: LlmProviderInput` | `Vec<LlmProviderDto>` | 空 Key 表示不改动原 Key |
| `delete_llm_provider` | `id: String` | `Vec<LlmProviderDto>` | 级联删模型行 |
| `fetch_llm_models` | `id: String` | `Vec<String>` | `GET {base_url}/models`,**不落库** |
| `save_llm_models` | `id: String, models: Vec<LlmModelInput>` | `Vec<LlmProviderDto>` | 用户挑完后保存 |
| `list_conversations` | — | `Vec<ConversationDto>` | 按 `updated_at` 降序 |
| `read_conversation` | `id: String` | `Vec<ChatMessageDto>` | 含 tool 轮,供界面回放 |
| `delete_conversation` | `id: String` | — | |
| `llm_chat_start` | `convId: Option<String>, text: String, model: ModelRef` | `String`(streamId) | 立即返回,内容走 event |
| `llm_chat_cancel` | `streamId: String` | — | 用户点停止 |

`llm_chat_start` 的 `convId` 为 `None` 时新建会话,标题取用户首句前 20 字。

### 6.3 流式事件

事件名 `llm:chat:<streamId>`,payload 是一个可辨识联合:

```ts
export type ChatStreamEvent =
  | { kind: "delta"; text: string }
  | { kind: "toolStart"; tool: string; args: string }
  | { kind: "toolEnd"; tool: string; ok: boolean; summary: string; plan?: RoutePlan; circle?: LifeCircle }
  | { kind: "done"; messageId: string }
  | { kind: "error"; code: string; message: string };
```

`toolStart` / `toolEnd` 存在的理由:工具调用期间没有文本输出,可能停顿几秒。界面要能显示「正在查询路线…」,否则用户以为卡死。`summary` 是一句人话(如「找到 3 条路线」),**不是原始 JSON**。

`toolEnd` 上的 `plan` / `circle` 是**未裁剪**的原始结构,只给地图用,不发回模型 —— 理由与两条路径的分工见 6.4.2。

`error` 必须带 `code`:前端按 code 分支(无 Key 跳设置页、网络失败给重试),靠匹配中文会在改文案时静默失效 —— 这与 `core/src/error.rs:3-7` 的既有理由一致。

### 6.4 前端结构与信息架构

#### 6.4.1 决策:对话是工作台的第四个视图,不是独立路由

原方案写「对话页是新的一级入口」并给了 `src/routes/chat/+page.svelte`。**这条推翻。**

否决理由是硬约束,不是审美:`MapCanvas.svelte:3` 直接 `import { planner }`,`:36-40` 只读 `planner.plan` / `planner.selectedRouteId` / `planner.circleResults` / `planner.reachablePois`。对话若是独立路由,那个页面上**没有地图**。模型 tool 辛苦拿回来的路线只能以文字形式存在 —— 而地图是参赛核心性所在(见那份文档 11 节)。一个算得出路线却画不出路线的对话框,比没有对话框更糟。

落点:`src/routes/+page.svelte` 现有的 `type View = "route" | "circle" | "logs"` 加一个 `"chat"`,header 那条 `<nav aria-label="功能模式">`(`:68-74`)加第四个按钮。左栏(`aside`,`lg:w-[400px]`)放对话流,右侧 `main` 的地图**原样保留**。

**nav 顺序:安心换乘 → 15 分钟生活圈 → AI 助手 → 日志。** 对话插在生活圈之后、日志之前。理由:前三项是用户要用的能力,日志是排查用的,本来就靠开关控制显隐(`SettingsDto.show_logs`),排在最后才不打断前三项的阅读顺序。`type View` 的联合成员顺序也照这个排,免得代码顺序与界面顺序对不上。

这同时是 hallmark 的结构要求。`references/structure.md` 的判据是「结构雷同才是 AI 指纹,不是视觉雷同」,而大模型功能的默认吸引子恰好是「整屏居中聊天框」。AutoBee 已有的结构指纹是工作台:**左栏提问,右图作答**。对话是最纯粹的「提问」,放进左栏是让它服从既有契约;另起一个居中聊天页则是在一个应用里塞两套互不相关的结构。

`"logs"` 视图(`:87-89`)是已有的例外 —— 它占满整宽,理由写在那行注释里(每行都长,挤在 400px 里全是折行)。对话**不走这个例外**:它要的正是旁边那张图。

#### 6.4.2 地图回灌契约

上面那个决定换来一个义务:**tool 执行成功后,结果必须写进 `planner` 状态**,否则地图仍然是空的,等于没解决问题。

- `plan_route` 成功 → 写 `planner.plan` 与 `planner.selectedRouteId`,地图按既有 `renderPlan` 自动画线。
- `life_circle` 成功 → 写 `planner.circleResults`,POI 走既有 `renderMarkers`。
- `suggest_places` → 不回灌。它是中间步骤,画一堆候选点只会干扰。

注意这跟 5.4 的裁剪规则**不矛盾,但是两条路**:发回模型的 JSON 不含折线点(省上下文),写进 `planner` 的是**未裁剪的原始结构**(地图要折线才画得出线)。`agent.rs` 里 tool 执行一次,裁剪版进 messages,完整版随 `toolEnd` 事件回前端。因此 6.3 的 `toolEnd` 除 `summary` 外还要带可选的结构化载荷:

```ts
| { kind: "toolEnd"; tool: string; ok: boolean; summary: string; plan?: RoutePlan; circle?: LifeCircle }
```

`summary` 仍是给人看的一句话,新增字段是给地图看的。两者分开,免得前端去解析那句中文。

#### 6.4.3 输入区的状态清单

hallmark `references/interaction-and-states.md` 的八态表在这里不是装饰 —— 对话输入区是全应用**唯一**会长时间处于「在飞」状态的控件(工具轮次可能十几秒),状态漏一个用户就会以为程序死了。

| 状态 | 对话输入区的具体表现 |
| --- | --- |
| Default | 可输入,发送键可用 |
| Hover | 仅在 `@media (hover: hover)` 下生效 |
| Focus | `:focus-visible` 2px 环,**不是** `:focus`;绝不 `outline: none` 不给替代 |
| Active | 发送键按下态 |
| Disabled | 只在「无可用模型」时禁用输入,并给出跳设置的落点;**空闲时绝不禁用** |
| Loading | 发送键换成停止键(对应 `llm_chat_cancel`);输入框**保持可输入**,允许用户先打下一句 |
| Error | 按 `error.code` 分支(无 Key 跳设置、网络失败给重试),`aria-invalid` |
| Success | 不做成功态。回复本身就是确认,再加一个绿勾是噪音 |

两条硬约束,直接抄自那份文档:

- **边框宽度跨状态恒定。** 状态变化只走 `background-color` / `outline` / `box-shadow`。focus 时改 `border-width` 会让整条输入区跳一像素,是最容易被眼睛抓到的「未调校」信号。
- **发送 / 停止键命中区 ≥ 44×44 CSS px**,视觉尺寸不必跟着变(用 padding 或 `::before` 撑)。

工具执行期间的停顿要有具体文案:`toolStart` 到 `toolEnd` 之间显示「正在查询路线…」这类**指名动作**的句子,不是转圈图标。理由与 6.3 里 `toolStart` 存在的理由是同一条。

#### 6.4.4 空态要点名能力

空对话不给「有什么可以帮您?」。hallmark 工作台宏结构的要求是点名输入、拒绝抽象。这里给三句可点击的示例问题,**一句对一个 tool**:

- 「从孝陵卫到南京南站,地铁怎么走最省步行?」→ `plan_route`
- 「钟家村附近有药店吗?」→ `life_circle`
- 「帮我找『光谷大道地铁站』的准确位置」→ `suggest_places`

这三句同时是给用户的能力边界说明:问天气问不出来(除非 MCP 已配,见第 7 节)。比一句「我能帮你规划路线」有用 —— 用户照着点一句就知道该怎么说话了。

`tools_ok = 0` 的模型(12 节问题 8)在这里退化:空态换成「当前模型不支持工具调用,只能纯聊天」,三句示例不显示。**不显示比显示了点不动好。**

#### 6.4.5 文件清单

```text
src/lib/state/chat.svelte.ts              会话状态(消息数组 + 流式累加 + 回灌 planner)
src/lib/components/app/ChatPanel.svelte    左栏对话流 + 输入区
src/lib/components/app/ChatBubble.svelte
src/lib/components/app/ToolTrace.svelte    工具调用的可折叠痕迹
src/lib/components/app/SettingsDialog.svelte  (改)服务商与模型配置作为第三个 section
src/lib/state/llm.svelte.ts
src/routes/+page.svelte                    (改)View 加 "chat",nav 加第四个按钮
```

没有 `src/routes/chat/+page.svelte` —— 这正是 6.4.1 的结论。

**也没有 `src/routes/settings/llm/+page.svelte`,原文这一行是错的。** 仓库里
`src/routes/` 只有 `+page.svelte` 与 `+layout.svelte` 两个文件,设置从来是**弹窗**
(`SettingsDialog.svelte`,地图服务商 + 规划偏好两个 `<section>`)。照原文新建路由
等于在同一个仓库里放两套设置入口,所以大模型配置是加进现有弹窗的第三个 `<section>`。
状态类同理叫 `llm.svelte.ts` 而不是 `llm-providers.svelte.ts` —— 与
`providers.svelte.ts`(地图服务商)并列,后者已经占掉了 providers 这个名字。

### 6.5 Markdown 渲染的 XSS 面

模型输出要按 Markdown 渲染。**若使用 `{@html}`,渲染器必须转义 HTML** —— 模型输出属于不可信数据,提示词注入可以让它吐出 `<script>`。

首期决定:**不用 `{@html}`**。只做最小渲染(段落、换行、代码块、列表),用 Svelte 模板逐块输出纯文本。理由:一个对话框不值得引入一条 XSS 面,而 AutoBee 的回答以短句和数字为主,不需要富文本。

### 6.6 错误码表

`AppError.code` 是前端与模型共用的分支依据,靠匹配中文 `message` 会在改文案时静默失效(`core/src/error.rs:3-7`)。已有与新增:

| code | 来源 | 谁会遇到 | 对方该怎么办 |
| --- | --- | --- | --- |
| `BAD_MODE` | `commands.rs::plan_route`(新增) | 主要是模型 | 按 message 里列出的 `subway / bus / walking` 改参数重试 |
| `BAD_SORT` | `commands.rs::plan_route`(新增) | 主要是模型 | 同上,改成 `time / distance` |
| `BAD_CRS` | `commands.rs::get_life_circle` | 模型 | 改坐标系 |
| `BAD_PACKAGE` | `commands.rs::get_life_circle` | 模型 | 改服务包 |
| `RESOLVE_FAILED` | `error.rs:49` | 两者 | 换更完整的地名重试(如「孝陵卫」→「孝陵卫地铁站」) |
| `NO_ROUTE` | `error.rs:58` | 两者 | 换出行方式或起终点 |
| `CROSS_BORDER` | `error.rs:67` | 两者 | 分别规划境内外路段 |
| `NO_PROVIDER` | `error.rs:76` | 用户 | 前端跳设置页填 Key;模型如实告知「未配置服务商」,**不要编造路线** |
| `ALL_PROVIDERS_FAILED` | `error.rs:85` | 两者 | 全部候选厂商都失败,提示稍后重试 |
| `UPSTREAM` | `error.rs:40` | 两者 | 上游返回错误,含厂商名与原因 |
| `NETWORK` | `error.rs:102` | 两者 | 网络失败,可重试 |
| `NOT_IMPLEMENTED` | `error.rs:94` | 两者 | 该厂商未接入,换一家 |

`BAD_MODE` / `BAD_SORT` 是这次新加的。之前 `plan_route` 对不认识的 `mode` / `sort` **静默兜底**(`_ => TravelMode::Subway`、`else SortBy::Time`):前端只从固定下拉发值所以永不触发,但模型会写 `transit` / `metro` / `地铁` / `fastest`,然后拿到一份看似正常、实为默认参数的结果 —— 它分辨不出错,也就无法重试。这正是 11 节「静默错误」那一类,因此在 tool 接入之前先改成报错。

两个 enum 的合法值由 `TravelMode::ALL` / `SortBy::ALL` 提供,**同一个常量同时用于三处**:tool 的 JSON schema `enum` 字段、错误 message 里的可选值提示、`parse` 的实现。三处共用一个来源,才不会出现 schema 说能填、实现不认的偏差。回归测试:`models.rs` 的 `出行方式与排序拒绝不认识的值而不是猜一个`。

## 7. MCP 接入(第五步,纳入范围)

### 7.0 纳入的理由:它是底座,不是一个功能

初版写「可延后到赛后」,已改为纳入(12 节问题 6)。

翻转的理由不是天气或路况本身 —— 那两项仍然只是演示面上的加分。真正的理由是**扩展形态**:AutoBee 自己的 tool 每增加一个,都要动 `llm/tools.rs` 的 schema、动 `agent.rs` 的分发、重编译、重发版。而 MCP 客户端做完之后,**新增一个外部能力等于在设置页填一个 URL**,一行 Rust 都不用改。

所以 MCP 的投入是一次性的,收益随后续每一个工具线性累加。7.3.1 实测协议面只有五条消息、619 行(nullclaw 已核实),这个底座的价格是已知且很低的 —— 值得现在付,而不是等到第一次「加个工具还得发版」时才付。

**但顺序不变**:M5 仍在 M1–M4 之后。底座的价值不构成先做底座的理由,它得先有一个能跑的对话才有东西可挂。

### 7.1 百度官方 MCP Server 存在(侦察)

- 官方开源仓库 `github.com/baidu-maps/mcp`(MIT)。
- 远程端点 Streamable HTTP `https://mcp.map.baidu.com/mcp?ak=<AK>`,SSE `https://mcp.map.baidu.com/sse?ak=<AK>`。
- 认证就是普通的百度 Web 服务 AK,**AutoBee 已存的 AK 可直接复用**;SSE 需在控制台为该 AK 勾选 MCP 服务项。
- 本地 stdio 方式走 PyPI `mcp-server-baidu-maps` 或 npm `@baidumap/mcp-server-baidu-map`。

### 7.2 重叠与互补

| 能力 | AutoBee 已有 | 百度 MCP | 结论 |
| --- | --- | --- | --- |
| POI 检索 | 有 | 有 | 重叠,用自己的 |
| 步行算路 | 有,且每个生活圈 POI 经真实步行验证 | 有 | 重叠,用自己的 |
| 坐标转换 | 本地 `geo.rs`,零延迟零配额 | 有(V2.3) | 重叠,用自己的 |
| 天气 | 无 | 有 | **互补** |
| 实时路况 | 无 | 有 | **互补** |
| 行政区划边界 | 无 | 有 | **互补** |
| 马克地图分享 / 地图调起 | 无 | 有 | **互补** |
| 跨厂商降级 | 有(`registry.rs` + `circuit.rs`) | 天生没有 | AutoBee 的护城河 |
| 无障碍负担评分 | 有(`accessibility.ts`) | 无此数据 | AutoBee 的护城河 |

**不用 MCP 替代重叠部分**:换来的是非确定性。AutoBee 的生活圈是「每个 POI 都经真实步行验证」的可复现结果,模型驱动的 tool 调用不保证同输入同输出。

### 7.3 技术选择:手写,不引 rmcp

**结论与初版相反,理由是实测数据。**

#### 7.3.1 协议最小面只有五条消息(nullclaw,已核实)

nullclaw(`ai/nullclaw`,Zig)手写了完整 MCP 客户端,`src/mcp.zig` 去掉测试**只 619 行**(总 934 行,315 行是测试)。它实现的全部协议面:

| 步骤 | 消息 | 位置 |
| --- | --- | --- |
| 1 | `initialize`(protocolVersion + capabilities + clientInfo) | `mcp.zig:62-72` |
| 2 | 校验 result 含 `protocolVersion`,否则 `error.InvalidHandshake` | `mcp.zig:74-80` |
| 3 | `notifications/initialized` 通知 | 同上 |
| 4 | `tools/list` | `mcp.zig:525` 起 |
| 5 | `tools/call` | 同文件 |

没有 resources、没有 prompts、没有 sampling。AutoBee 要接的百度官方端点只需要 `tools/list` + `tools/call` 两条。

#### 7.3.2 引 rmcp 的真实成本(实测,非估算)

初版写「约 569 K SLoC」,那是 rmcp 整个依赖闭包的规模,**对 AutoBee 是误导** —— 绝大部分 AutoBee 已经有了。用两份 `Cargo.lock` 求闭包差集实测:

| 量 | 值 |
| --- | --- |
| rmcp 传递依赖闭包 | 197 crate |
| AutoBee 已有 | 169 crate |
| **真实新增** | **28 crate** |

同时修正初版两处错判:

1. **reqwest 双版本不是 rmcp 带来的。** AutoBee 的 lock 里 `reqwest 0.12.28`(route-core)与 `0.13.5`(tauri 2.11.6 自己拉的)**已经共存**。rmcp 要 `^0.13.2`,会复用 tauri 那一份,不新增第三份,也不新增 TLS 栈。
2. **`aws-lc-sys` / `cmake` 不是 rmcp 固有成本。** 它们在 openfang 的 lock 里由 rustls 0.23 的 crypto provider 选择带入。AutoBee 的 rustls 0.23.45 依赖的是 `ring`,lock 里既无 `aws-lc-sys` 也无 `cmake`,便携 MSVC 缺 CMake 的风险不存在。

#### 7.3.3 因此手写

28 个 crate 不算失控,但也买不到什么:要用的只有 `tools/list` 和 `tools/call` 两条消息,而这两条的手写量按 nullclaw 的实测是**百行级**。在 `opt-level = "z"` + `lto = true` + `codegen-units = 1`(`Cargo.toml:65-70`)下,28 个 crate 要付的全量重编时间是每次发版都付的固定成本。

手写还带来一个 SDK 给不了的东西:**按 server 降级**。`mcp.zig:525-529` 在某个 server 的 `tools/list` 失败时 log + `continue`,跳过它,其余照常工作。对 AutoBee 尤其重要 —— 百度端点挂掉时,本地三个 tool 必须继续可用。

#### 7.3.4 工具命名:必须加前缀

`mcp.zig:550` 用 `mcp_{server}_{tool}`。AutoBee 沿用这个形状:`mcp_baidu_<tool>`。

理由是 openfang 的反面教材:`kernel.rs:6241-6251` 把 skill 工具**不加前缀**直接塞进列表,与内置同名时只靠「内置优先匹配」掩盖冲突(`tool_runner.rs:520-543`),等于静默屏蔽。AutoBee 本地三个 tool 名固定,MCP 侧一律带前缀,冲突在命名期就不可能发生。

#### 7.3.5 MCP 返回值是不可信数据

MCP 结果来自第三方 server,属外部内容,必须包裹后才进上下文。**openfang 的做法与 2.2.2 原文的描述不符,已实读更正**:它不是固定字符串标记,而是 `SHA256(url)` 前 6 字节派生的 `EXTCONTENT_{12位hex}`(`crates/openfang-runtime/src/web_content.rs:39-45` 的 `content_boundary`),包裹在 `wrap_external_content`(`:49-57`)。那是**确定性**的 —— 同一个 url 每次得到同一个 id。

AutoBee 取**随机** hex boundary id:确定性 id 可被第三方预测,而 MCP server 的 url 就是它自己,它完全知道自己的 boundary 会是什么。再配合检测伪造的闭合标记(nullclaw `security/external_content.zig` 的 `matchSpoofedMarker` 是可照搬的形状)。


### 7.4 首次接入必做

先用 `tools/list` 打印远程端点的真实 tool 清单。官方文档只给了中文能力名,**没给 tool 标识符**(侦察明确未能查到)。照文档猜名字会得到一个连不上的实现。

## 8. 实施任务清单

六组严格按依赖排序。**每组做完停下来程序都是可用的**。每个任务写明改哪个文件、做什么、怎么算完成;判据是可运行的命令或可观察的现象,不是「写完了」。

### M0 拆 `commands.rs`

这一组**不新增任何行为**。放在最前面是因为 M1 起每一组都要往里加 command,先拆比边加边拆干净。

- [x] **M0-1 拆成目录**(已完成 2026-09-24)
  - `src-tauri/src/commands.rs`(现 360 行)→ `commands/mod.rs` + `commands/map.rs`。现有 12 个 command 全进 `map.rs`,`mod.rs` 只做 `pub mod` 与 `pub use`。
  - `mod.rs` 的重导出**必须是 `pub use map::*;`**,不能逐名列 12 个函数。`#[tauri::command]` 在每个函数旁边还生成了两个隐藏项(`__cmd__xxx` 与 `__tauri_command_name_xxx`),`generate_handler!` 展开后引用的是它们;逐名导出带不走隐藏项,编译报 24 个 `could not find __cmd__xxx in commands`,而错误信息里只字不提宏生成这回事。
  - 这样 `lib.rs` 的 `generate_handler!` 列表才不用改 —— 那份清单的每一行都是前端 `invoke` 的字符串契约,改它等于改契约。
  - 判据:`pnpm verify` 全绿;`pnpm dev:app` 起来后规划一次路线、看一次生活圈。这是纯搬移,单独一个提交,不掺功能代码。

### M1 配置层

- [x] **M1-1 加两张表**(已完成 2026-09-24)
  - `credentials/schema.rs` 的 `CREATE`(现 20-35 行)追加 `llm_providers` / `llm_models` 两段 DDL,照 4.2 的 SQL。
  - `VERSION` 加一。**原文写「现 `schema.rs:14`,值 4」已过期** —— 常量现在在 `schema.rs:9`,值 7(M1-1 推到 5,百度双 AK 列推到 6,会话三表推到 7)。**`migrate` 里不需要对应分支** —— 表是靠 `CREATE` 里的 `IF NOT EXISTS` 补的,老库缺表会在下次开库时建好;但版本号必须往前走,否则将来真要改写数据时无从判断这一版跑过没有。
  - 外键默认是关的,要开。开在 `schema::prepare` 里面(建表之前),**不要开在 `store.rs` 的 `open_at`**:`open_at`(`:40`)与测试用的 `memory()`(`:62`)是两条独立的打开路径,各自调一次 `prepare`(`:52`、`:65`)。只在 `open_at` 加,下面那条级联测试走 `memory()`,外键是关的,测试失败而生产是对的 —— 排查方向会被带偏。
  - 判据实测:`cargo test -p autobee-shell` 全绿 55 条。**反向验证做了**:把 `PRAGMA foreign_keys` 临时改成 `OFF`,两条新测试(级联删除、悬空 `provider_id` 被拒)双双 FAILED;改回 `ON` 全过。不做这一步的话,测试可能只是恰好通过而 `PRAGMA` 压根没生效。
- [x] **M1-2 `llm/store.rs`**(已完成 2026-09-24)
  - provider / model 的 CRUD。Key 走 `credentials::crypto::protect` / `unprotect`,与 `map_providers` 同构。
  - 空 Key 的 `save` 表示**不改动原 Key**(6.2 表格),不是清空 —— 用户编辑名字时不会重新输一遍 Key。
  - 下发前一律过 `logging::mask_secret`。
  - 判据实测:`llm::store` 8 条全绿,覆盖掩码、空 Key 不覆盖、解不开密文当没填、`base_url` 末尾斜杠归一。
- [x] **M1-3 `llm/probe.rs`**(已完成 2026-09-24)
  - `GET {base_url}/models`,返回 `Vec<String>`,**不落库**。
  - 判据:`fetch_llm_models` 对真实中转商返回非空列表。**这一条要用户拿真实 Key 点一次才算过** —— 单测只能覆盖到「没填 Key 直接拒」(`LLM_KEY_REQUIRED`)与 URL 拼接,发不出真请求。`llm::probe` 3 条全绿。
  - **`/models` 要鉴权头,而地图侧一律把 Key 放查询串**,所以新增 `core/src/http.rs::get_json_with_auth` 而不是给 `get_json` 加参数 —— 后者会让地图侧几十个调用点各传一个 `None`。Anthropic 用 `x-api-key` + `anthropic-version: 2023-06-01`(缺版本头直接 400),OpenAI 兼容用 `Authorization: Bearer`。
  - 非 2xx 时把响应体前 200 字符带进错误:中转商的 401 往往在 body 里写明是「余额不足」还是「key 无效」,只报 `HTTP 401` 等于把线索丢了。空列表按错误处理(`LLM_NO_MODELS`,502),给个空多选框用户没办法知道是 `base_url` 少了 `/v1` 还是 Key 没开通。
- [x] **M1-4 五个 command**(已完成 2026-09-24)
  - `commands/llm.rs`:`list_llm_providers` / `save_llm_provider` / `delete_llm_provider` / `fetch_llm_models` / `save_llm_models`,签名照 6.2 的表。
  - `lib.rs` 的 `generate_handler!` 注册这五个。
  - 判据实测:`pnpm verify` 全绿。**注意 clippy 3 秒返回是缓存空转**,`touch` 过 `commands/llm.rs` / `llm/store.rs` / `core/src/http.rs` 再跑,确认四个 crate 都真的重新 `Checking` 了。
- [x] **M1-5 设置页**(已完成 2026-09-24)
  - 配置作为第三个 `<section>` 加进 `SettingsDialog.svelte`,**不是新建路由** —— 理由见 6.4.5 的修正段。
  - `src/lib/ipc/types.ts` 加 `LlmProviderDto` / `LlmProviderInput` / `LlmModelInput` / `LlmProtocol`,`client.ts` 的 `PlannerClient` 加五个方法 —— 漏了接口那一步,`planner.svelte.test.ts` 的 `fakeClient` 会编译失败,这是刻意的约束(9.4),**实测确实触发了**,补完五个方法才过。
  - 判据实测:明文 Key 那条**已用一次性 example 验证**。往真实文件存 canary `sk-PLAINTEXT-CANARY-…`,`wal_checkpoint TRUNCATE` 强制落盘(不 checkpoint 的话数据可能还在 WAL 里,搜不到会变成假阳性)后按字节搜索:36864 字节里**命中 0 次**,下发给前端的是 `sk-P********disk`,后端 `secret_of` 取回的明文与原文一致。三条同时成立才算过 —— 只搜不到明文也可能是根本没存进去。
  - **剩下两条判据要用户操作**:填真实中转商 base_url + Key 点「获取模型列表」出现真实模型名、重启程序 Key 仍在。模型无法代跑。
  - 顺带给 `Input.svelte` 加了 `ariaLabel` prop:紧凑行里放不下 `<Label>`,但控件必须有可访问名。直接写 `aria-label` 会被 svelte-check 拒(`$$ComponentProps` 里没这个键)。

### M2 非流式对话

先跳过 SSE 少一半复杂度:协议对不对、鉴权头对不对,在非流式下调试快得多。

- [x] **M2-1 会话两张表**(已完成 2026-09-24,实际是三张)
  - `schema.rs` 追加 `llm_conversations` / `llm_messages`,`VERSION` 6→7。**实际加了三张** —— `mcp_servers` 一起建了:同一次 `VERSION` 变更能带多张表,分两次加等于让老库多跑一轮迁移判断而没有任何好处。
  - 读写落在新文件 `llm/conversations.rs`,不是 `store.rs`。前者是会话记录,后者是入口密钥,混在一个文件里将来分不开。
  - **`read_conversation` 按 `(created_at, id)` 排序,不能只按 `created_at`。** 一次工具调用产生的 assistant 轮与 tool 轮几乎必然落在同一秒,只按时间戳排序时 SQLite 不保证稳定,界面回放会把因果顺序颠倒(先看到工具结果,再看到发起调用的那一轮)。
  - `new_id` 从 `store.rs` 提为 `pub(super)` 共用,不留第二份实现。
  - 判据实测:`cargo test -p autobee-shell` **111 passed / 0 failed**。含「写一轮对话再读出来,顺序与 role 不变」与级联删除两条。
- [x] **M2-2 一次性请求**(已完成 2026-09-24,落点与原文不同)
  - **原文说写 `llm/http.rs`,实际拆成三处**:HTTP 传输在 `core/src/http.rs`(`post_json` / `post_stream` / `LLM_CLIENT`),协议差异在 `llm/protocol.rs`,一次性对话在 `llm/chat.rs` 的 `one_shot`。理由:传输层要与地图侧共用连接池,协议层要被流式与非流式共用,写成一个 `llm/http.rs` 会让 M4 把它整个拆一遍。
  - **`LLM_CLIENT` 超时 600 秒,与地图的 10 秒分开。** 地图那个超时对大模型是必然超时,共用一个 Client 等于每次长回答都在 10 秒处断掉。
  - **三种协议全做了**,不是原文的「只实现 `openai-chat` 一种」:三者的差异只有端点路径、消息体形状、tool 声明字段三处,集中在 `protocol.rs` 里一次写完比留两个将来补要省。但 **tool calling 只有 `openai-chat` 支持**,另两个协议传了非空 `tools` 会报 `TOOLS_UNSUPPORTED` 而不是静默丢掉 —— 静默丢掉的后果是模型答得像在用工具但其实没调用。
  - **截断的统一折叠**:`openai-chat` 的 `finish_reason == "length"`、`openai-responses` 的 `status == "incomplete"`、`anthropic` 的 `stop_reason == "max_tokens"` 三者都折成 `TurnOutcome::Truncated`,不折成 `ToolCalls`。M3-2 依赖这一层区分来整批作废。
  - 判据实测:`cargo test -p autobee-shell` **111 passed / 0 failed**,clippy `-D warnings` **0 问题**(确认输出含 `Checking autobee-shell`,非缓存空转)。单测只覆盖请求体拼装与响应解析,不发真请求。
- [x] **M2-3 `llm_chat_start` 非流式版**(已完成 2026-09-24,**后被 M4-3 取代**)
  - 当时按原文实现:返回 `ChatStartResult { convId, reply, messages }`,不发 event。
  - **M4-3 已把它切成流式**,返回值换成 `ChatStartHandle { streamId, convId, userMessage }`,
    正文走 event。非流式那条路**没有留下兼容壳**(清一次到底),所以这一条的产物
    如今只以「`run_turn` 这个非流式入口仍在,被单测用来钉回合循环」的形式存在。
  - 判据「能与模型来回对话」**要真实 Key,我没跑过**。代码路径由 M4-3 的 115 条
    前端单测与 229 条 Rust 单测覆盖,但那不等于判据过了。
- [x] **M2-4 对话页最小实现**(已完成 2026-09-24)
  - `ChatPanel.svelte` + `ChatBubble.svelte`,`chat.svelte.ts` 持状态。`+page.svelte` 的
    `View` 加 `"chat"`,nav 加第四个按钮(顺序:安心换乘 → 生活圈 → AI 助手 → 日志)。
  - 判据「关程序重开,历史对话还在」**要真实窗口,我没跑过**。会话读写有单测,
    但「关掉进程再开」这一步单测碰不到。

### M3 tool calling(核心)

这一组是整个方案唯一无参照实现的部分,也是最容易做出「看着能用、其实在胡说」的一组。四个任务里有两个的判据是**与直接点规划的结果逐字对比**,不是「模型答得挺像」。

- [x] **M3-1 `llm/tools.rs` 的 schema 与裁剪结构**(已完成 2026-09-24)
  - 三个 tool 的 JSON Schema 照 5.2 写完:`plan_route` / `search_place` / `life_circle`。
  - enum 值直取 `TravelMode::ALL` / `SortBy::ALL` / `LifeCirclePackage::ALL` / `Crs::ALL`,不手抄。
  - **`life_circle` 加了原文没有的可选 `crs` 参数。** 这是实现时发现的真缺陷:core 的 `life_circle` 只吃 BD-09,而 `plan_route` / `search_place` 回给模型的坐标是 `settings.out_crs`(默认 GCJ-02)。模型把上一轮刚拿到的坐标原样填进 `life_circle` 是最自然的动作,结果是整体偏移几百米**而完全不报错** —— 又一个 5.3 那类静默偏差。按 `map.rs:329-340` 的现成做法双向换算,认不出的值报 `BAD_CRS`。
  - **不认识的 `mode` / `sort` / `package` 报错,不兜底成默认值。** 兜底的后果是模型分辨不出自己拿到的是「按它要求算的」还是「按默认参数算的」,也就无从重试。
  - 裁剪:丢掉 `Segment.points` 只留 `pointCount` 与首末点,`routes` 最多 3 条,`pois` 每类最多 5 个,`crs` 保留且无 `skip_serializing_if`。整体上限 8000 字符,切点先对齐 `char_boundary` 再回退到最近换行 —— 按字节切会把一个汉字劈成两半,模型收到的是乱码而不是截断。
  - 判据实测:`cargo test -p autobee-shell` **111 passed / 0 failed**。含 schema enum 与 `TravelMode::ALL` 逐字相等、裁剪后 `points` 不存在而 `crs` 存在两条。
- [x] **M3-2 `llm/agent.rs` 回合循环**(已完成 2026-09-24)
  - 轮次上限 5;达上限仍要求调工具则停下并告知用户。tool 报错回灌而不中断本轮。
  - **`Truncated` 整批作废,一个都不执行**,每个都作为错误回灌。
  - `run_loop` 不碰 IO:`ask` 与 `exec` 都是闭包入参,所以「截断的调用是否真的没执行」
    能被单测直接断言,不必靠真实 Key 跑一次去看。
  - 判据实测:三条单测全在且全过 —— `轮次上限触发时给出提示而不是继续循环`、
    `工具报错作为_tool_消息回灌而不中断本轮`、`截断时整批工具调用一个都不执行`
    (后者断言 `executed == []` 且两个调用都作为 `ToolEnd{ok:false}` 回灌)。
  - **另补一条原文没要求的**:`循环自己不发_delta_只发_done`。它钉住「正文只由 `ask` 发」
    这条不变式 —— 破坏后果是界面正文重复一遍。已实测:故意在 `run_loop` 里补一个
    `Delta` 后该测试 FAILED(`left: 1, right: 0`),改回即过。
- [x] **M3-3 `http.rs` 加 tools 字段**(已完成 2026-09-24,落点是 `protocol.rs`)
  - 请求体加 `tools`、解析 `tool_calls` 实际由 `protocol.rs` 的 `build_body` 承担
    (M2-2 已把协议差异集中到那里,再开一个 `http.rs` 分支会让同一件事有两处真相)。
  - 另两个协议传非空 `tools` 报 `TOOLS_UNSUPPORTED`,不静默丢掉。
  - 判据实测:`pnpm verify` 全绿(svelte-check 0 error / vitest 115 passed / Rust 229 passed)。
- [x] **M3-4 system prompt 坐标系硬约束**(已完成 2026-09-24)
  - `SYSTEM_PROMPT` 第 2 条写明不得把坐标念给用户,理由(不同服务商坐标系不同、
    百米级误导且无法被发现)也写在 prompt 里 —— 只下命令不给理由的约束模型更容易违反。
  - 判据:`system_prompt_含坐标系硬约束` 单测钉住那句话在。但**判据的实质部分
    (「模型给的时长与直接点规划的结果一致」)要真实 Key,我没跑过** —— 单测只能
    证明约束写进去了,证明不了模型遵守它。

### M4 流式

- [x] **M4-1 不提依赖:blocking + 手写 SSE**(2026-09-24 决定,推翻原文)
  - 原文要求把 `tokio` / `futures-util` / `uuid` 提为直接依赖。**三条都不做**,理由是实读现状:
    - `uuid`:`shell/src/credentials/store.rs` 的 `new_id()` 有明确注释说明为什么不引 uuid。提它等于反悔一个有理由的决定。
    - `futures-util`:当前整个 workspace **零依赖**。引它只为 `bytes_stream()` 的 async SSE。
    - `tokio`:当前没有任何 crate 直接依赖,只经 `tauri` 传递。
  - 真正的理由:`core/src/http.rs` 整个是 `reqwest::blocking`,模块文档第 3-5 行写明「引 async 会把 `async` 传染到每个 adapter 与每个单测,换不来吞吐」。走 async SSE 会把地图侧几十个调用点一起拖进去。
  - 改用:`reqwest::blocking::Response` 实现了 `std::io::Read`,在 `spawn_blocking` 里按块读;SSE 解析做成**纯状态机**(喂任意切分的字节块),跨 chunk 断行照样可测。已落地 `core/src/http.rs` 的 `post_stream`(返回未读 `Response`)与长超时客户端 `LLM_CLIENT`(600 秒 —— 地图的 10 秒对大模型是必然超时)。
  - 判据实测:`cargo tree` 里没有第二个 async runtime,而且是因为压根没加。`route-core` clippy `-D warnings` 0 问题(确认 `Checking route-core` 真实重编,非缓存)。
- [x] **M4-2 SSE 解析**(已完成 2026-09-24,不是 `bytes_stream`)
  - 原文写「改 `bytes_stream`」,那是 async 的 API。按 M4-1 的决定走 blocking:
    `shell/src/llm/sse.rs` 是一个**纯状态机**,喂任意切分的字节块,不碰 IO。
  - 判据实测:10 条单测,含跨 chunk 切断的行、`[DONE]` 终止、`error` 事件立刻报错。
- [x] **M4-3 事件通道**(已完成 2026-09-28,落点比原文大一圈)
  - 不用 mpsc:`run_turn_streaming` 的 `on_event` 已经是回调,后台线程里直接
    `app.emit(&format!("llm:chat:{stream_id}"), payload)`。多一层 channel 只是把
    同一个回调换个形状,还多一处「发送端先挂掉」的错误要处理。
  - **`llm_chat_start` 整个切成流式**:返回 `ChatStartHandle { streamId, convId, userMessage }`,
    正文与工具进展走 event。非流式那条路没留兼容壳。
  - **原文 §6.2 写「返回 `String`(streamId)」,这里多回两个字段。** 原形状有个漏洞:
    会话是后端新建的,若 `convId` 只随成功事件回去,**上游失败时前端永远拿不到它** ——
    而用户那句话已经落库了,下一句又会新建一个会话,库里于是攒下一串各含一条孤立
    用户消息的会话。所以建会话与落用户消息放在 spawn 之前做完,两者一并同步返回。
  - **`done` 事件带 `messages` 而不是原文的 `messageId`**:一轮可能新增 assistant 与
    多条 tool 行,只回一个 id 前端仍得再查一次库。
  - `toolEnd` 已带 §6.4.2 要求的 `plan` / `circle` 原始载荷，`types.ts` 已声明；M4-5 已完成。
  - 这条义务已由下方 M4-5 条目落地，不再是未完成事项。
  - **判据「首字 1 秒内出现」已实测**(2026-09-28,一次性 SSE 桩服务器,非真实 Key):
    服务端首块立刻发、之后每 300ms 一块,总时长 1562ms。链路测得 **首个 delta 0ms**。
    这个形状才证得了「边出字边推」:若实现读完整份响应才吐 delta,首字延迟会贴近
    1562ms 而不是 0ms —— 一次性回完的桩测不出这个差别,所以桩必须分块慢发。
  - 同一次实测顺带确认的三件事:请求体里 `stream: true` 与三个 tool 的 schema
    **真的发到了线上**、`accept: text/event-stream` 头在、跨 chunk 切断的
    `tool_calls.arguments` 拼回合法 JSON(分三片到达仍拼出完整参数)。
  - 事件序列实测:纯文本轮是 `Delta` ×3 → `Done`(**`Delta` 不重复**,与 `run_loop`
    的不变式一致);工具轮是 `ToolStart` → `ToolEnd(ok=false)` → `Delta` → `Done`,
    即**工具失败被回灌后本轮继续**而不是中断。
  - 判据另一半「工具调用期间显示具名动作」由单测钉住:`plan_route` → 「正在规划路线…」,
    `delta` 到达后清空;未登记的工具名退回通用文案而不泄露 `mcp_` 这类内部标识符。
  - **仍未实测的是真实模型**:桩服务器证明了线路与事件序列,证明不了某家中转商的
    具体分片形状。
- [x] **M4-4 `llm_chat_cancel`**(已完成 2026-09-28)
  - `llm_chat_start` 每起一条流就往 `StreamRegistry` 登记 `Arc<AtomicBool>`,
    流结束时**无论成败都摘掉**(`finish_stream`)—— 不摘会让这张表无界增长,而且
    同一个 id 的第二次取消会「成功」,对一条已经结束的流。
  - 取消只置标志位,不等流真的停下:那个线程可能正堵在 `read` 上。
  - 取消用 `CANCELLED` / **status 499**,不用 5xx。499 是「客户端主动关闭」惯例码,
    与真实失败分得开,界面据此不弹错误提示。
  - 发送键在流式期间换成停止键,**输入框不禁用**(§8.2 的 Loading 态)。
  - 判据实测:`停止后立刻能再发一句` 断言 `chatCancel` 收到 `s1` 且第二次 `send`
    真的发出;`停止后到达的事件一概不改界面` 断言残余 `delta` 与 `CANCELLED` 都不
    改列表 —— 后者是真实存在的竞态,后端那两条事件在取消后仍可能在路上。
  - **后端取消已在真实 socket 上实测**(2026-09-28,同一个 SSE 桩):对一条永不结束的
    流,置标志后 **617ms 返回 `CANCELLED` / status 499**(标志在 600ms 处置上,即约
    17ms 内反应),已到的部分正文保留。桩服务器那侧打出 `closed-by-client`,
    确认连接**真的断了**而不是后台线程还挂在 `read` 上继续收。
- [x] **M4-5 tool 结果回灌进 planner(§6.4.2 的义务,新增条目)**
  - 已完成。`tools.rs::dispatch` 一次执行同时产出两份结果:`summary` 是裁剪版,进
    messages 给模型;`ToolPayload::Plan/Circle` 是未裁剪原始结构,随 `toolEnd` 回前端。
  - `plan_route` 成功 → `planner.applyPlan`(含 `selectedRouteId`);`life_circle` 成功 →
    `planner.applyCircle`(含服务包勾选与 `circleResults`);`search_place` 不回灌。
  - 判据实测(2026-09-28,真实腾讯 Key,光谷广场 → 汉口火车站,地铁/时间):直接规划与
    `dispatch` 的 4 个方案折线点数均为 `[29, 129, 13]`,完全一致;喂回模型的 summary
    不含 `points` 数组。Rust 与前端分别有点数、载荷路由和失败/中间步骤回归测试。

### M5 MCP

- [x] **M5-1 `llm/mcp.rs` 客户端**(已完成 2026-09-24)
  - 手写 Streamable HTTP,四条消息:`initialize` / `notifications/initialized` / `tools/list` / `tools/call`。不引 `rmcp`。
  - **四条全部走 `post_stream` 而非 `post_json`**,原文没写这一点但它是必须的:`mcp-session-id` 在**响应头**里(`post_json` 只回 body,拿不到),而且服务端可以把 JSON-RPC 响应包在 SSE 的 `data:` 行里返回 —— 只按纯 JSON 解析会在这类服务端上直接失败。
  - `initialize` 的 result 校验含 `protocolVersion`,缺了报握手失败。
  - 单 server 失败只跳过该 server。
  - **`wrap_external` 的伪造闭合标记检测比 nullclaw 原版严。** 移植时实测出原版三处可绕过:标记中间插零宽字符(U+200B / U+FEFF 等)原版判不匹配即放行;全角同形字母同理;闭合 `>>>` 找不到时原版 `return null` 放行,但那段内容拼上后文仍能闭合真边界。三处都补强了,并限 64 字符扫描窗口避免 O(n²)。
  - 判据实测:`cargo test -p autobee-shell` **111 passed / 0 failed**,含「一个 server 连不上时其余 server 的 tool 仍在列表里」。clippy 顺带修掉 `mcp.rs:482` 的 `manual_ignore_case_cmp`。
- [x] **M5-2 `mcp_servers` 表 + 设置页**
  - `schema.rs` 在 VERSION 7 一起创建 `mcp_servers`; `commands/mcp.rs` 提供增删改查与 `tools/list` command;设置页提供启停、URL 校验、读取工具。
  - URL 校验只放过 HTTPS，或指向 `127.0.0.1` / `localhost` / `::1` 的 HTTP。Rust 单测已验证外网 `http://` 被拒、本机 HTTP 通过。
- [x] **M5-3 先打印真实 tool 清单**(2026-09-28 已完成)
  - 使用仓库已有百度 AK 请求 `https://mcp.map.baidu.com/mcp?ak=<AK>`，完成 `initialize`、
    `notifications/initialized`、`tools/list`。HTTP 状态分别为 `200`、`202`、`200`；
    `tools/list` 返回 14 个工具。验证输出未打印 AK。
  - 真实工具名表(百度返回原名，接入 AutoBee 后统一加 `mcp_baidu_` 前缀)：
  - `map_geocode`
  - `map_reverse_geocode`
  - `map_search_places`
  - `map_place_details`
  - `map_directions_matrix`
  - `map_directions`
  - `map_weather`
  - `map_ip_location`
  - `map_road_traffic`
  - `map_search_pro`
  - `map_district_search`
  - `map_uri`
  - `map_mark`
  - `map_geoconv`
- [x] **M5-4 合并进 tool 列表**
  - `McpRegistry::load_enabled` 每轮从启用的 MCP server 动态执行 `tools/list`,把远端 schema 以 `mcp_<server>_<tool>` 前缀合并进同一份 API `tools` 字段。
  - `agent.rs` 对未命中内置工具的 `mcp_` 调用转发 `tools/call`;返回值经随机 hex boundary 包裹、伪造闭合标记检测后才回灌上下文。单个 server 连接失败只跳过该 server。
  - 判据:回归测试已覆盖空 registry 不生成硬编码工具、前缀命名和伪造闭合标记检出；真实 MCP 端到端仍属于 M5-5。
- [ ] **M5-5 验收扩展性**
  - 判据一:连接百度 MCP,能回答 AutoBee 单独答不了的问题。
  - 判据二:设置页再填任意第三方 MCP server,不改 Rust、不重新编译,其 tool 出现在模型可用列表并能被调用。
  - 代码闭环已具备，但真实验收未完成：百度 MCP 的 `tools/list` 已通过并返回 14 个工具；真实调用 `map_weather` 返回百度侧 `Service activation timeout after 12 seconds`，提示需在百度控制台启用该 AK 的 MCP 服务。另需真实模型完成一次问答，当前不能勾选。

### 8.1 与大赛的关系

M1 至 M4 与现有参赛方案(`docs/AutoBee-百度地图大赛设计方案.md`)**不冲突但也不必需**。参赛主链路是确定性的路线规划与生活圈,大模型是增量演示。

建议:若距提交时间紧,先完成参赛主链路的 M1 至 M3(那份文档的),再回来做这份的 M1 至 M3。**大模型对话不应挤占百度地图核心性的展示占比** —— 那份文档 11 节已把「多 provider 反而削弱百度核心性」列为风险,大模型同理。

MCP 纳入范围(12 节问题 6)不改变这个优先级:M5 仍排在最后。若提交前只做得完 M1–M4,对话功能本身是完整可演示的,MCP 缺席不留半成品 —— 这也是把它放在最后一个里程碑而不是拆进前面几步的原因。

## 9. 测试策略

沿用现有约定:模块内 `mod tests`、中文测试函数名、`Connection::open_in_memory()` 建库(`schema.rs:135-144`)、测试模块顶部 `#[allow(clippy::unwrap_used, reason = "…")]`。

### 9.1 值得写的测试

| 测试 | 断言什么 | 为什么值得 |
| --- | --- | --- |
| `删掉服务商后模型行跟着消失` | 级联删除生效 | `foreign_keys` 没开时静默留孤儿行,查询时才炸 |
| `保存时留空密钥不会清掉原密钥` | 空 Key = 不改动 | 用户改名字时会连带清空 Key,这是真实会发生的误操作 |
| `密钥下发前端时被掩码` | DTO 里没有明文 | 泄密路径 |
| `枚举值与 schema 声明一致` | `TravelMode` / `LifeCirclePackage` 的 serde 名逐字等于 tool schema 的 enum | 写错时模型会反复重试同一个错值,现象极难定位(5.2) |
| `裁剪后的路线不含折线点` | `points` 字段不存在 | 漏裁一次就吃光上下文 |
| `裁剪后的路线保留 crs` | `crs` 字段存在 | 坐标系是静默错误唯一来源(5.3) |
| `工具轮次达到上限时停止` | 第 6 轮不再请求 | 死循环会烧配额 |
| `tool 报错被回灌而不是中断对话` | 错误进 messages,循环继续 | 否则一次上游波动就断掉整段对话 |

### 9.2 不写的测试

- 不测「保存后能读回来」这类纯 CRUD 往返 —— 那是在测 SQLite。
- 不测 HTTP 请求体的字段拼装逐字相等 —— 那是在测实现,改个字段顺序就红。
- 不对真实大模型发请求做断言 —— 不确定、要钱、离线就红。协议层用本地假 server(见 9.3)。

### 9.3 流式的验证方法

侦察报告提到 voynote 有一个值得借鉴的做法:起一个真 TCP server 发真 SSE,用**握手**而不是 `sleep` 判定「边到边出」。这样「攒完才发」的错误实现会卡死而不是偶尔失败。

这套思路不需要 Tauri 运行时,可以直接搬。M4 做完后补一个这样的测试。

### 9.4 前端

新命令必须加进 `PlannerClient` 接口(6.1)。`src/lib/state/planner.svelte.test.ts` 的 `fakeClient` 模式已经验证可用 —— 对话状态的测试注入一个假 client,断言流式事件累加后的消息内容,不碰真实 IPC。

## 10. 新增文件清单

```text
src-tauri/shell/src/llm/mod.rs
src-tauri/shell/src/llm/store.rs  provider/model/会话 CRUD + DPAPI(同步,M1 已完成)
src-tauri/shell/src/llm/probe.rs  拉模型列表(同步,M1 已完成)
src-tauri/src/llm/http.rs         协议实现(M2 非流式 → M4 流式)
src-tauri/src/llm/tools.rs        三个 tool 的 schema 与裁剪后结构
src-tauri/src/llm/agent.rs        回合循环
src-tauri/src/commands/llm.rs     IPC command
src-tauri/src/commands/mod.rs     (M0 拆分)只做 pub mod / pub use
src-tauri/src/commands/map.rs     (M0 拆分)现有 12 个地图 command 原样搬入
src-tauri/src/llm/mcp.rs          手写 MCP 客户端(四条消息 + 握手校验,见 7.3.1)

src/lib/state/chat.svelte.ts
src/lib/state/llm.svelte.ts
src/lib/components/app/ChatPanel.svelte   左栏对话流 + 输入区
src/lib/components/app/ChatBubble.svelte
src/lib/components/app/ToolTrace.svelte
src/lib/components/app/SettingsDialog.svelte  (改)第三个 section:大模型服务
src/routes/settings/mcp/+page.svelte      MCP server 配置
src/routes/+page.svelte          (改)View 加 "chat",nav 加第四个按钮
src/lib/ipc/types.ts             (改)加 LLM 相关类型
src/lib/ipc/client.ts            (改)PlannerClient 加方法
src-tauri/shell/src/credentials/schema.rs  (改)CREATE 加表 + VERSION 4→5 + 开 foreign_keys
src-tauri/shell/src/credentials/mod.rs     (改)加 LLM DTO 与 LlmProtocol
src-tauri/core/src/http.rs                 (改)加 get_json_with_auth(/models 要鉴权头)
src-tauri/src/lib.rs             (改)注册 command
```

**上面 `src-tauri/src/llm/` 那几行里,`store.rs` 与 `probe.rs` 实际落在
`src-tauri/shell/src/llm/`(M1 实测)。** 理由见 3.2 的修正段:HTTP 客户端只有 core 有。
`http.rs` / `tools.rs` / `agent.rs` / `mcp.rs` 到 M2 及以后再定位置,
但既然 M1 已经证明 shell 能拿到 HTTP,大概率也在 shell 而不是 app。

现有 `src-tauri/src/commands.rs` 是单文件(360 行 / 12 个 command)。新增 LLM 的十个 command 继续塞进去会让它膨胀到难以浏览,**因此这次拆**:`commands.rs` → `commands/mod.rs` + `commands/map.rs` + `commands/llm.rs`。拆分本身是 M0-1,纯搬移不改行为、单独一个提交,与功能代码分开。理由不是这次十个 command,是下一次还会有第十一个。

## 11. 风险与规避

### 11.1 破坏性改动:开启 `PRAGMA foreign_keys`

本方案唯一触及现有行为的改动。当前 `Store::open_at` 没有开外键约束,开了之后:

- **好处**:级联删除生效,不留孤儿行。
- **风险**:如果既有 `map_providers` 数据里存在违反外键的行,开启后相关操作会开始报错。经检查 `map_providers` / `prefs` 两表**没有任何外键声明**,所以开启对现有数据无影响。
- 仍需在 M1 验收时确认:用一份真实的旧 `settings.db` 打开程序,设置页能正常读写。

### 11.2 其余风险

| 风险 | 影响 | 规避 |
| --- | --- | --- |
| 模型把 BD-09 坐标当 GPS 讲给用户 | 静默错误,偏差百米级,不报错不崩溃 | 5.3 三条对策全部实施;M3 验收专门检查 |
| tool 返回值过大吃光上下文 | 对话第二轮就超限 | 5.4 裁剪规则 + `裁剪后的路线不含折线点` 测试 |
| enum 值与 serde 名不一致 | 模型反复重试同一个错值,现象难定位 | 加单测锁定;或从 Rust 侧生成 schema |
| 中转商协议细节偏离标准 | 请求被拒,错误信息含混 | 错误里带上游原始响应摘要(截断),写进日志 |
| 模型编造路线而不调工具 | 用户分辨不出真假 | system prompt 明确「路线与地点信息必须来自工具」;`tools_ok = 0` 的模型在界面上标注「不支持工具」 |
| MCP 客户端手写出协议错误 | 连不上或静默丢工具 | 7.3.1 的五条消息逐条对照 nullclaw `mcp.zig`;首次接入先打印 `tools/list` 原文(7.4) |
| 两套 HTTP 写法共存 | 新人困惑 | `llm/http.rs` 文件头写明为什么这里是 async 而 core 是 blocking |
| 对话挤占参赛演示占比 | 削弱百度地图核心性 | 8.1:参赛主链路优先,对话作为增量 |
| Markdown 渲染引入 XSS | 模型输出可被提示词注入操纵 | 6.5:首期不用 `{@html}` |

## 12. 评审问题清单

以下问题需要拍板后才进开发。**未定项不影响 M1 启动**,但 M3 之前必须有答案。

问题 3 / 5 / 6 已拍板(2026-09-23),「我的建议」列改记已定结论。

| # | 问题 | 我的建议 | 影响范围 |
| --- | --- | --- | --- |
| 1 | 是否合并 endpoints 层(3.3)? | 合并。AutoBee 没有多线路负载均衡需求 | 表结构,后期加表可逆 |
| 2 | 首期只做 `openai-chat` 协议(5.6)? | 是。中转商几乎都兼容 | M2/M3 工作量减三分之二 |
| 3 | 对话页是一级入口还是设置页里的子功能? | **已定:都不是。** 对话是主页工作台的第四个视图,复用左栏 + 地图(6.4.1) | 前端信息架构 |
| 4 | 是否需要会话历史持久化? | 需要。否则关程序就丢,用户会重复问同一个问题 | 两张表,M2 范围 |
| 5 | `commands.rs` 是否借这次拆成目录(第 10 节)? | **已定:拆。** `commands/{mod,map,llm}.rs`,纯搬移单独提交 | 一次纯搬移,可单独提交 |
| 6 | MCP 是否纳入参赛版本? | **已定:纳入。** 它是后续工具扩展的底座,不是一次性功能(第 7 节) | M5 进范围 |
| 7 | 轮次上限设 5 是否合适? | 合适。三个 tool 的任务不需要更多轮 | 一个常量,易调 |
| 8 | 是否需要「模型不支持工具」的降级路径? | 需要。`tools_ok = 0` 时退化为纯对话并在界面标注 | 一个字段 + 一处分支 |

### 12.1 我认为不该做的事

评审时若有人提出以下方向,建议拒绝,理由已在正文:

- **照搬 voynote 的 skills**(2.2):纯提示词注入解决不了「模型调不动 core」的问题,会给人「已经接好了」的错觉。注意这是对 voynote 那个实现的否决,不是对 skill 机制的否决 —— openfang 证明了 skill 可以真是工具(2.2.1)。AutoBee 首期不做的理由只有一个:三个 tool 手写声明就够,skill 运行时(子进程 + 清单 + 签名)是为「第三方扩展」准备的,AutoBee 没有这个需求。
- **引 rmcp SDK**(7.3):实测只新增 28 个 crate,不算失控,但买到的仅是两条消息的实现;而按 server 降级这个真正要紧的行为,SDK 不提供。
- **用 MCP 替代自己的算路与生活圈**(7.2):换来非确定性,丢掉「每个 POI 经真实步行验证」这个可复现性。
- **把 `route-core` 改成 async**(1.2):违背它存在的理由,拖慢单测。
- **让模型接管确定性链路**(1.2):用户点「规划路线」不该经过模型。

## 13. 评审结论

> 待填。评审通过后在此记录:结论、被否决的项、被修改的决策,然后进 M1。

