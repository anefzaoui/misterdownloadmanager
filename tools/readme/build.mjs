// The README's figures, as SVG in a light and a dark version (GitHub picks one with <picture>),
// and its row of language flags (from i18n/languages.tsv):
//   docs/images/races-{light,dark}.svg       three everyday races, MDM vs IDM, in seconds
//   docs/images/bench-time-{light,dark}.svg  time to finish, IDM vs MDM, every scenario
// Every number comes from docs/bench/run5/results.json and i18n/languages.tsv; nothing is typed
// in by hand. The races are sketched with rough.js (fixed seeds: the same drawing every build) and
// their words are Patrick Hand (SIL OFL) turned into outlines by opentype.js, since GitHub's
// image sandbox blocks fonts inside SVGs. The detailed chart is Vega-Lite. No browser needed.
//
//   cd tools/readme && npm install && node build.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import opentype from "opentype.js";
import rough from "roughjs";
import * as vega from "vega";
import { compile } from "vega-lite";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const hand = opentype.loadSync(
  path.join(here, "node_modules/@fontsource/patrick-hand/files/patrick-hand-latin-400-normal.woff"));
const out = path.join(root, "docs/images");
fs.mkdirSync(out, { recursive: true });

const bench = JSON.parse(fs.readFileSync(path.join(root, "docs/bench/run5/results.json"), "utf8"));
const languages = fs.readFileSync(path.join(root, "i18n/languages.tsv"), "utf8")
  .split("\n").filter((l) => l.trim() && !l.startsWith("#")).length;

// GitHub's page surfaces and text, and the categorical slots 1 (blue, MDM) and 2 (orange, IDM),
// validated on these surfaces (dataviz validate_palette.js: all checks pass in both modes).
const THEMES = {
  light: { surface: "#ffffff", tile: "#f6f8fa", border: "#d1d9e0", text: "#1f2328", muted: "#59636e",
           grid: "#d8dee4", mdm: "#2a78d6", idm: "#eb6834" },
  dark: { surface: "#0d1117", tile: "#151b23", border: "#3d444d", text: "#f0f6fc", muted: "#9198a1",
          grid: "#2a313c", mdm: "#3987e5", idm: "#d95926" },
};
const FONT = "-apple-system, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif";

const SHORT = {
  "fast-1g": "1 GiB, fast link",
  "capped-100m": "100 MiB, 2 MB/s per connection",
  "latency-100m": "100 MiB, 150 ms latency",
  "slowconn-32m": "32 MiB, one connection crawls",
  "maxconns-50m": "50 MiB, server allows 2 connections",
  "drops-50m": "50 MiB, connections cut midway",
  "small-256k": "256 KiB file",
};
const med = (s, client, key) => s.clients[client].stats[key].median;
const scenarios = bench.scenarios.map((s) => ({
  id: s.id,
  label: SHORT[s.id] ?? s.title,
  idm: med(s, "IDM", "e2e_ms"),
  mdm: med(s, "MDM", "e2e_ms"),
}));
const byId = Object.fromEntries(bench.scenarios.map((s) => [s.id, s]));

function config(t) {
  return {
    background: t.surface,
    font: FONT,
    view: { stroke: null },
    axis: { labelColor: t.muted, titleColor: t.muted, labelFontSize: 13, titleFontSize: 13,
            titleFontWeight: "normal", gridColor: t.grid, gridWidth: 1, domain: false, ticks: false,
            labelPadding: 8 },
    legend: { labelColor: t.text, labelFontSize: 13, labelLimit: 400, symbolType: "square", symbolSize: 140,
              orient: "top", direction: "horizontal", title: null },
    title: { color: t.text, fontSize: 16, fontWeight: 600, anchor: "start", subtitleColor: t.muted,
             subtitleFontSize: 13, offset: 14 },
  };
}

async function render(spec, file) {
  const view = new vega.View(vega.parse(compile(spec).spec), { renderer: "none" });
  const svg = await view.toSVG();
  fs.writeFileSync(path.join(out, file), svg);
}

function timeChart(t) {
  const rows = scenarios.flatMap((s) => [
    { scenario: s.label, client: "Mister Download Manager", seconds: s.mdm / 1000 },
    { scenario: s.label, client: "Internet Download Manager 6.42", seconds: s.idm / 1000 },
  ]);
  return {
    $schema: "https://vega.github.io/schema/vega-lite/v6.json",
    config: config(t),
    width: 560,
    title: { text: "Time to finish a download (lower is better)",
             subtitle: "Median of 7 runs per scenario, seconds, both at their default settings" },
    data: { values: rows },
    encoding: {
      y: { field: "scenario", type: "nominal", sort: scenarios.map((s) => s.label), title: null,
           scale: { paddingInner: 0.35, paddingOuter: 0.2 },
           axis: { grid: false, labelFontSize: 13, labelColor: t.text, labelLimit: 400 } },
      yOffset: { field: "client", sort: ["Mister Download Manager", "Internet Download Manager 6.42"],
                 scale: { paddingInner: 0 } },
      x: { field: "seconds", type: "quantitative", title: null,
           axis: { tickCount: 5, format: "~s", labelExpr: "datum.label + ' s'" } },
    },
    // With yOffset the step is one bar's band: 16 px bars, a 2 px gap, air between scenarios.
    height: { step: 18 },
    layer: [
      { mark: { type: "bar", height: 16, cornerRadiusEnd: 4 },
        encoding: { color: { field: "client", type: "nominal",
                             sort: ["Mister Download Manager", "Internet Download Manager 6.42"],
                             scale: { range: [t.mdm, t.idm] } } } },
      { mark: { type: "text", align: "left", dx: 6, fontSize: 12, color: t.muted },
        transform: [{ calculate: "format(datum.seconds, datum.seconds < 0.1 ? '.2f' : '.1f')", as: "value" }],
        encoding: { text: { field: "value" } } },
    ],
  };
}

// The races: three everyday cases, MDM's and IDM's times as bars in plain seconds, the verdict
// in words (rounded down: "5 times faster" for 5.4).
function esc(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
}
function seconds(ms) {
  const s = ms / 1000;
  if (s < 0.1) return "instantly";
  if (Math.abs(s - 1) < 0.05) return "1 second";
  return `${s.toFixed(1).replace(/\.0$/, "")} seconds`;
}
function verdict(x) {
  if (x >= 100) return "Over 100 times faster";
  if (x >= 2) return `${Math.floor(x)} times faster`;
  return `${Math.round((x - 1) * 100)}% faster`;
}
// Hand-drawn pieces: words as outlines (anchor "start" or "end" at x), and rough.js shapes.
function write(str, x, y, size, color, anchor = "start", bold = false) {
  const w = hand.getAdvanceWidth(str, size);
  const d = hand.getPath(str, anchor === "end" ? x - w : x, y, size).toPathData(2);
  const stroke = bold ? ` stroke="${color}" stroke-width="0.8" stroke-linejoin="round"` : "";
  return `<path d="${d}" fill="${color}"${stroke}/>`;
}
function sketch(drawable) {
  return rough.generator().toPaths(drawable).map((p) =>
    `<path d="${p.d}" stroke="${p.stroke}" stroke-width="${p.strokeWidth}" fill="${p.fill ?? "none"}" stroke-linecap="round" stroke-linejoin="round"/>`).join("");
}

function races(t) {
  const cases = [
    ["fast-1g", "A big file (1 GB)"],
    ["small-256k", "A small file"],
    ["drops-50m", "A shaky connection"],
  ].map(([id, title]) => ({ title, mdm: med(byId[id], "MDM", "e2e_ms"), idm: med(byId[id], "IDM", "e2e_ms") }));
  const gen = rough.generator();
  const W = 880, cols = 3, gap = 18, cw = (W - gap * (cols - 1)) / cols, H = 214;
  const barX = 22, barW = cw - 44, pad = 4;
  const parts = cases.map((c, i) => {
    const x = i * (cw + gap);
    const seed = 7 + i * 13;
    const mdmW = Math.max(10, (barW * c.mdm) / c.idm);
    const verdictText = verdict(c.idm / c.mdm);
    const vw = hand.getAdvanceWidth(verdictText, 30);
    // Two hatch directions as well as two colours: the bars stay apart in grey too.
    const bar = (y, w, color, angle, s) => sketch(gen.rectangle(barX, y, w, 16, {
      seed: s, roughness: 1.3, bowing: 1.5, stroke: color, strokeWidth: 1.6,
      fill: color, fillStyle: "hachure", hachureAngle: angle, hachureGap: 4.5, fillWeight: 1.4 }));
    return `<g transform="translate(${x},0)">
  <rect x="${pad}" y="${pad}" width="${cw - 2 * pad}" height="${H - 2 * pad}" rx="12" fill="${t.tile}"/>
  ${sketch(gen.rectangle(pad, pad, cw - 2 * pad, H - 2 * pad, { seed, roughness: 1.4, bowing: 1.2, stroke: t.border, strokeWidth: 1.8 }))}
  ${write(c.title, 22, 40, 22, t.text, "start", true)}
  ${write("Mister Download Manager", 22, 72, 17, t.text)}
  ${write(seconds(c.mdm), cw - 22, 72, 17, t.text, "end", true)}
  ${bar(80, mdmW, t.mdm, -41, seed + 1)}
  ${write("IDM 6.42", 22, 122, 17, t.muted)}
  ${write(seconds(c.idm), cw - 22, 122, 17, t.muted, "end")}
  ${bar(130, barW, t.idm, 41, seed + 2)}
  ${write(verdictText, 22, 186, 30, t.text, "start", true)}
  ${sketch(gen.line(22, 194, 22 + vw, 194, { seed: seed + 3, roughness: 1.6, bowing: 2, stroke: t.mdm, strokeWidth: 2.4 }))}
</g>`;
  });
  const label = cases.map((c) => `${c.title}: Mister Download Manager ${seconds(c.mdm)}, IDM ${seconds(c.idm)}, ${verdict(c.idm / c.mdm).toLowerCase()}`).join("; ");
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="${esc(label)}">
<title>${esc(label)}</title>
${parts.join("\n")}
</svg>
`;
}

// Replace what is between README.md's <!-- name:start --> and <!-- name:end --> markers.
function fill(name, body) {
  const readme = path.join(root, "README.md");
  const text = fs.readFileSync(readme, "utf8");
  const start = `<!-- ${name}:start -->`, end = `<!-- ${name}:end -->`;
  const a = text.indexOf(start), b = text.indexOf(end);
  if (a < 0 || b < a) throw new Error(`README.md has no ${name} markers`);
  fs.writeFileSync(readme, text.slice(0, a) + `${start}\n${body}\n` + text.slice(b));
}

// The README's comparison table: the measured times in plain words, then the facts.
function compareTable() {
  const row = (label, id, key = "e2e_ms") =>
    `| ${label} | **${seconds(med(byId[id], "MDM", key))}** | ${seconds(med(byId[id], "IDM", key))} |`;
  return [
    "| | Mister Download Manager | IDM 6.42 |",
    "| --- | --- | --- |",
    row("Download a big file (1 GB)", "fast-1g"),
    row("Download a small file", "small-256k"),
    row("Download over a shaky connection", "drops-50m"),
    row("Wait before a download starts", "fast-1g", "startup_ms"),
    "| Price | **Free, forever** | Paid, after a 30-day trial |",
    "| Ads, nag screens, sign-up | **None** | Registration needed |",
    "| Open source | **Yes** (GPL) | No |",
  ].join("\n");
}

// The README's row of languages, between its <!-- languages:start/end --> markers: every row of
// i18n/languages.tsv as its flag, with the language's own and English names on hover.
function languageRow() {
  const lines = fs.readFileSync(path.join(root, "i18n/languages.tsv"), "utf8").split("\n");
  const header = lines.find((l) => l.startsWith("# code\t")).slice(2).split("\t");
  const rows = lines.filter((l) => l.trim() && !l.startsWith("#")).map((l) => {
    const cols = l.split("\t");
    return Object.fromEntries(header.map((h, i) => [h, cols[i]]));
  });
  rows.sort((a, b) => a.english.localeCompare(b.english));
  const attr = (s) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
  const flags = rows.map((r) => {
    const name = r.native === r.english ? r.english : `${r.native} (${r.english})`;
    return `<img src="crates/app/assets/flags/${r.flag}.svg" width="28" height="21" alt="${attr(name)}" title="${attr(name)}">`;
  });
  fill("languages", `<p align="center">\n${flags.join("\n")}\n</p>`);
}

for (const [mode, t] of Object.entries(THEMES)) {
  fs.writeFileSync(path.join(out, `races-${mode}.svg`), races(t));
  await render(timeChart(t), `bench-time-${mode}.svg`);
}
languageRow();
fill("compare", compareTable());
console.log(`wrote README.md's comparison table, language row and docs/images/{races,bench-time}-{light,dark}.svg (${languages} languages)`);
