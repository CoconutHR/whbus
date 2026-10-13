/**
 * 在 CI 构建前把版本号写进构建产物。
 *
 * # 为什么必须在构建期注入
 *
 * "检查更新"靠比较**已安装版本**与远端 tag。但工作流的 Release tag 是
 * 滚动的 `latest`（每次 push 覆盖同一个 Release），tag 名解析成数字是 0，
 * 于是无论怎么更新都会显示"已是最新"——功能形同虚设。
 *
 * 所以构建时必须把一个**单调递增**的版本号写进：
 *   - `src-tauri/tauri.conf.json` 的 `version`（进 APK 的 manifest）
 *   - `package.json` 的 `version`（前端也读它做展示）
 *
 * # 版本号怎么定
 *
 * - 打了 `v*` tag：用 tag（`v1.2.3` → `1.2.3`）
 * - 其余情况（main 滚动发布）：用 `0.YYMMDD.<run_number>`，
 *   例如 `0.261013.42`。日期段保证比昨天的大，
 *   run_number 保证同一天多次构建也单调递增。
 */

import { readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** 读取要写入的版本号 */
function resolveVersion() {
  const refType = process.env.GITHUB_REF_TYPE ?? "";
  const refName = process.env.GITHUB_REF_NAME ?? "";
  const runNumber = process.env.GITHUB_RUN_NUMBER ?? "0";

  // 显式指定的版本优先（便于本地或手动触发时覆盖）
  if (process.env.WHBUS_VERSION) {
    return { version: process.env.WHBUS_VERSION, source: "WHBUS_VERSION 环境变量" };
  }

  if (refType === "tag" && /^v\d/.test(refName)) {
    return { version: refName.replace(/^v/, ""), source: `tag ${refName}` };
  }

  // 滚动发布：0.YYMMDD.<run>。日期用 UTC，与 CI 一致。
  const d = new Date();
  const yy = String(d.getUTCFullYear()).slice(2);
  const mm = String(d.getUTCMonth() + 1).padStart(2, "0");
  const dd = String(d.getUTCDate()).padStart(2, "0");
  return {
    version: `0.${yy}${mm}${dd}.${runNumber}`,
    source: `main 滚动发布（run #${runNumber}）`,
  };
}

const { version, source } = resolveVersion();

// 语义化版本校验：安卓的 versionName 不接受任意字符串
if (!/^\d+\.\d+\.\d+/.test(version)) {
  console.error(`✗ 版本号 "${version}" 不是合法的 x.y.z 形式`);
  process.exit(1);
}

function patchJson(relPath, mutate) {
  const p = join(root, relPath);
  const raw = readFileSync(p, "utf8");
  const obj = JSON.parse(raw);
  const before = obj.version;
  mutate(obj, version);
  // 保留原文件末尾换行与 2 空格缩进，避免产生无意义的整文件 diff
  writeFileSync(p, `${JSON.stringify(obj, null, 2)}\n`, "utf8");
  console.log(`  ${relPath}: ${before} → ${version}`);
}

console.log(`版本号来源：${source}`);
patchJson("src-tauri/tauri.conf.json", (o, v) => {
  o.version = v;
});
patchJson("package.json", (o, v) => {
  o.version = v;
});

// Cargo.toml 单独处理：它没有 version 字段时不必强加，
// 但 tauri 会读它做校验，所以存在就同步。
const cargoPath = "src-tauri/Cargo.toml";
const cargo = readFileSync(join(root, cargoPath), "utf8");
const next = cargo.replace(
  /^(version\s*=\s*)"[^"]*"/m,
  (_m, prefix) => `${prefix}"${version}"`,
);
if (next !== cargo) {
  writeFileSync(join(root, cargoPath), next, "utf8");
  console.log(`  ${cargoPath}: version → ${version}`);
}

// 顺手打个日志，便于在 CI 输出里核对
try {
  const sha = execSync("git rev-parse --short HEAD", { encoding: "utf8" }).trim();
  console.log(`  commit: ${sha}`);
} catch {
  // 没有 git 也能继续
}

console.log(`✓ 版本号已写入：${version}`);
