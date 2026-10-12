/**
 * 给 tauri 生成的 Android 工程注入 release 签名配置。
 *
 * # 为什么需要这个脚本
 *
 * tauri-cli 的 `app/build.gradle.kts` 模板里**没有 signingConfigs**，
 * 因此 `assembleRelease` 产出的是 `app-universal-release-unsigned.apk`，
 * 而未签名的 APK **无法安装**（`INSTALL_PARSE_FAILED_NO_CERTIFICATES`）。
 *
 * 官方文档要求手工编辑该文件，但 `src-tauri/gen/android` 是生成物
 * （官方 .gitignore 默认忽略），所以改动必须在每次构建前重新注入，
 * 不能靠提交文件。这就是本脚本存在的理由。
 *
 * # 补丁内容
 *
 * 1. 在 `android {` 块内、`buildTypes {` 之前插入 `signingConfigs { create("release") { … } }`
 * 2. 给 `getByName("release")` 加上 `signingConfig = signingConfigs.getByName("release")`
 *
 * 密钥信息从 `keystore.properties` 读取（由 CI 写入，不进仓库）。
 *
 * # 健壮性
 *
 * 模板会随 tauri-cli 版本变化，因此每个 patch 都检查模式是否命中：
 * 命中不了就**明确报错并打印附近内容**，而不是静默产出一个未签名的包——
 * 后者会让你在安装失败时完全摸不着头脑。
 */

import fs from "node:fs";
import path from "node:path";

const gradlePath =
  process.argv[2] ?? "src-tauri/gen/android/app/build.gradle.kts";

if (!fs.existsSync(gradlePath)) {
  console.error(`✗ 找不到 ${gradlePath}`);
  console.error("  请先运行 `pnpm tauri android init`。");
  process.exit(1);
}

let src = fs.readFileSync(gradlePath, "utf8");
const before = src;

const fail = (msg, needle) => {
  console.error(`✗ ${msg}`);
  const i = needle ? src.indexOf(needle) : -1;
  if (i >= 0) {
    console.error("  附近内容：");
    console.error(
      src
        .slice(Math.max(0, i - 200), i + 400)
        .split("\n")
        .map((l) => "    " + l)
        .join("\n"),
    );
  }
  console.error(
    "\n  tauri-cli 的 Gradle 模板可能已变更，请对照更新 scripts/patch-android-signing.mjs。",
  );
  process.exit(1);
};

// ---- 0. 幂等：已注入过就直接退出 ----
if (src.includes('create("release")') && src.includes("keystore.properties")) {
  console.log("・签名配置已存在，跳过");
  process.exit(0);
}

// ---- 1. 补 import ----
// 模板已有 `import java.util.Properties`，没有 FileInputStream
if (!src.includes("import java.io.FileInputStream")) {
  if (!src.includes("import java.util.Properties")) {
    fail("找不到 `import java.util.Properties`，无法安全插入 import");
  }
  src = src.replace(
    "import java.util.Properties",
    "import java.util.Properties\nimport java.io.FileInputStream",
  );
  console.log("・已补 import java.io.FileInputStream");
}

// ---- 2. 插入 signingConfigs ----
const SIGNING_BLOCK = `
    // ---- release 签名配置（由 scripts/patch-android-signing.mjs 注入）----
    // 没有它 assembleRelease 只会产出 *-unsigned.apk，而那个包装不上。
    signingConfigs {
        create("release") {
            val keystorePropertiesFile = rootProject.file("keystore.properties")
            val keystoreProperties = Properties()
            if (keystorePropertiesFile.exists()) {
                keystoreProperties.load(FileInputStream(keystorePropertiesFile))
            }
            keyAlias = keystoreProperties["keyAlias"] as String
            keyPassword = keystoreProperties["password"] as String
            storeFile = file(keystoreProperties["storeFile"] as String)
            storePassword = keystoreProperties["password"] as String
        }
    }

    buildTypes {`;

if (src.includes("buildTypes {")) {
  // 只在 android {} 块里的那一处替换：模板中 buildTypes 只出现一次
  src = src.replace("    buildTypes {", SIGNING_BLOCK);
  console.log("・已插入 signingConfigs");
} else {
  fail("找不到 `buildTypes {`，无法插入 signingConfigs 块", "android {");
}

// ---- 3. 让 release 用上签名 ----
const RELEASE_ANCHOR = 'getByName("release") {';
if (src.includes(RELEASE_ANCHOR)) {
  src = src.replace(
    RELEASE_ANCHOR,
    `${RELEASE_ANCHOR}\n            signingConfig = signingConfigs.getByName("release")`,
  );
  console.log("・已为 release 指定 signingConfig");
} else {
  fail(`找不到 \`${RELEASE_ANCHOR}\``, "buildTypes {");
}

// ---- 4. 写回 ----
if (src === before) {
  fail("补丁未产生任何变更");
}

fs.writeFileSync(gradlePath, src);
console.log(`✓ 已注入 release 签名配置：${path.resolve(gradlePath)}`);

// ---- 5. 自检：确认三处都在 ----
const ok = [
  "import java.io.FileInputStream",
  'create("release")',
  'signingConfig = signingConfigs.getByName("release")',
].every((n) => src.includes(n));

if (!ok) {
  fail("自检失败：补丁后仍缺少必要片段");
}
console.log("✓ 自检通过");
