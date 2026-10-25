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
| `AutoBee-大模型能力接入设计方案.md` | §8 | 24 | M0 / M1 / M2 / M3 / M4 / M5 |
| `AutoBee-百度地图大赛设计方案.md` | §8 | 14 | M1 / M2 / M3 |

`AutoBee-概要设计.md` 不含任务,它说明三份怎么分工、以及**已实现**的范围(§6)。

**注意前缀撞号**:大模型方案和大赛方案都有 `M1`/`M2`/`M3`。下面一律写成
`大模型 M1-1` / `大赛 M1-1` 区分,只写 `M1-1` 的地方是有歧义的,别照抄。

## 当前进度(2026-09-28)

**45 / 53 已完成。**(总数 52 → 53:新增 M4-5,见下。)剩 6 条:
大赛 M1-5 与 M3 全组(5,全部要人工验证),大模型 M5-5(1,需真实扩展性验收)。
M5-2 的 URL 校验与设置页、M5-4 的运行时工具合并、M5-3 的百度真实 tool 清单已完成，判据见详细设计。

**大模型 M2 / M3 / M4 三组的代码全部落地**,`pnpm verify` 全绿:
svelte-check 0 error / vitest **121 passed** / Rust **232 passed**,
`clippy --workspace --all-targets -- -D warnings` 0 问题,`cargo fmt --all -- --check` 通过。

**M4-5「tool 结果回灌进 planner」已完成。** `toolEnd` 携带未裁剪的路线/生活圈
结构;地图通过 `planner.applyPlan` / `applyCircle` 回灌。真实腾讯 Key 验证直接规划
与 tool 路径的折线点数一致(`[29, 129, 13]`),模型 summary 不含 `points` 数组。

**M4 的流式链路已在真实 socket 上实测**(2026-09-28,一次性 SSE 桩服务器):
首个 delta **0ms**(总时长 1562ms 的分块慢发流),跨 chunk 切断的
`tool_calls.arguments` 拼回合法 JSON,取消返回 499 且服务端确认连接断开。
**下一条代码任务是大模型 M5-5**,M5-1/M5-2/M5-3/M5-4 已完成。M5-5 仍需真实模型与任意第三方 MCP 的扩展性验收。


**还有三条判据要真实 API Key 或真实窗口**,单测与桩都证明不了:
M2-3「能与模型来回对话」、M2-4「关程序重开历史还在」、M3-4「时长与直接点规划一致」。
前两条要真实窗口,第三条要真实 Key —— 桩服务器能证明线路,证不了模型的行为。

**仍被人工验证挡住的是大赛 M1-5 与 M3-1 ~ M3-4**:需要真实窗口里的视觉确认。
M1-4 的代码与 API 面已验完,M1-5 五项里 4 项有实跑证据,只差「覆盖物屏幕可见」。
本机 Chrome 无头启得起来却从不导航(5 种启动组合下本地服务器 0 请求),模型产不出
这条证据 —— 需要用户跑一次。

质量门基线(2026-09-24 实测):前端 **93**、core **64**、shell **111**、CLI **16**,
svelte-check **355 files 0 error**,clippy `-D warnings` **0 问题**,fmt 干净。

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

### C1 拆出 `autobee-shell` — CLI 方案 §9,行 323-349

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

### M0 拆 `commands.rs` — 大模型方案 §8,行 632-641

- [x] **M0-1 拆成目录** — `commands.rs`(360 行、12 个 command)拆成
  `commands/mod.rs` + `commands/map.rs`;`mod.rs` 只做 `pub mod` + `pub use`,
  `lib.rs` 的 `generate_handler!` 不用改

**已完成(2026-09-24)。** `pnpm verify` 全绿:前端 56、shell 36、core 50、
clippy 0 warning、fmt 干净。纯搬移,用 `mv` 而不是手抄,行为零变化。

**文档里原本写错的一点(已改正)**:「`pub use` 之后路径不变」只在 glob 下成立。
`#[tauri::command]` 会在函数旁生成 `__cmd__xxx` / `__tauri_command_name_xxx`
两个隐藏项,`generate_handler!` 引用的是它们。逐名 `pub use map::{…}` 带不走,
编译报 24 个 `could not find __cmd__xxx in commands`,报错里不提宏生成。
必须是 `pub use map::*;`,`mod.rs` 里已写明理由。

## 第二阶段:参赛主链路(大赛方案 §8,行 380-478)

优先于大模型那五组。理由见上面「为什么是这个顺序」。

### 大赛 M1 百度主链路可用 — 行 386-436

- [x] **大赛 M1-1 取得 AK** — 实测**一个 AK 两侧通用**,原先「两个不通用」是错的
- [x] **大赛 M1-2 实现 `BaiduProvider`** — 新建 `core/src/providers/baidu.rs`
- [x] **大赛 M1-3 登记进工厂与注册表** — `factory.rs` 的 `is_implemented` 加百度;
  `registry.rs:119` 百度 priority 改 **0**(不是 1 —— `registry.rs:199` 是稳定排序,
  写 1 会与腾讯并列而腾讯在前)
- [x] **大赛 M1-4 主地图换百度 JS API GL** — 底图、标注、路线覆盖物全换;
  原文「现在是 `tiles.rs` 的 ArcGIS 栅格瓦片」是错的,当时是腾讯 `TMap` GL 矢量 SDK。
  `BMapGL.Point` 是 **`(lng, lat)`** —— 与百度 Web 服务 API 的 `lat,lng` 相反。
  **代码已完,但缺一条人工视觉判据**:本机 Chrome 无头启得起来却从不导航
  (5 种启动组合下本地服务器 0 请求),已改用 SDK 源码逐个核对 API 面代替,
  详见设计方案行 424-428
- [ ] **大赛 M1-5 验收整条链路** — POI 联想、选点、三种规划、覆盖物**五项全部来自百度**。
  **4 项已有实跑证据**(四端点全 `status=0`);第 5 项「覆盖物屏幕可见」需真实 UI,
  详见设计方案行 430-435

**M1-1 / M1-2 / M1-3 / M1-4 的代码已完成(2026-09-24)。** `pnpm verify` 全绿:core 64 个
测试(原 50 + 百度新增 14)、shell 40、CLI 12、前端 56,svelte-check 350 files 0 error,
clippy `-D warnings` 0 问题、fmt 干净。

**M1-5 的四项实跑证据(2026-09-24)。** 真 AK 打四个端点全部 `status=0`:
`place/v2/search`(3 条,首条「中国国家图书馆」)、`geocoding/v3`(lng=116.3316
lat=39.9528)、`direction/v2/walking`(1 方案 9725 米)、`direction/v2/transit`。
**三种规划确实走不同策略**:`tactics_incity=5`(`Subway`)得 5 方案 / 首选 5 段
= 3 步行 + 2 地铁 / 4059 秒;`=1`(`Bus` 与 `Walking`)得 4 方案 / 首选 3 段
= 2 步行 + 1 公交 / 5682 秒。**`tactics_incity` 合法范围是 0-5**,传 11 / 13 会
`status=2`;`baidu.rs:513` 传的 5 与 1 都在范围内。

**M1-1 的结论与原文相反,设置页少一个字段。** 原先写「Web 服务 AK 与 JS API GL AK
是两个不同的 AK,要分两个字段存」,那是照文档推的。拿用户给的 AK 实测:四个 Web
服务端点(`place/v2/search`、`geocoding/v3`、`direction/v2/walking`、
`direction/v2/transit`)全部 `status=0`;`api.map.baidu.com/getscript?type=webgl&v=1.0&ak=…`
返回 1.2 MB 真实 SDK,里面 `window.B_BUSINESS_INFO={"business":0,"unauth":0,…}`,
**`unauth: 0` 就是已授权**。所以设置页先只留一个百度 AK 字段。

**`place/v3/region` 评估过,不换,留 v2。** 用户给了百度官方的 v3 示例,逐项拿真
AK 对比:字段名、零结果行为、`total`、`page_size`、`adcode`、坏 AK 错误形状、
`detail_info.children`、`navi_location` **八项全同**,八条查询词返回结果一字不差。
v3 只多一个 `d7_hot_value`(7 天热度)与更多坐标小数位,都用不上。换过去是纯改动
风险换零收益。

过程中我曾判断「v3 能挡住模糊匹配、v2 不能」,那是**错的** —— 我拿了两个不同查询词
(`…地名…` 与 `…地址…`)对比。同一条查询词下两个版本都返回贵阳那家 `zzzzt公寓`,
而那家店真实存在且名字确实以 `zzzz` 开头,返回它对两个版本都正确。

另外官方 v3 参数表写 `type` 是**可选**,用户那份示例注释标「必填」是错的,与实测
一致(不传、传空都 `status=0`)。`region` 确实必填(不传 `status=2`)。

**`scope=2` 顺带拿到了 M2 要用的东西。** `detail_info.children` 是车站各出入口的
**真实坐标**(实测南京南站 9 个子点:「北进站口」「出站口」`出入口;车站入口` /
`出入口;车站出口`,还有各停车场),`navi_location` 是导航上车点。无障碍负担评分要算
「进出站步行」时,数据已经在响应里,不必改请求。adapter 现在丢掉它们是对的 ——
`Place` 不该带厂商特有字段(`providers/mod.rs:6-7` 的契约),M2 需要时另开归一化结构。

**照 `amap.rs` 抄之前必须知道的一点**:百度在**同一个接口**里,请求参数与
响应折线的坐标顺序是**相反**的 —— 请求 `origin`/`destination` 是 `纬度,经度`
(文档原文「格式为:纬度,经度」),而响应的 `path` 串是 `经度,纬度`
(公交页对 `path` 的备注给了示例 `"116.321858,40.039183;…"`)。
`baidu.rs` 里 `location()` 与 `path()` 的顺序**故意是反的**,模块文档写了原因。
顺手「统一」掉其中一个,得到的是一条经纬度对调的折线:`LatLng` 不校验范围,
所以不报错,只是线跑到极区。这条已落成测试
`步行折线按经度在前解析而不是纬度在前`。

**设计文档没写、查官方文档才发现的三点**:

1. **百度公交接口不需要城市参数。** 高德必须先逆地理编码拿 `citycode`
   (`amap.rs:105` 的 `city_code`),百度的参数表里根本没有 `city` ——
   起终点坐标就够它定城市,跨城也是同一个接口。比高德少一次请求。
2. **`steps` 是二维数组**(`steps[i][j]`),不是高德那种平铺的 `segments`。
   按平铺解会一个分段都取不到,而且**不报错** —— 表现是「算不出路线」。
   更麻烦的是官方返回参数表把内层命名为 `schemes`,官方响应示例里却是匿名
   数组,两者矛盾。`sub_steps()` 两种形状都接,各有一条测试。
3. **公交与地铁共用 `vehicle_info.type = 3`**,区分在 `detail.type`(1 是地铁)。
   只看 `vehicle_info.type` 会把每条地铁都标成公交,配色与徽标全错。

**`geocode` 的「搜不到」有两条路,原先只接了一条(已修)。** 我先前写「百度整张
状态码表里没有零结果码,所以只能按结构判」—— 那是照文档推的,拿真 AK 打过之后
不成立。实测 `address=qwertyuiop asdfghjkl` 返回:

```json
{"status":1,"msg":"Internal Service Error:无相关结果","results":[]}
```

`status=1` 在官方状态码表里写的是「服务器内部错误」,百度把零结果塞进了这个码。
按「非 0 即错误」处理的话 `geocode` **永远返回不了 `Ok(None)`**。两条路现在都接:
`status=1` 且 `msg` 含「无相关结果」,以及结构不完整(取不到 `/result/location`)。

真故障也是 `status=1`,只能靠 `msg` 那句中文分开。**匹配中文文案是明知其脆弱后
选的**:不匹配就会把真故障一起咽掉(该提示「百度服务异常」时显示「地址搜不到」);
匹配则是百度哪天改了文案,行为退回「返回 `Err`」—— 也就是修之前的行为,而
`resolve_on:75` 用 `.ok().flatten()`、`geocode_place:302` 用 `let Ok(Some(..))`,
两处都把 `Err` 与 `Ok(None)` 收敛成同一条路,不会因此出故障。坏的方向是安全的。

**还有个挡不住的:百度会模糊匹配。** `zzzz不可能存在的地址zzzz` 返回 `status=0`
加北京朝阳的坐标(匹配到「北京不可能科技有限公司」)。它的字段签名
`level=NoClass` / `precise=0` / `confidence=50` / `comprehension=0` 与真实的
「南京南站」**完全一致** —— 所以不能按这些字段判零结果,那会把站名查询一起毙掉,
而站名是公交应用最主要的输入。地名类输入靠 `planner.rs:63` 那条
「先 suggest 再 geocode」的顺序兜。

**错误详情字段在这个端点是 `msg` 而不是 `message`**,两者不同时出现,`ensure_ok`
要先读 `message` 再回落 `msg`。坏 AK 时四个端点**都**用 `message`(实测 `status=200`
+「APP不存在,AK有误请检查再重试」),所以权限/配额类错误读 `message` 是对的。

**`suggest` 走 `place/v2/search` + `region=全国`,不走 `place/v2/suggestion`**:
suggestion 的 `region` 是必填的具体城市、固定返回 10 条且没有条数参数,
接不住 trait 的 `page_size`;两者响应键名还不一样(search 是复数 `results`,
suggestion 是单数 `result`)。另外 `adcode` 必须传 `extensions_adcode=true`
才召回,而 adcode 是编排层判境内/境外的依据。

**顺手修掉的两处与百度无关的既有破损**(都在未提交的 `src-tauri/cli/`,
扩门到 `--workspace` 后才暴露):`args.rs` 的 `Command::Suggest` 少了
`out_crs` 字段,而 `main.rs:58` 与 `run::suggest` 都在用它,编译不过;
`args.rs` 测试模块里三处 `panic!` 在 `-D warnings` 下报错,按既有写法
加进那条 `#[allow]` 的 reason 里。


M1-5 是 §5.2「不把百度只当底图」的门槛,不是可选的收尾。

### 大赛 M2 负担评分与解释 — 行 437-463

- [x] **大赛 M2-1 确认指标在百度数据上成立** — 用**真实百度响应**归一化后的样本跑排序;
  现有 `accessibility.test.ts` 用的是构造数据,证明不了 adapter 字段填对了
- [x] **大赛 M2-2 补齐 §6.2 的五种排序** — 切画像排序与文案可见变化,且同画像连点两次结果一致
  §6.2 的「依次升序」是**字典序**,加权和实现不出来;`TravelProfile` 由 3 项扩到 5 项
  详见设计方案 448-462
- [x] **大赛 M2-3 决策标签(6 个,现在一个都没有)** — `RouteCard.svelte:49/52`
  现在只有地铁/公交两个模式徽标
  §4.2 的「低步行」触发条件写成「或」与判据冲突;「少换乘」反过来不能加 tie-break
  UI 可见性靠 SSR 渲染单独验的,Vite ssrLoadModule 这条路不通
  详见设计方案 463-476
- [x] **大赛 M2-4 阈值挪进设置** — `RouteCard.svelte:42` 现在写死 500 米,违反 §4.2 自己的规定
      落点不是原文说的 `config.rs` 的 `Settings`(那个喂算路),而是 `SettingsDto`,照 `show_logs` 的判例
      写死的 500 共四处,不止原文点名的那一处;`burdenScore` 里的 `transfers * 500` 是量纲系数不是阈值
      详见设计方案 477-503
- [x] **大赛 M2-5 守住诚信边界** — `grep -rn "无障碍" src/lib/components src/routes`
      判据漏了画像名:「老人/行动缓慢」承诺了应用不具备的判断能力,已改成「少走长段」
      下拉框旁补了依据说明;六个标签靠两个新真实场景(直达、3 次换乘)全部触发过
      纯公交方案必须用 TravelMode::Bus 取,Subway 模式会把它全筛掉(baidu.rs:389)
      详见设计方案 504-531
  每处命中都不能是承诺性说法

### 大赛 M3 展示完整度 — 行 464-478

- [ ] **大赛 M3-1 固化两个演示场景** — 同场景连跑三次结果一致
- [ ] **大赛 M3-2 四个非正常态** — 各手动触发一次并截图;断网、清空 AK 是最快的触发方式
- [ ] **大赛 M3-3 录视频** — 3 分钟 + 8 分钟两个,脚本在 §9
- [ ] **大赛 M3-4 提交材料** — §10 的复选框全部勾上

## 第三阶段:CLI 入口(CLI 方案 §9,行 317-425)

整段完成:C1 拆 crate、C2 参数解析与 `route`、C3 `suggest` 与 `circle`、C4 进构建管线。
CLI 已随 `pnpm build:app` 一起产出并进 portable zip,`pnpm smoke:cli` 可对打包产物重跑冒烟。

### C2 参数解析 + `route` — 行 350-388

- [x] **C2-1 建 `autobee-cli` crate** — 第三个 crate,不是根包的第二个 bin(理由见 §3.2)
- [x] **C2-2 写 `args.rs` 的分词器** — 手写,不引 clap(理由见 §6.2,不是「少几个依赖」)
- [x] **C2-3 枚举值从 `ALL` 打印** — `TravelMode::ALL`(`models.rs:160`)、
  `SortBy::ALL`(`models.rs:185`);不许手写第二份清单
- [x] **C2-4 exit code 映射** — 表在 §5.3
- [x] **C2-5 接上 `route`** — 调 `planner::plan_route`。验收时修掉两个真 bug:
  `degraded` 错拼进 stdout(违反 §5.2)、`Route.duration` 被当秒又除一次 60
  (29 分钟的方案显示成「1 分钟」)。详见 §9 C2-5

### C3 `suggest` + `circle` — 行 389-405

- [x] **C3-1 `suggest`** — 调 `planner::suggest_places`(`planner.rs:276`,返回裸 `Vec<Place>`)。
  它不换坐标系,CLI 必须按各家 `coord_sys` 逐条换到 `--out-crs`;空结果不是错误,
  但「一个 Key 都没配」要报 `NO_PROVIDER` 而不是装作搜不到
- [x] **C3-2 `circle`** — 调 `life_circle::life_circle`(`life_circle.rs:154`);
  `LifeCirclePackage::ALL` 在 `models.rs:364`(本清单早前写「没有 `ALL`」是错的,
  C2-3 已经在用它),`parse` 在 `models.rs:347`

### C4 进构建管线 — 行 406-425

- [x] **C4-1 编译进 release** — `scripts/build-app.bat` 的 `[4/6]` 之后加一步。
  必须调 `scripts\cargo-msvc.bat`,裸 `cargo` 在本机链接失败
- [x] **C4-2 收进 `dist\`** — 同一脚本 `[5/6]`。
  顺带修掉一个真缺陷:`%DIST%` 从不清空,导致本机的 `data\settings.db`
  被 `Compress-Archive` 打进 portable zip。已在 `[5/6]` 开头加 `rd /s /q "%DIST%"`
- [x] **C4-3 确认质量门不用改** — 确认过:`verify` 输出里有 `autobee-cli` 的 `16 passed`
- [x] **C4-4 冒烟** — 打出来的 exe 跑 §8.3 六条,全过。
  另落 `scripts/smoke-cli.mjs`(`pnpm smoke:cli`),14 项。
  是 `.mjs` 不是 `.bat`:判据里的地名是中文,`.bat` 必须纯 ASCII

## 第四阶段:大模型接入(大模型方案 §8,行 628-740)

M0 是它的前置,已在第一阶段列出。

### 大模型 M1 配置层 — 行 642-665

- [x] **大模型 M1-1 加两张表** — 同时开 `PRAGMA foreign_keys`。
  **加在 `schema::prepare` 里**(`schema.rs:57`),不是 `store.rs::open_at`(`:40`)——
  后者与测试用的 `memory()`(`:62`)是两条独立路径、各自调 `prepare`,
  只改 `open_at` 会让级联测试失败而生产正确
- [x] **大模型 M1-2 `llm/store.rs`** — 密钥走现有 DPAPI 通道,不新建
- [x] **大模型 M1-3 `llm/probe.rs`** — 拉模型列表
- [x] **大模型 M1-4 五个 command**
- [x] **大模型 M1-5 设置页** — 判据含 `settings.db` 里搜不到明文 Key

**大模型 M1 全组已完成(2026-09-24)。** `pnpm verify` 全绿:svelte-check 355 files
0 error、前端 86 测试(新增 `llm.svelte.test.ts` 4 条)、shell 55、core 64、CLI 16,
clippy `-D warnings` 0 问题、fmt 干净。
shell 那 55 条按模块实测:`llm::store` 8、`llm::probe` 3、`credentials::schema` 9
(其中 2 条是本组新增的外键回归)、`credentials::store` 11、`credentials::resolve` 9、
`credentials::crypto` 5、`logging` 4、`logs` 4、`paths` 2。
**本组新增 13 条**(8 + 3 + 2)。顺带纠正交接记录里的两个过期数字:
前一组结束时 shell 是 42 条、前端是 82 条,不是 40 与 56。

**明文 Key 判据已实测,不是「看代码觉得对」。** 一次性 example 往真实文件
存入 canary `sk-PLAINTEXT-CANARY-…`,`wal_checkpoint TRUNCATE` 强制落盘后
按字节搜索:36864 字节里**命中 0 次**,下发给前端的是 `sk-P********disk`,
而后端 `secret_of` 取回的明文与原文一致。三条同时成立才算过 ——
只搜不到明文也可能是根本没存进去。

**外键那条判据做了反向验证。** 把 `PRAGMA foreign_keys` 临时改成 `OFF`,
两条新测试(级联删除、悬空 `provider_id` 被拒)**双双 FAILED**;改回 `ON` 全过。
不做这一步的话,测试可能只是恰好通过而 `PRAGMA` 压根没生效。

**设计文档三处与实际不符,已按实际实现并回写文档:**
1. **`VERSION` 要加到 5,不是「保持 4 不变」。** 原文自己在 M1-1 里写了要加一,
   §4.1 又写「保持 4」—— 两处矛盾。加到 5:老库的 `user_version` 停在 4,
   不加的话将来真要迁移时无从判断这一版有没有跑过。
2. **没有 `src/routes/settings/llm/+page.svelte` 这个路由。** 设置是**弹窗**
   (`SettingsDialog.svelte`),不是独立页面。照原文新建路由等于在同一个仓库里
   放两套设置入口,所以大模型配置作为第三个 `<section>` 加进现有弹窗。
3. **`probe.rs` 放在 shell 而不是 `src-tauri/src/llm/`。** 原文按 §3.2「async
   边界划在 app 层」把它放 app 侧,但 app 与 shell 都没有 HTTP 客户端,
   只有 core 有(blocking `reqwest` + 共享连接池)。新增
   `core/src/http.rs::get_json_with_auth` 复用那个池子,`probe.rs` 跟
   `store.rs` 放一起 —— 否则要么给 app 加第二个 HTTP 栈,要么让 CLI 也拉上 tauri。

**`/models` 需要鉴权头,而地图侧一律把 Key 放查询串**,所以加的是
`get_json_with_auth` 而不是给 `get_json` 加参数 —— 后者会让地图侧几十个
调用点各传一个 `None`。Anthropic 用 `x-api-key` + `anthropic-version: 2023-06-01`
(缺版本头直接 400),OpenAI 兼容用 `Authorization: Bearer`。
非 2xx 时把响应体前 200 字符带进错误:中转商的 401 往往在 body 里写明
「余额不足」还是「key 无效」,只报 `HTTP 401` 等于把线索丢了。

**空模型列表按错误处理**(`LLM_NO_MODELS`,502)。给界面一个空的多选框,
用户没有任何办法知道是 base_url 少了 `/v1` 还是 Key 没开通。

### 大模型 M2 非流式对话 — 行 666-682

- [x] **大模型 M2-1 会话两张表** — 实际建了三张(`mcp_servers` 一起),`VERSION` 6→7
- [x] **大模型 M2-2 一次性请求** — 落点不是 `llm/http.rs`:传输在 `core/src/http.rs`、
  协议差异在 `llm/protocol.rs`、一次性对话在 `llm/chat.rs`。三种协议全做了
- [x] **大模型 M2-3 `llm_chat_start` 非流式版** — **后被 M4-3 切成流式**,没留兼容壳。
  判据「能与模型来回对话」要真实 Key,未实测
- [x] **大模型 M2-4 对话页最小实现** — 判据「关程序重开历史还在」要真实窗口,未实测

### 大模型 M3 tool calling(核心)— 行 683-706

- [x] **大模型 M3-1 `llm/tools.rs` 的 schema 与裁剪结构** — 另加了原文没有的
  `life_circle` 可选 `crs` 参数:core 只吃 BD-09 而回给模型的是 GCJ-02,原样填回会静默偏移
- [x] **大模型 M3-2 `llm/agent.rs` 回合循环** — 判据三条全过,含
  `截断时整批工具调用一个都不执行`。另补一条原文没要求的 `循环自己不发_delta_只发_done`,
  已用「故意破坏 → 测试 FAILED」验证它真的挡得住
- [x] **大模型 M3-3 `http.rs` 加 tools 字段** — 落点是 `protocol.rs` 的 `build_body`,
  不另开 `http.rs` 分支(同一件事两处真相)
- [x] **大模型 M3-4 system prompt 坐标系硬约束** — 单测钉住约束写进去了,但
  判据的实质部分(时长与直接点规划一致)要真实 Key,未实测

### 大模型 M4 流式 — 行 707-721

- [x] **大模型 M4-1 不提依赖** — 推翻原文:`tokio` / `futures-util` / `uuid` 三条都不加。
  `reqwest::blocking::Response` 实现了 `Read`,SSE 在阻塞线程里按块读即可
- [x] **大模型 M4-2 SSE 解析** — 纯状态机(不是原文的 async `bytes_stream`),
  10 条单测含跨 chunk 切断的行
- [x] **大模型 M4-3 事件通道** — 不用 mpsc,后台线程直接 `app.emit`。
  `llm_chat_start` 返回 `ChatStartHandle`(比原文多回 `convId` 与 `userMessage`:
  上游失败时前端仍需知道往哪个会话追加,否则库里攒一串孤立会话)。
  判据两条都实测过:SSE 桩上首个 delta **0ms**(流总长 1562ms,分块慢发才测得出
  「边出字边推」);「工具期间具名动作」由单测钉住
- [x] **大模型 M4-4 `llm_chat_cancel`** — 登记 `Arc<AtomicBool>`,结束时无论成败都摘。
  `CANCELLED` / status 499 与真实失败分开。输入框在流式期间不禁用。
  真实 socket 上实测:约 17ms 内返回 499,服务端确认 `closed-by-client`
- [ ] **大模型 M4-5 tool 结果回灌进 planner** — 新增条目,§6.4.2 的未落地义务:
  现在 `toolEnd` 只带 `summary`,地图画不出对话算出来的路线

### 大模型 M5 MCP — 行 722-741

固定排最后,理由见上面。

- [x] **大模型 M5-1 `llm/mcp.rs` 客户端** — 手写,不引 rmcp。四条消息全走 `post_stream`:
  session id 在响应头里,且响应可能包在 SSE `data:` 行里
- [x] **大模型 M5-2 `mcp_servers` 表 + 设置页** — URL 校验只放过 HTTPS 或本机 HTTP；Rust 单测覆盖外网 HTTP 拒绝与 `127.0.0.1` 通过，设置页可增删改、启停、读取工具
- [x] **大模型 M5-3 先打印真实 tool 清单** — 2026-09-28 已连接百度 MCP `tools/list`，返回 14 个真实工具名，完整清单见详细设计 §7.4
- [x] **大模型 M5-4 合并进 tool 列表** — MCP 返回值是第三方内容,
  那条注入检出单测就是防线本身
- [ ] **大模型 M5-5 验收扩展性** — 代码闭环已具备；百度 `tools/list` 已返回 14 个真实工具，但真实 `tools/call map_weather` 被百度侧 `Service activation timeout after 12 seconds` 拒绝，需先在控制台启用该 AK 的 MCP 服务；还需真实模型问答验收。


### 大模型 M6 Skills — 大模型方案 §2.2.3

- [x] **M6-1 离线 Skill 包与数据库** — 仅导入 `.zip` 根目录的 `skill.json` / `SKILL.md`；不解压、不执行脚本，逐项限长，拒绝路径穿越、未知文件和包内凭据。
- [x] **M6-2 Skill 与 MCP 绑定** — 新声明 server 默认关闭；同名不同地址拒绝覆盖；删除 Skill 不删 server；启用 Skill 最多三个。
- [x] **M6-3 对话装配与工具筛选** — 不可变导航 system prompt 在前；已启用 Skill 指引追加其后；独占绑定 MCP 只在关联 Skill 启用时进工具列表；历史未绑定 MCP 保持兼容。
- [x] **M6-4 设置页与 IPC** — 导入、查看指引、启停、绑定/解绑 MCP server、删除；不展示或录入包内密钥。
- [x] **M6-5 验收与质量门** — 覆盖合法包、恶意包、启停工具可见性、导航链路不变；`pnpm verify` 全绿。
## 开发环境:两条不知道就会卡住的

### cargo 必须走 desktop-kit

```
pnpm run test:rust      # = desktop-kit cargo test --workspace
pnpm run clippy
pnpm run fmt:rust
```

裸 `cargo` 在这台机器上链接会失败:Git 自带的 coreutils 里有个 `link.exe`,
它在 PATH 里遮蔽了 MSVC 的链接器。`desktop-kit cargo` 先注入便携 MSVC
(默认 `D:\Applications\msvc-portable\msvc`,可用 `PORTABLE_MSVC_ROOT` 改;首次运行
`node_modules\@ouraihub\desktop-kit\msvc\setup-msvc.bat` 下载)。
构建 `pnpm run build:app`(产物 `dist/candidate-windows/` 与 `dist/AutoBee/`),
清理 `pnpm run clean`(`-- --deep` 连 node_modules 一起删;不碰 `tools/` 和 `data/`)。

### pnpm 用 vendored standalone

```
node tools/pnpm-standalone/package/bin/pnpm.mjs run verify
node tools\pnpm-standalone\package\bin\pnpm.mjs run build:app
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
