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
// Rendered at 128 px so the in-app logo stays sharp on high-DPI screens.
render(128, "src/assets/logo-64.png");
render(32, "src-tauri/icons/tray.png");
// Paused tray icon: the same mark in gray.
const gray = svg
  .replace(/<svg([^>]*)>/, '<svg$1><defs><filter id="paused"><feColorMatrix type="saturate" values="0"/></filter></defs><g filter="url(#paused)" opacity="0.75">')
  .replace("</svg>", "</g></svg>");
render(32, "src-tauri/icons/tray-paused.png", gray);
console.log("icons written");
