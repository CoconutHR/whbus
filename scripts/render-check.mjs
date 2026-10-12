/**
 * 用 CDP 在无头 Edge 里加载页面，捕获完整报错与布局尺寸。
 *
 * 之所以自己写：浏览器工具依赖未安装的 bsk，而 Chrome DevTools Protocol
 * 本身用 HTTP + WebSocket 就能驱动，不需要额外依赖。
 */
import { spawn } from "node:child_process";
import http from "node:http";

const EDGE = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const PORT = 9222;
const URL = process.argv[2] ?? "http://127.0.0.1:1420";
const WIDTH = Number(process.argv[3] ?? 412);
const HEIGHT = Number(process.argv[4] ?? 892);
const OUT = process.argv[5] ?? null;

const child = spawn(
  EDGE,
  [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--no-first-run",
    "--disable-extensions",
    `--remote-debugging-port=${PORT}`,
    `--window-size=${WIDTH},${HEIGHT}`,
    `--user-data-dir=${process.env.TEMP}\\whbus_cdp`,
    "about:blank",
  ],
  { stdio: "ignore", detached: false },
);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function getJson(path) {
  return new Promise((resolve, reject) => {
    http
      .get({ host: "127.0.0.1", port: PORT, path }, (res) => {
        let d = "";
        res.on("data", (c) => (d += c));
        res.on("end", () => {
          try {
            resolve(JSON.parse(d));
          } catch (e) {
            reject(e);
          }
        });
      })
      .on("error", reject);
  });
}

async function waitForDevtools() {
  for (let i = 0; i < 40; i++) {
    try {
      return await getJson("/json/version");
    } catch {
      await sleep(250);
    }
  }
  throw new Error("DevTools 未就绪");
}

// ---- 极简 CDP 客户端（避免引入 ws 依赖，用原生 WebSocket）----
class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    this.events = [];
    ws.addEventListener("message", (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
      } else if (msg.method) {
        this.events.push(msg);
      }
    });
  }
  send(method, params = {}) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
}

const main = async () => {
  await waitForDevtools();

  // 关键：/json/version 给的是 **browser** target，它没有 Runtime/Page 域。
  // 必须从 /json/list 里取一个 type=page 的 target 才能做渲染检查。
  let targets = [];
  for (let i = 0; i < 20; i++) {
    targets = await getJson("/json/list");
    if (targets.some((t) => t.type === "page" && t.webSocketDebuggerUrl)) break;
    await sleep(250);
  }
  const page = targets.find((t) => t.type === "page" && t.webSocketDebuggerUrl);
  if (!page) throw new Error("找不到 page target");

  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.addEventListener("open", res);
    ws.addEventListener("error", rej);
  });

  const cdp = new Cdp(ws);
  await cdp.send("Runtime.enable");
  await cdp.send("Log.enable");
  await cdp.send("Page.enable");
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: WIDTH,
    height: HEIGHT,
    deviceScaleFactor: 1,
    mobile: true,
  });

  await cdp.send("Page.navigate", { url: URL });
  await sleep(5000);

  // ---- 收集报错 ----
  const errors = [];
  for (const e of cdp.events) {
    if (e.method === "Runtime.exceptionThrown") {
      const d = e.params.exceptionDetails;
      errors.push(
        `EXCEPTION: ${d.exception?.description ?? d.text}\n` +
          (d.stackTrace?.callFrames ?? [])
            .slice(0, 6)
            .map((f) => `    at ${f.functionName || "<anon>"} (${f.url}:${f.lineNumber + 1}:${f.columnNumber + 1})`)
            .join("\n"),
      );
    }
    if (e.method === "Log.entryAdded" && e.params.entry.level === "error") {
      errors.push(`LOG: ${e.params.entry.text}`);
    }
  }

  // ---- 量布局 ----
  const evalJs = async (expr) => {
    const r = await cdp.send("Runtime.evaluate", {
      expression: expr,
      returnByValue: true,
      awaitPromise: false,
    });
    if (r.exceptionDetails) {
      return `__EXCEPTION__ ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`;
    }
    return r.result?.value;
  };

  // 可选：依次点击底部 Tab，验证每个界面都能渲染
  const tabCount = await evalJs(`document.querySelectorAll('.tabbar__item').length`);
  const perTab = [];
  for (let i = 0; i < (tabCount || 0); i++) {
    await evalJs(`document.querySelectorAll('.tabbar__item')[${i}]?.click()`);
    await sleep(1200);
    const info = await evalJs(`JSON.stringify({
      label: document.querySelectorAll('.tabbar__item')[${i}]?.textContent,
      errs: [...document.querySelectorAll('.notice--danger, .state__title')]
              .map(e => e.textContent).slice(0, 3),
      overflowRight: [...document.querySelectorAll('*')]
        .filter(e => e.getBoundingClientRect().right > window.innerWidth + 1).length,
      text: (document.querySelector('.app__pane:not([hidden])')?.innerText ?? '').slice(0, 220)
    })`);
    perTab.push(JSON.parse(info));
  }

  const layout = await evalJs(`JSON.stringify({
    innerWidth: window.innerWidth,
    innerHeight: window.innerHeight,
    scrollWidth: document.documentElement.scrollWidth,
    scrollHeight: document.documentElement.scrollHeight,
    bodyScrollWidth: document.body.scrollWidth,
    rootChildren: document.getElementById('root')?.children.length ?? -1,
    tabs: [...document.querySelectorAll('.tabbar__item')].map(e => e.textContent),
    tabbarWidth: document.querySelector('.tabbar')?.getBoundingClientRect().width ?? -1,
    overflowers: [...document.querySelectorAll('*')]
      .filter(e => e.getBoundingClientRect().right > window.innerWidth + 1)
      .slice(0, 8)
      .map(e => e.className + ' right=' + Math.round(e.getBoundingClientRect().right)),
    text: document.body.innerText.slice(0, 400)
  })`);

  console.log("=== 报错 ===");
  console.log(errors.length ? errors.join("\n") : "（无）");
  console.log("\n=== 布局 ===");
  console.log(JSON.stringify(JSON.parse(layout), null, 2));

  if (perTab.length) {
    console.log("\n=== 逐个 Tab 渲染检查 ===");
    for (const t of perTab) {
      console.log(`\n[${t.label}]  横向溢出元素=${t.overflowRight}`);
      if (t.errs?.length) console.log("  可见状态文案:", t.errs.join(" | "));
      console.log("  正文:", t.text.replace(/\n/g, " / "));
    }
  }

  if (OUT) {
    const shot = await cdp.send("Page.captureScreenshot", { format: "png" });
    const fs = await import("node:fs");
    fs.writeFileSync(OUT, Buffer.from(shot.data, "base64"));
    console.log(`\n截图已写入 ${OUT}`);
  }

  ws.close();
  child.kill();
  process.exit(0);
};

main().catch((e) => {
  console.error("失败:", e.message);
  child.kill();
  process.exit(1);
});
