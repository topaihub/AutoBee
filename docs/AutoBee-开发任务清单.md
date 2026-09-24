# AutoBee 开发任务清单

给接手开发的人或模型的**唯一入口**。52 条任务分散在三份详细设计的实施计划节里,
不读完三份就不知道该先动哪个 —— 这份文档只解决「顺序」和「在哪查判据」两件事。

## 怎么用这份文档

**判据不在这里。** 每条任务的完成判据写在它自己那份设计文档里,这份只给
任务号、一句话说明、和指回原文的位置。两处都写判据的话早晚会分叉,
到时候你不知道该信哪份。

流程:

1. 在下面的执行顺序里找到第一条未勾选的任务。
2. 打开它对应的设计文档小节,**读完整条**(判据、陷阱、涉及的文件行号都在那)。
3. 做完后按判据自查,通过了再回来勾这份和原文那份。
4. **判据没过不许开下一条。** 顺序不是建议,依赖关系在下面「为什么是这个顺序」里。

## 三份设计文档

| 文档 | 实施计划节 | 任务数 | 前缀 |
| --- | --- | --- | --- |
| `AutoBee-CLI-设计方案.md` | §9 | 15 | C1 / C2 / C3 / C4 |
| `AutoBee-大模型能力接入设计方案.md` | §8 | 23 | M0 / M1 / M2 / M3 / M4 / M5 |
| `AutoBee-百度地图大赛设计方案.md` | §8 | 14 | M1 / M2 / M3 |

`AutoBee-概要设计.md` 不含任务,它说明三份怎么分工、以及**已实现**的范围(§6)。

**注意前缀撞号**:大模型方案和大赛方案都有 `M1`/`M2`/`M3`。下面一律写成
`大模型 M1-1` / `大赛 M1-1` 区分,只写 `M1-1` 的地方是有歧义的,别照抄。

## 为什么是这个顺序

只有三处是**真依赖**(不按顺序会返工),其余都是优先级取舍:

1. **C1 → C2/C3/C4**:C1 把凭据、日志、路径拆成 `autobee-shell` crate。
   CLI 要读同一份密钥,不拆的话它得依赖 `tauri` 才能读到 —— 这是 CLI 方案
   §3.1 那两个硬约束的结论。
2. **M0 → 大模型 M1 及以后**:M0 把 `commands.rs`(360 行、12 个 command)
   拆成目录。后面每一组都要往里加 command,先拆比边加边拆干净。
3. **大赛 M1 → 大赛 M2**:负担评分要跑在百度真实响应上,provider 没接通
   就只能拿构造数据测,而构造数据证明不了 adapter 字段填对了
   (大赛 M2-1 的判据原话)。

**C1 与 M0 排在所有功能开发之前**,因为两者都是纯搬迁不加行为。等功能堆上去
再拆,要改的调用点会多一倍。

**大赛 M1–M3 优先于大模型 M1–M5。** 理由在大模型方案 §8.1:参赛主链路是
确定性的路线规划与生活圈,大模型是增量演示,不该挤占百度核心性的展示占比。
距提交时间紧就先做大赛那三组。

**大模型 M5(MCP)固定排最后。** 只做完 M1–M4 的话对话功能本身是完整可演示的,
MCP 缺席不留半成品 —— 这是把它放最后一个里程碑而不是拆进前面的原因。

## 第一阶段:重构地基(纯搬迁,不加行为)

### C1 拆出 `autobee-shell` — CLI 方案 §9,行 327-348

- [x] **C1-1 建 crate 骨架** — `src-tauri/shell/Cargo.toml`,依赖照根包挑 7 项
- [x] **C1-2 搬模块** — `paths.rs`/`error.rs`/`logging.rs`/`logs.rs`/`credentials/` 五项移进 shell
- [x] **C1-3 接回根包** — `members` 加 `shell`,`commands.rs` 的 `crate::` 改 `autobee_shell::`
- [x] **C1-4 守住可见性** — `protect`/`unprotect` 保持 `pub(crate)`

**已完成(2026-09-24)。** 实际结果:shell 36 个测试全绿(搬迁前根包 35 条
一一对应,外加下面那条新的),core 50 个、前端 56 个不受影响。搬完后根包不再
直接依赖 `rusqlite`/`tracing-subscriber`/`serde`/`serde_json`/`thiserror`/`windows`,
已从根 `Cargo.toml` 删除。

**做这一组时踩到的、文档里没写的一点**:`shell/` 在根包目录内,不进 `members`
时 cargo 直接报「包不在 workspace 里」。所以 C1-1 的判据
(`cargo build -p autobee-shell` 通过)要求 `members` 那一行提前到 C1-1 改,
不能等到 C1-3。

**顺手补掉的一条判据**:C1-3 原本要求手动点设置页填 Key 读回掩码 —— 那是
10.1 那个 `#[cfg(windows)]` DPAPI 陷阱唯一的检出手段。已改成自动化测试
`store.rs::真实文件路径上密钥能加密落盘再读回`,走 `open_at`(不是测试用的
`memory()`,两条是独立路径)。这条测试经过反向验证:把 `protect` 临时改成
返回 `None` 后它确实 FAILED。所以那个陷阱不再依赖人工。

### M0 拆 `commands.rs` — 大模型方案 §8,行 632-640

- [ ] **M0-1 拆成目录** — `commands.rs`(360 行、12 个 command)拆成
  `commands/mod.rs` + `commands/map.rs`;`mod.rs` 只做 `pub mod` + `pub use`,
  `lib.rs` 的 `generate_handler!` 不用改

纯搬移,单独一个提交,不掺功能代码。

## 第二阶段:参赛主链路(大赛方案 §8,行 380-454)

优先于大模型那五组。理由见上面「为什么是这个顺序」。

### 大赛 M1 百度主链路可用 — 行 386-412

- [ ] **大赛 M1-1 申请两个 AK** — Web 服务 AK 与 JS API GL 的 AK,两个不通用
- [ ] **大赛 M1-2 实现 `BaiduProvider`** — 新建 `core/src/providers/baidu.rs`(现在没有这个文件)
- [ ] **大赛 M1-3 登记进工厂与注册表** — `factory.rs:18-19` 的 `is_implemented` 加百度;
  `registry.rs` 里百度 priority 改 **0**(不是 1 —— `registry.rs:199` 是稳定排序,
  写 1 会与腾讯并列而腾讯在前)
- [ ] **大赛 M1-4 主地图换百度 JS API GL** — 底图、标注、路线覆盖物全换
- [ ] **大赛 M1-5 验收整条链路** — POI 联想、选点、三种规划、覆盖物**五项全部来自百度**

M1-5 是 §5.2「不把百度只当底图」的门槛,不是可选的收尾。

### 大赛 M2 负担评分与解释 — 行 413-439

- [ ] **大赛 M2-1 确认指标在百度数据上成立** — 用**真实百度响应**归一化后的样本跑排序;
  现有 `accessibility.test.ts` 用的是构造数据,证明不了 adapter 字段填对了
- [ ] **大赛 M2-2 补齐 §6.2 的五种排序** — 切画像排序与文案可见变化,且同画像连点两次结果一致
- [ ] **大赛 M2-3 决策标签(6 个,现在一个都没有)** — `RouteCard.svelte:49/52`
  现在只有地铁/公交两个模式徽标
- [ ] **大赛 M2-4 阈值挪进设置** — `RouteCard.svelte:42` 现在写死 500 米,违反 §4.2 自己的规定
- [ ] **大赛 M2-5 守住诚信边界** — `grep -rn "无障碍" src/lib/components src/routes`
  每处命中都不能是承诺性说法

### 大赛 M3 展示完整度 — 行 440-454

- [ ] **大赛 M3-1 固化两个演示场景** — 同场景连跑三次结果一致
- [ ] **大赛 M3-2 四个非正常态** — 各手动触发一次并截图;断网、清空 AK 是最快的触发方式
- [ ] **大赛 M3-3 录视频** — 3 分钟 + 8 分钟两个,脚本在 §9
- [ ] **大赛 M3-4 提交材料** — §10 的复选框全部勾上

## 第三阶段:CLI 入口(CLI 方案 §9,行 347-399)

C1 已完成,这三组可以随时开工。与大赛那几组无依赖关系,谁先做都行。

### C2 参数解析 + `route` — 行 347-373

- [ ] **C2-1 建 `autobee-cli` crate** — 第三个 crate,不是根包的第二个 bin(理由见 §3.2)
- [ ] **C2-2 写 `args.rs` 的分词器** — 手写,不引 clap(理由见 §6.2,不是「少几个依赖」)
- [ ] **C2-3 枚举值从 `ALL` 打印** — `TravelMode::ALL`(`models.rs:160`)、
  `SortBy::ALL`(`models.rs:185`);不许手写第二份清单
- [ ] **C2-4 exit code 映射** — 表在 §5.3
- [ ] **C2-5 接上 `route`** — 调 `planner::plan_route`

### C3 `suggest` + `circle` — 行 374-384

- [ ] **C3-1 `suggest`** — 调 `planner::suggest_places`(`planner.rs:276`,返回裸 `Vec<Place>`)
- [ ] **C3-2 `circle`** — 调 `life_circle::life_circle`(`life_circle.rs:154`);
  注意 `LifeCirclePackage` 只有 `parse`(`models.rs:344`)**没有 `ALL`**,
  C2-3 那条要用 `ALL` 的话得先加

### C4 进构建管线 — 行 385-399

- [ ] **C4-1 编译进 release** — `scripts/build-app.bat` 的 `[4/6]`,`tauri build` 调用在行 124
- [ ] **C4-2 收进 `dist\`** — 同一脚本 `[5/6]`,注释在行 137、echo 在行 139
- [ ] **C4-3 确认质量门不用改** — `package.json` 的 `test:rust`/`clippy`/`fmt:rust`
  已经是 `--workspace`/`--all`,新 crate 自动进;跑一次确认
- [ ] **C4-4 冒烟** — 打出来的 exe 跑三个子命令各一次

## 第四阶段:大模型接入(大模型方案 §8,行 628-740)

M0 是它的前置,已在第一阶段列出。

### 大模型 M1 配置层 — 行 641-664

- [ ] **大模型 M1-1 加两张表** — 同时开 `PRAGMA foreign_keys`。
  **加在 `schema::prepare` 里**(`schema.rs:57`),不是 `store.rs::open_at`(`:40`)——
  后者与测试用的 `memory()`(`:62`)是两条独立路径、各自调 `prepare`,
  只改 `open_at` 会让级联测试失败而生产正确
- [ ] **大模型 M1-2 `llm/store.rs`** — 密钥走现有 DPAPI 通道,不新建
- [ ] **大模型 M1-3 `llm/probe.rs`** — 拉模型列表
- [ ] **大模型 M1-4 五个 command**
- [ ] **大模型 M1-5 设置页** — 判据含 `settings.db` 里搜不到明文 Key

### 大模型 M2 非流式对话 — 行 665-681

- [ ] **大模型 M2-1 会话两张表**
- [ ] **大模型 M2-2 `llm/http.rs` 一次性请求**
- [ ] **大模型 M2-3 `llm_chat_start` 非流式版**
- [ ] **大模型 M2-4 对话页最小实现**

### 大模型 M3 tool calling(核心)— 行 682-705

- [ ] **大模型 M3-1 `llm/tools.rs` 的 schema 与裁剪结构** — 判据第二条
  (裁剪后 `points` 不存在、`crs` 存在)是 §5.3 静默偏差的唯一自动化防线
- [ ] **大模型 M3-2 `llm/agent.rs` 回合循环** — 判据第三条
  (`stopReason == "length"` 时执行过的 tool 数为 0)必须有,是唯一能挡住那个无声错误的
- [ ] **大模型 M3-3 `http.rs` 加 tools 字段**
- [ ] **大模型 M3-4 system prompt 坐标系硬约束** — 两条判据都要看,
  只看第二条会漏掉「模型编了个好看的时长」

### 大模型 M4 流式 — 行 706-720

- [ ] **大模型 M4-1 提依赖** — `cargo tree` 里不许出现第二个 async runtime
- [ ] **大模型 M4-2 SSE 解析** — 单测必须含**跨 chunk 切断的行**,
  那是手写 SSE 解析唯一会错的地方
- [ ] **大模型 M4-3 事件通道**
- [ ] **大模型 M4-4 `llm_chat_cancel`**

### 大模型 M5 MCP — 行 721-740

固定排最后,理由见上面。

- [ ] **大模型 M5-1 `llm/mcp.rs` 客户端** — 手写,不引 rmcp(理由见 §7.3.2,有实测数据)
- [ ] **大模型 M5-2 `mcp_servers` 表 + 设置页** — 判据:外网 `http://` 被拒、
  `http://127.0.0.1:x` 通过
- [ ] **大模型 M5-3 先打印真实 tool 清单** — 判据是往 §7.3 下方补一张真实 tool 名表;
  那张表现在是空的,**这一条没做完就是没做**
- [ ] **大模型 M5-4 合并进 tool 列表** — MCP 返回值是第三方内容,
  那条注入检出单测就是防线本身
- [ ] **大模型 M5-5 验收扩展性**

## 开发环境:两条不知道就会卡住的

### cargo 必须走 wrapper

```
cmd.exe /d /c call "scripts\cargo-msvc.bat" test --workspace
cmd.exe /d /c call "scripts\cargo-msvc.bat" clippy --workspace --all-targets -- -D warnings
cmd.exe /d /c call "scripts\cargo-msvc.bat" fmt --all -- --check
```

裸 `cargo` 在这台机器上链接会失败:Git 自带的 coreutils 里有个 `link.exe`,
它在 PATH 里遮蔽了 MSVC 的链接器。wrapper 就是把 MSVC 的路径放到前面。

### pnpm 用 vendored standalone

```
node tools/pnpm-standalone/package/bin/pnpm.mjs run verify
```

裸 `pnpm` 首次调用会挂住:`package.json` 里 `packageManager: pnpm@12.5.1`
会触发下载 + re-exec。一律用 `node` 驱动仓库内的 standalone。

`verify` 覆盖前端 vitest(56)、`cargo test --workspace`(shell 35 + core 50)、
clippy `-D warnings`、`fmt --check`。提交前跑它。

### 其他约定

- `.bat` 文件保持 CRLF + 纯 ASCII。
- 密钥、日志、缓存都在 `%LOCALAPPDATA%\AutoBee`,不在仓库里。
- 测试里的 `unwrap` 要在 `mod tests` 上方加
  `#[allow(clippy::unwrap_used, reason = "测试里断言失败即测试失败")]`,
  照 `life_circle.rs:171` 既有写法。根包与 shell 的 lint 配置里
  `unwrap_used = "warn"`,而质量门是 `-D warnings`。
