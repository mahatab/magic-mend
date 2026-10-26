// Renders the app icon SVG to PNGs: the 1024 px source for `tauri icon`, plus tray icons.
import { Resvg } from "@resvg/resvg-js";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
const svg = readFileSync("assets/icon.svg", "utf8");
const render = (size, file, src = svg) => {
  const png = new Resvg(src, { fitTo: { mode: "width", value: size } }).render().asPng();
  writeFileSync(file, png);
};
mkdirSync("src-tauri/icons", { recursive: true });
render(1024, "assets/icon-1024.png");
render(64, "src/assets/logo-64.png");
render(32, "src-tauri/icons/tray.png");
// Paused tray icon: the same mark with gray ink and thread.
const gray = svg.replaceAll("#A33B2E", "#A3A6AD").replaceAll("#1C1B19", "#7A7E87");
render(32, "src-tauri/icons/tray-paused.png", gray);
console.log("icons written");
