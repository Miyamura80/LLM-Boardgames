// Transform a self-contained `game_report.html` into Artifact page-content.
//
// The game report is a full HTML document, but Claude's Artifact tool supplies
// its own <!doctype>/<head>/<body> skeleton and wants page content only. This
// strips the outer document, keeping <title> + <style> + the <body> inner
// markup (including the inline <script>), which is exactly what the Artifact
// wrapper expects. The report is otherwise self-contained (no external assets),
// so it renders identically once wrapped.
//
// Usage: node scripts/report_to_artifact.mjs <in.html> <out.html>

import { readFileSync, writeFileSync } from "node:fs";

const [, , inPath, outPath] = process.argv;
if (!inPath || !outPath) {
	console.error(
		"usage: node scripts/report_to_artifact.mjs <in.html> <out.html>",
	);
	process.exit(1);
}

const html = readFileSync(inPath, "utf8");
const title =
	(html.match(/<title>([\s\S]*?)<\/title>/) || [])[1] ||
	"Secret Hitler — Game Report";
const style = (html.match(/<style[^>]*>[\s\S]*?<\/style>/) || [])[0] || "";
const body = (html.match(/<body[^>]*>([\s\S]*?)<\/body>/) || [])[1] || "";
// Fail loudly rather than silently ship a broken/unstyled artifact — the
// template and this extractor are maintained separately and can drift.
if (!body) {
	console.error(`no <body> found in ${inPath} — is it a game report?`);
	process.exit(1);
}
if (!style) {
	console.error(
		`no <style> found in ${inPath} — the artifact would be unstyled`,
	);
	process.exit(1);
}

writeFileSync(outPath, `<title>${title}</title>\n${style}\n${body}`);
console.log(
	`wrote ${outPath} (${(readFileSync(outPath).length / 1024).toFixed(0)} KB)`,
);
