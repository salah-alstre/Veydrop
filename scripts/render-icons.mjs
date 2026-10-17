// Renders branding/veydrop-mark.svg to PNGs used by `tauri icon` and the web apps.
import { Resvg } from "../desktop/node_modules/@resvg/resvg-js/index.js";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
const svg = readFileSync(new URL("../branding/veydrop-mark.svg", import.meta.url), "utf8");
mkdirSync(new URL("../branding/png/", import.meta.url), { recursive: true });
for (const size of [1024, 512, 192, 180, 32]) {
  const png = new Resvg(svg, { fitTo: { mode: "width", value: size } }).render().asPng();
  writeFileSync(new URL(`../branding/png/veydrop-${size}.png`, import.meta.url), png);
}
console.log("ok");
