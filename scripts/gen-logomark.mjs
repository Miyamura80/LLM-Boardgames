#!/usr/bin/env node
// Generates the Secret Hitler square logomark: an Art-Deco "SH" monogram,
// split by faction color (liberal-teal S / fascist-red H) on an ink tile with
// a gold double-rule frame. Uses the brand display face (Oswald), embedded as a
// data-URI so the SVG is self-contained; PNGs are the guaranteed rasters.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(HERE, "../assets/logomark");
mkdirSync(OUT, { recursive: true });

const OSWALD_B64 = readFileSync(
	resolve(
		HERE,
		"../.claude/skills/secret-hitler-brand/assets/fonts/oswald-latin.woff2",
	),
).toString("base64");

// ── Brand tokens (from .claude/skills/secret-hitler-brand/assets/…css) ────────
const THEMES = {
	noir: {
		bg: "#16120c",
		bgCore: "#241a10",
		ray: "#20170e", // very subtle sunburst texture
		frame: "#d8ae4e",
		liberal: "#4fa9bc",
		fascist: "#e0553b",
		rule: "#d8ae4e",
	},
	parchment: {
		bg: "#ece0c6",
		bgCore: "#f6eeda",
		ray: "#e4d5b0",
		frame: "#c79a3e",
		liberal: "#2e7c8c",
		fascist: "#c33a22",
		rule: "#c79a3e",
	},
};

const S = 512;
const CX = 256;

function sunburst(cx, cy, r, fill) {
	const spokes = 16;
	const slice = (Math.PI * 2) / (spokes * 2);
	let d = "";
	for (let i = 0; i < spokes; i++) {
		const a0 = i * 2 * slice - Math.PI / 2;
		const a1 = a0 + slice;
		const p = (a) =>
			`${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`;
		d += `M${cx} ${cy} L${p(a0)} L${p(a1)} Z `;
	}
	return `<path d="${d.trim()}" fill="${fill}"/>`;
}

function svg(t) {
	// Monogram: two Oswald glyphs, condensed & bold, baseline at y=356.
	const baseline = 356;
	const size = 312; // ~cap height 205 on the 512 grid
	const sx = 180; // S centre
	const hx = 334; // H centre
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${S} ${S}" width="${S}" height="${S}" role="img" aria-label="Secret Hitler">
  <defs>
    <style>
      @font-face{font-family:"Oswald";font-weight:700;font-style:normal;
        src:url(data:font/woff2;base64,${OSWALD_B64}) format("woff2");}
      .mono{font-family:"Oswald","Arial Narrow",sans-serif;font-weight:700;
        font-size:${size}px;text-anchor:middle;}
    </style>
    <radialGradient id="vig" cx="50%" cy="46%" r="64%">
      <stop offset="0%" stop-color="${t.bgCore}"/>
      <stop offset="100%" stop-color="${t.bg}"/>
    </radialGradient>
    <clipPath id="field"><rect x="40" y="40" width="432" height="432" rx="7"/></clipPath>
  </defs>

  <rect x="0" y="0" width="${S}" height="${S}" rx="18" fill="${t.bg}"/>
  <g clip-path="url(#field)">
    <rect x="40" y="40" width="432" height="432" fill="url(#vig)"/>
    ${sunburst(CX, 232, 300, t.ray)}
  </g>

  <!-- Art-Deco double-rule frame + corner ticks -->
  <rect x="26" y="26" width="460" height="460" rx="10" fill="none" stroke="${t.frame}" stroke-width="4"/>
  <rect x="40" y="40" width="432" height="432" rx="7" fill="none" stroke="${t.frame}" stroke-width="1.5" opacity="0.6"/>
  ${[
		[40, 40],
		[472, 40],
		[40, 472],
		[472, 472],
	]
		.map(
			([x, y]) =>
				`<rect x="${x - 5}" y="${y - 5}" width="10" height="10" fill="${t.frame}" transform="rotate(45 ${x} ${y})"/>`,
		)
		.join("\n  ")}

  <!-- SH monogram: liberal-teal S · fascist-red H -->
  <text class="mono" x="${sx}" y="${baseline}" fill="${t.liberal}">S</text>
  <text class="mono" x="${hx}" y="${baseline}" fill="${t.fascist}">H</text>

  <!-- Art-Deco baseline bar -->
  <rect x="${CX - 96}" y="386" width="192" height="7" rx="1.5" fill="${t.rule}"/>
  <rect x="${CX - 96}" y="398" width="192" height="2" rx="1" fill="${t.rule}" opacity="0.55"/>
</svg>
`;
}

for (const [name, t] of Object.entries(THEMES)) {
	writeFileSync(resolve(OUT, `logomark-${name}.svg`), svg(t));
	console.log("wrote", `logomark-${name}.svg`);
}
writeFileSync(resolve(OUT, "logomark.svg"), svg(THEMES.noir));
console.log("wrote", "logomark.svg (canonical = Noir)");
