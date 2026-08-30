import { readFile, readdir, stat } from "node:fs/promises";
import { basename } from "node:path";

const dist = new URL("../dist/", import.meta.url);
const assets = new URL("assets/", dist);

const names = await readdir(assets);
const files = await Promise.all(
  names.map(async (name) => ({ name, size: (await stat(new URL(name, assets))).size })),
);
const html = await readFile(new URL("index.html", dist), "utf8");
const entryName = html.match(/<script[^>]+src="\.\/assets\/(index-[^"]+\.js)"/)?.[1]
  ?? html.match(/<script[^>]+src="\/assets\/(index-[^"]+\.js)"/)?.[1];
if (!entryName) throw new Error("Could not resolve the initial operator entry chunk");

const entry = files.find((file) => file.name === entryName);
if (!entry) throw new Error(`Initial operator entry is missing: ${entryName}`);
if (entry.size > 112 * 1024) throw new Error(`Initial operator JavaScript exceeded 112 KiB: ${entry.size} bytes`);

const initialCss = files.find((file) => /^index-[^.]+\.css$/.test(file.name));
if (!initialCss) throw new Error("Initial operator stylesheet is missing");
if (initialCss.size > 40 * 1024) throw new Error(`Initial operator CSS exceeded 40 KiB: ${initialCss.size} bytes`);

if (/perspective-(?:server|viewer|js).*\.wasm/.test(html)) {
  throw new Error("Operator HTML eagerly references Perspective WebAssembly");
}
const entryCode = await readFile(new URL(entry.name, assets), "utf8");
if (entryCode.includes("perspective-viewer-datagrid")) {
  throw new Error("Initial operator JavaScript contains the Perspective datagrid implementation");
}

const perspective = files.filter((file) => !file.name.endsWith(".map") && /perspective|pro-dark/i.test(file.name));
const perspectiveBytes = perspective.reduce((total, file) => total + file.size, 0);
if (perspectiveBytes > 5 * 1024 * 1024) {
  throw new Error(`On-demand Perspective payload exceeded 5 MiB: ${perspectiveBytes} bytes`);
}

console.log(`operator_initial_js=${basename(entry.name)} bytes=${entry.size}`);
console.log(`operator_initial_css=${basename(initialCss.name)} bytes=${initialCss.size}`);
console.log(`perspective_on_demand_assets=${perspective.length} bytes=${perspectiveBytes}`);
console.log("operator_performance_contract=pass");
