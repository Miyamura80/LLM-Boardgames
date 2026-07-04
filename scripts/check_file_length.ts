import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative, resolve } from "node:path";

const REPO_ROOT = resolve(import.meta.dirname, "..");

const SKIP_DIRS = new Set([
	".git",
	"node_modules",
	".next",
	"target",
	"dist",
	".venv",
	".cache",
]);

const EXTENSIONS = new Set([".ts", ".tsx", ".rs"]);

interface Config {
	max_lines: number;
	exclude: string[];
}

function loadConfig(): Config {
	const pkg = JSON.parse(
		readFileSync(join(REPO_ROOT, "package.json"), "utf-8"),
	);
	const cfg = pkg.fileLength ?? {};
	return {
		max_lines: cfg.max_lines ?? 500,
		exclude: cfg.exclude ?? [],
	};
}

function walk(dir: string): string[] {
	const results: string[] = [];
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		if (entry.isDirectory()) {
			if (!SKIP_DIRS.has(entry.name)) {
				results.push(...walk(join(dir, entry.name)));
			}
		} else if (EXTENSIONS.has(extname(entry.name))) {
			results.push(join(dir, entry.name));
		}
	}
	return results;
}

function main(): number {
	const config = loadConfig();
	const excludeSet = new Set(config.exclude);
	const violations: [string, number][] = [];

	for (const file of walk(REPO_ROOT)) {
		const rel = relative(REPO_ROOT, file).replace(/\\/g, "/");
		if (excludeSet.has(rel)) continue;

		const content = readFileSync(file, "utf-8");
		const lineCount = content.trimEnd().split("\n").length;
		if (lineCount > config.max_lines) {
			violations.push([rel, lineCount]);
		}
	}

	if (violations.length > 0) {
		violations.sort((a, b) => a[0].localeCompare(b[0]));
		console.error(
			`File length check failed: ${violations.length} file(s) exceed ${config.max_lines} lines`,
		);
		for (const [path, count] of violations) {
			console.error(`  ${path}: ${count} lines`);
		}
		console.error(
			'Refactor large files into smaller modules, or add to "fileLength.exclude" in package.json.',
		);
		return 1;
	}

	console.log(
		`File length check passed (all files <= ${config.max_lines} lines).`,
	);
	return 0;
}

process.exit(main());
