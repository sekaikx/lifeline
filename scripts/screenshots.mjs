// UI smoke test + README screenshots without the desktop shell.
//
// Drives the built UI in Chromium; every Tauri invoke() is forwarded to
// `airdrop rpc`, i.e. the same Rust engine the desktop app embeds. Fails if
// the page logs any error.
//
//   cargo build --release -p lifeline-cli
//   npm run build && npx vite preview --port 4173 &
//   npm i --no-save playwright && npx playwright install chromium
//   node scripts/screenshots.mjs docs/img http://localhost:4173 dark
import { chromium } from "playwright";
import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const BIN = fileURLToPath(new URL("../target/release/lifeline", import.meta.url));
const OUT = process.argv[2] ?? "screenshots";
const PAGE = process.argv[3] ?? "http://localhost:4173";
const theme = process.argv[4] ?? "dark";
mkdirSync(OUT, { recursive: true });

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH || undefined,
  args: ["--use-gl=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.exposeFunction("__rpc", (cmd, args) => {
  if (cmd === "import_scenario" || cmd === "save_text") return null;
  const r = spawnSync(BIN, ["rpc"], { input: JSON.stringify({ cmd, args }), encoding: "utf8", maxBuffer: 1 << 28 });
  const reply = JSON.parse(r.stdout);
  if (reply.error) throw new Error(reply.error);
  return reply.ok;
});
await page.addInitScript((theme) => {
  localStorage.setItem("airdrop.theme", theme);
  window.__TAURI_INTERNALS__ = { invoke: (cmd, args) => window.__rpc(cmd, args), transformCallback: () => 0 };
}, theme);
await page.goto(PAGE);
await page.waitForSelector(".kpis", { timeout: 20000 });
await page.waitForTimeout(2500);
await page.screenshot({ path: `${OUT}/${theme}-1-drop.png` });

for (const [i, name] of [[2, "Release plan"], [3, "Dispersion"], [4, "Numerics"], [5, "Atmosphere"]]) {
  await page.getByRole("tab", { name }).click();
  await page.waitForTimeout(name === "Dispersion" ? 4000 : 1500);
  await page.screenshot({ path: `${OUT}/${theme}-${i}-${name.toLowerCase().replace(" ", "-")}.png` });
}
// back to 3D with Monte Carlo impacts overlaid
await page.getByRole("tab", { name: "3D drop" }).click();
await page.waitForTimeout(2500);
await page.screenshot({ path: `${OUT}/${theme}-6-drop-mc.png` });
await browser.close();
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log(`ok: screenshots in ${OUT}`);
