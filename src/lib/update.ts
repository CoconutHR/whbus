/**
 * 版本更新检查。
 *
 * # 为什么"只跳转、不自动装"
 *
 * 真正在应用内自动安装 APK，安卓必须满足两个硬条件：
 *
 * 1. 声明 `REQUEST_INSTALL_PACKAGES` 权限，并在系统设置里引导用户
 *    手动打开「允许安装未知应用」—— 这是个**用户手动步骤**，
 *    不打开就一定会失败；
 * 2. 通过 `FileProvider` 生成 `content://` URI 交给系统安装器，
 *    需要给 Android 工程加 Kotlin 代码与 `file_paths.xml`。
 *
 * 而本项目的 `src-tauri/gen/` 是 **CI 每次重新生成**的，加进去的原生代码
 * 没有稳定的落点。更关键的是：检查更新本身是纯收益的功能，
 * 不该因为"自动安装"这一步没做好而整个不做。
 *
 * 所以这里做**检查 + 一键跳转到 Release 直链**：
 * 用户点一下就到浏览器下载，装完覆盖安装即可（签名固定，能覆盖）。
 * 想省掉这一步的话，后续加 Kotlin 插件再补 `REQUEST_INSTALL_PACKAGES`。
 */

import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./api";

/** 仓库地址（Release 与 Assets 都从这里拼） */
const REPO = "CoconutHR/whbus";

export interface UpdateInfo {
  /** 形如 `v0.2.0` */
  version: string;
  /** 当前版本 */
  current: string;
  /** 发布时间（ISO 字符串） */
  publishedAt: string;
  /** 更新说明（Release body 的前若干行） */
  notes: string;
  /** 适合本机架构的 APK 资产名 */
  asset: string;
  /** 全部 APK 资产名 */
  assets: string[];
}

export type UpdateCheck =
  | { kind: "latest" }
  | { kind: "available"; info: UpdateInfo }
  /** 检查本身失败（无网络、被限流等）—— 不该当成"有新版本" */
  | { kind: "unknown"; message: string };

/** 后端返回的原始结构 */
interface RawRelease {
  version: string;
  publishedAt: string;
  notes: string;
  assets: string[];
}

/**
 * 检查是否有新版本。
 *
 * 版本比较在 Rust 侧做，因为它要处理 `v0.2.0` 与 `0.2.0` 的差异。
 * 这里只负责把结果翻译成 UI 能直接用的三态。
 *
 * **网络失败一律返回 `unknown`**，绝不返回 `latest` ——
 * 把"没查到"说成"已是最新"是在骗用户。
 */
export async function checkForUpdate(current: string): Promise<UpdateCheck> {
  if (!isTauri()) {
    return { kind: "unknown", message: "浏览器预览模式下无法检查更新" };
  }

  let raw: RawRelease | null;
  try {
    raw = await invoke<RawRelease | null>("check_update", { current });
  } catch (e) {
    return {
      kind: "unknown",
      message: `检查更新失败：${e instanceof Error ? e.message : String(e)}`,
    };
  }

  if (!raw) return { kind: "latest" };

  // 按本机架构挑一个最合适的包；挑不到就退回第一个 APK
  const prefer = pickAsset(raw.assets, navigator.userAgent);
  return {
    kind: "available",
    info: {
      version: raw.version,
      current,
      publishedAt: raw.publishedAt,
      notes: raw.notes,
      asset: prefer ?? raw.assets[0] ?? "",
      assets: raw.assets,
    },
  };
}

/**
 * 从资产列表里挑出适合本机架构的 APK。
 *
 * 安卓 WebView 的 UA 里带架构信息（如 `Android 14; Pixel 7`），
 * 但**不总是可靠**——所以这里只当作偏好，挑不到就用默认包。
 */
function pickAsset(assets: string[], ua: string): string | null {
  const apks = assets.filter((a) => a.endsWith(".apk"));
  if (apks.length === 0) return null;

  const has = (needle: string) =>
    apks.find((a) => a.toLowerCase().includes(needle));

  const u = ua.toLowerCase();
  if (u.includes("arm64") || u.includes("aarch64")) {
    return has("arm64") ?? has("v8a") ?? null;
  }
  if (u.includes("armv7") || u.includes("armeabi")) {
    return has("armeabi") ?? has("v7a") ?? null;
  }
  // 大多数现代手机是 arm64；默认取它比取 x86 合理
  return has("arm64") ?? has("universal") ?? apks[0] ?? null;
}

/** 用系统浏览器打开链接（走 opener 插件，不占用当前 WebView） */
export async function openUrl(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return;
  }
  const { openUrl: open } = await import("@tauri-apps/plugin-opener");
  await open(url);
}

/** Release 页面地址（用户手动查看全部版本时用） */
export function releaseUrl(): string {
  return `https://github.com/${REPO}/releases/latest`;
}
