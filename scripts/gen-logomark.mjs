#!/usr/bin/env node
// Generates the Secret Hitler square logomark in the project BANNER style:
// bold cream Oswald block-caps ("SH") with a deep near-black 3D extrusion on a
// fascist red-orange vignetted field, tilted slightly up to the right — the same
// propaganda-poster treatment as media/banner.png. Oswald is embedded as a
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
const CREAM = "#ede4ce"; // parchment face (banner letter fill)
const INK = "#17110b"; // near-black extrusion / Hitler ink
const THEMES = {
	// Canonical — matches the banner: cream letters on red-orange.
	red: {
		vigCore: "#e0553b", // brighter centre
		vigEdge: "#a8311c", // darker corners
		face: CREAM,
		extrude: INK,
	},
	// Inverse poster for light surfaces: red letters on parchment.
	parchment: {
		vigCore: "#f2ead6",
		vigEdge: "#dcc79c",
		face: "#c33a22",
		extrude: INK,
	},
};

const S = 512;
const CX = 256;

// Stacked-copy 3D extrusion: N offset copies in the shadow colour, deepest
// first, then the face on top. Universally renderable (no CSS text-shadow).
function block(text, cx, y, size, face, extrude) {
	const depth = 46; // extrusion length in px
	const vx = 15; // slight rightward lean
	const vy = 46; // mostly downward
	const attrs = `text-anchor="middle" font-family="Oswald,'Arial Narrow',sans-serif" font-weight="700" font-size="${size}" letter-spacing="2"`;
	let out = "";
	for (let i = depth; i >= 1; i--) {
		const dx = ((vx * i) / depth).toFixed(2);
		const dy = ((vy * i) / depth).toFixed(2);
		out += `<text x="${(cx + +dx).toFixed(2)}" y="${(y + +dy).toFixed(2)}" ${attrs} fill="${extrude}">${text}</text>`;
	}
	out += `<text x="${cx}" y="${y}" ${attrs} fill="${face}">${text}</text>`;
	return out;
}

function svg(t) {
	const size = 278;
	const baseline = 316; // leaves room for the extrusion below
	return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${S} ${S}" width="${S}" height="${S}" role="img" aria-label="Secret Hitler">
  <defs>
    <style>
      @font-face{font-family:"Oswald";font-weight:700;font-style:normal;
        src:url(data:font/woff2;base64,${OSWALD_B64}) format("woff2");}
    </style>
    <radialGradient id="vig" cx="46%" cy="40%" r="72%">
      <stop offset="0%" stop-color="${t.vigCore}"/>
      <stop offset="100%" stop-color="${t.vigEdge}"/>
    </radialGradient>
    <filter id="grain"><feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" stitchTiles="stitch"/>
      <feColorMatrix type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0.5 0"/></filter>
    <clipPath id="tile"><rect x="0" y="0" width="${S}" height="${S}" rx="18"/></clipPath>
  </defs>

  <g clip-path="url(#tile)">
    <rect x="0" y="0" width="${S}" height="${S}" fill="url(#vig)"/>
    <rect x="0" y="0" width="${S}" height="${S}" filter="url(#grain)" opacity="0.05"/>
    <!-- SH block-letters, tilted up to the right like the banner -->
    <g transform="rotate(-5 ${CX} ${CX})">
      ${block("SH", CX, baseline, size, t.face, t.extrude)}
    </g>
  </g>
</svg>
`;
}

for (const [name, t] of Object.entries(THEMES)) {
	writeFileSync(resolve(OUT, `logomark-${name}.svg`), svg(t));
	console.log("wrote", `logomark-${name}.svg`);
}
writeFileSync(resolve(OUT, "logomark.svg"), svg(THEMES.red));
console.log("wrote", "logomark.svg (canonical = banner red)");
