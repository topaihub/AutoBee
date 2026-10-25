// CLI 冒烟检查,跑在**打包产物**上(CLI 设计方案 §8.3)。
//
//   node scripts/smoke-cli.mjs                 # 默认 dist\AutoBee
//   node scripts/smoke-cli.mjs "别处\目录"
//
// 为什么必须对着打包产物跑而不是 `cargo run`:§3.1 约束一是「exe 真的往控制台
// 打字」。debug 构建能过而 release 静默是可能的 —— `windows_subsystem` 只在
// release 生效。只有在 shipped exe 上跑才回归得了这条。
//
// 为什么是 .mjs 而不是 .bat:判据里的地名是中文,而 .bat 必须保持纯 ASCII
// (cmd.exe 按 OEM 代码页读文件,行内 UTF-8 中文会破坏解析)。把中文放进
// 参数数组里由 Node 直接 spawn,绕开整个代码页问题。

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const target = process.argv[2] ?? path.join(root, "dist", "AutoBee");
const cli = path.join(target, "autobee-cli.exe");

if (!existsSync(cli)) {
  console.error(`ERROR: 找不到 autobee-cli.exe:${cli}`);
  console.error("       先跑 pnpm run build:app,或传一个目录进来。");
  process.exit(1);
}

/** 跑一次 CLI,拿到退出码与两个流。 */
function run(args) {
  const result = spawnSync(cli, args, { encoding: "utf8" });
  return {
    code: result.status,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
}

let passed = 0;
let failed = 0;

function check(label, ok, detail) {
  if (ok) {
    passed += 1;
    console.log(`  PASS  ${label}`);
  } else {
    failed += 1;
    console.log(`  FAIL  ${label}${detail ? ` — ${detail}` : ""}`);
  }
}

/** 退出码必须等于 want。 */
function expectCode(label, want, args) {
  const { code } = run(args);
  check(`${label}(exit ${want})`, code === want, `实际 exit ${code}`);
}

console.log("============================================");
console.log(`  AutoBee CLI 冒烟 — ${cli}`);
console.log("============================================\n");

// 不需要 Key 的那几条:纯参数解析。
expectCode("--help", 0, ["--help"]);
expectCode("--version", 0, ["--version"]);
expectCode("--mode 不认识的值", 2, ["route", "--from", "x", "--to", "y", "--mode", "transit"]);
expectCode("未知 flag", 2, ["route", "--from", "x", "--to", "y", "--nonsense"]);
expectCode("缺 --to", 2, ["route", "--from", "x"]);

// `--help` 得真的有内容:退出码 0 但一个字不打正是 §3.1 约束一要挡的那个手滑。
{
  const { stdout } = run(["--help"]);
  check("--help 真的打了用法", stdout.includes("用法") && stdout.length > 200, `只有 ${stdout.length} 字节`);
}

// 报错要把合法值列出来,否则调用方只知道「错了」不知道该填什么。
{
  const { stdout, stderr } = run(["route", "--from", "x", "--to", "y", "--mode", "transit"]);
  check("合法值列在 stderr", stderr.includes("subway") && stderr.includes("walking"), stderr.trim());
  check("报错时 stdout 干净", stdout === "", `stdout 有 ${stdout.length} 字节`);
}

// Key 在 exe 同目录那份 DPAPI 加密库里,没配就跳过算路。跳过不等于通过,
// 下面的计数会分别说清楚。
const probe = run(["suggest", "--keyword", "test"]);
const haveKey = probe.code !== 4;

if (!haveKey) {
  console.log("\n  SKIP  算路相关检查:这个安装的档案库里没有 API Key。");
  console.log("        去设置页填一个再重跑。\n");
} else {
  expectCode("route 文本", 0, ["route", "--from", "南京南站", "--to", "新街口", "--mode", "subway"]);
  expectCode("suggest", 0, ["suggest", "--keyword", "孝陵卫"]);
  expectCode("circle", 0, ["circle", "--at", "32.0603,118.7969", "--package", "care"]);

  // §5.2 的管道契约:stdout 必须是一个干净 JSON —— 没有日志、没有进度、
  // 没有降级说明。这就是 tracing writer 必须设成 stderr 的原因。
  const { code, stdout } = run(["route", "--from", "南京南站", "--to", "新街口", "--json"]);
  let parsed = null;
  let why = "";
  try {
    parsed = JSON.parse(stdout);
  } catch (error) {
    why = error.message;
  }
  check("--json 的 stdout 是干净 JSON", parsed !== null, why);
  check("--json 有方案", Array.isArray(parsed?.routes) && parsed.routes.length > 0, `exit ${code}`);

  // 耗时按分钟:曾经把分钟当秒再除 60,长途方案全塌成「1 分钟」。
  const text = run(["route", "--from", "南京南站", "--to", "新街口", "--mode", "subway"]).stdout;
  const minutes = [...text.matchAll(/(\d+) 分钟/g)].map((m) => Number(m[1]));
  check(
    "耗时不是全塌成 1 分钟",
    minutes.length > 0 && minutes.some((value) => value > 5),
    `解析到 ${JSON.stringify(minutes)}`,
  );
}

console.log("\n============================================");
console.log(`  通过 ${passed}   失败 ${failed}`);
console.log("============================================");
process.exit(failed > 0 ? 1 : 0);
