// Render an SVG to PNG with Chromium: node scripts/brand/render-svg.mjs in.svg out.png width height [background]
import { chromium } from "playwright";
import { readFileSync } from "node:fs";
const [src, out, w, h, bg] = process.argv.slice(2);
const b = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined });
const p = await b.newPage({ viewport: { width: +w, height: +h }, deviceScaleFactor: 1 });
await p.setContent(`<html><body style="margin:0;background:${bg || "transparent"}">${readFileSync(src, "utf8").replace("<svg ", `<svg width="${w}" height="${h}" `)}</body></html>`);
await p.screenshot({ path: out, omitBackground: !bg });
await b.close();
