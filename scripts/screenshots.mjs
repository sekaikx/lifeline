// UI smoke test and README screenshots, without the desktop shell.
//
// Drives the built UI in Chromium; every Tauri invoke() is forwarded to
// `lifeline rpc`, the same Rust engine the desktop app embeds. Real areas
// download open data on first use (minutes), then load from the cache.
// Exits non-zero if the page logs an error.
//
//   cargo build --release -p lifeline-cli
//   npm run build && npx vite preview --port 4173 &
//   npm i --no-save playwright
//   node scripts/screenshots.mjs docs/img http://localhost:4173 light
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const BIN = process.env.LIFELINE_BIN ?? fileURLToPath(new URL("../target/release/lifeline", import.meta.url));
const OUT = process.argv[2] ?? "screenshots";
const PAGE = process.argv[3] ?? "http://localhost:4173";
const theme = process.argv[4] ?? "light";
mkdirSync(OUT, { recursive: true });

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH || undefined,
  args: ["--use-gl=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));

// async, so long downloads do not freeze the page
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
await page.addInitScript((theme) => {
  localStorage.clear();
  localStorage.setItem("lifeline.theme", theme);
  window.__TAURI_INTERNALS__ = { invoke: (c, a) => window.__rpc(c, a), transformCallback: () => 0 };
}, theme);

const shot = (name) => page.screenshot({ path: `${OUT}/${name}.png` });
const loadArea = async (label) => {
  await page.getByLabel("Load", { exact: true }).selectOption({ label });
  await page.getByRole("button", { name: "Load area and example plan" }).click();
  await page.waitForFunction(
    (n) => document.querySelector(".area-now strong")?.textContent === n,
    label.split(",")[0],
    { timeout: 600_000 },
  );
  await page.waitForTimeout(5000);
};

await page.goto(PAGE);
await page.waitForSelector(".req-row", { timeout: 60_000 });

await loadArea("Gorkha, Nepal");
await shot("plan");
await page.locator(".req-row").nth(3).click();
await page.waitForTimeout(800);
await shot("plan-request");
await page.getByRole("button", { name: "Satellite" }).click();
await page.waitForTimeout(12_000);
await shot("satellite");

await loadArea("Dadu, Pakistan");
await page.getByLabel(/As of/).fill("2022-09-05");
const detector = page.getByLabel("Detector");
if (await detector.count()) await detector.selectOption("model");
await page.getByRole("button", { name: "Detect new water" }).click();
await page.waitForSelector("text=New water on", { timeout: 600_000 });
await page.waitForTimeout(6000);
await shot("flood");

await page.getByRole("tab", { name: "Airdrop lab" }).click();
await page.waitForTimeout(4000);
await shot("airdrop-lab");

await browser.close();
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log(`ok: screenshots in ${OUT}`);
