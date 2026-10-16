// Fast UI smoke test on the generated valley: drives the built UI in
// Chromium with every invoke() forwarded to `lifeline rpc`, pans and zooms
// the map, and fails on any page error.
//
//   cargo build --release -p lifeline-cli
//   npm run build && npx vite preview --port 4173 &
//   node scripts/smoke.mjs [out-dir] [url]
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const BIN = process.env.LIFELINE_BIN ?? fileURLToPath(new URL("../target/release/lifeline", import.meta.url));
const OUT = process.argv[2] ?? "smoke";
const PAGE = process.argv[3] ?? "http://localhost:4173";
mkdirSync(OUT, { recursive: true });

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.exposeFunction(
  "__rpc",
  (cmd, args) =>
    new Promise((resolve, reject) => {
      if (cmd === "open_text" || cmd === "save_text") return resolve(null);
      const p = spawn(BIN, ["rpc"]);
      let out = "";
      p.stdout.on("data", (d) => (out += d));
      p.on("close", () => {
        try {
          const r = JSON.parse(out);
          r.error ? reject(new Error(r.error)) : resolve(r.ok);
        } catch (e) {
          reject(e);
        }
      });
      p.stdin.end(JSON.stringify({ cmd, args }));
    }),
);
await page.addInitScript(() => {
  localStorage.clear();
  window.__TAURI_INTERNALS__ = { invoke: (c, a) => window.__rpc(c, a), transformCallback: () => 0 };
});
await page.goto(PAGE);
await page.waitForSelector(".pmap-svg", { timeout: 60_000 });
await page.waitForFunction(() => document.querySelectorAll(".pmap-handle").length > 3, null, { timeout: 60_000 });
await page.waitForTimeout(1500);
await page.screenshot({ path: `${OUT}/plan.png` });

// pan and zoom, timing the frames
const box = await page.locator(".pmap-svg").boundingBox();
const t0 = Date.now();
await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
await page.mouse.down();
for (let i = 0; i < 60; i++) await page.mouse.move(box.x + box.width / 2 + i * 4, box.y + box.height / 2 + i * 2);
await page.mouse.up();
for (let i = 0; i < 20; i++) await page.mouse.wheel(0, -60);
console.log(`pan+zoom: ${Date.now() - t0} ms`);
await page.waitForTimeout(500);
await page.screenshot({ path: `${OUT}/plan-zoomed.png` });

for (const tab of process.env.SMOKE_TABS?.split(",").filter(Boolean) ?? []) {
  await page.getByRole("tab", { name: tab }).click();
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${OUT}/tab-${tab.toLowerCase().replace(/\W+/g, "-")}.png` });
}

await browser.close();
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log("smoke OK");
