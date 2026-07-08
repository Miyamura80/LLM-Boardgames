// Guard: the live frontend brand tokens must stay identical to the canonical
// copy shipped by the `secret-hitler-brand` skill. The skill file is the source
// of truth; `frontend/src/brand.css` is a served copy (Vite needs it under the
// app). Keeping them byte-identical is a manual discipline, so enforce it here
// instead of trusting a comment. Re-sync by copying the skill asset over the
// frontend copy:
//   cp .claude/skills/secret-hitler-brand/assets/secret-hitler-brand.css frontend/src/brand.css

import { readFileSync } from "node:fs";

const CANONICAL =
	".claude/skills/secret-hitler-brand/assets/secret-hitler-brand.css";
const COPY = "frontend/src/brand.css";

const canonical = readFileSync(CANONICAL, "utf8");
const copy = readFileSync(COPY, "utf8");

if (canonical !== copy) {
	console.error(
		`Brand tokens have drifted:\n  source: ${CANONICAL}\n  copy:   ${COPY}\n` +
			`Re-sync with:\n  cp ${CANONICAL} ${COPY}`,
	);
	process.exit(1);
}
console.log("✅ brand tokens in sync");
