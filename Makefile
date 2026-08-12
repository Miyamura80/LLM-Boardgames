# ANSI color codes
GREEN=\033[0;32m
YELLOW=\033[0;33m
RED=\033[0;31m
BLUE=\033[0;34m
RESET=\033[0m

PROJECT_ROOT=.

.DEFAULT_GOAL := help

########################################################
# Help
########################################################

### Help
.PHONY: help
help: ## Show this help message
	@echo "$(BLUE)Available Make Targets$(RESET)"
	@echo ""
	@awk 'BEGIN {FS = ":.*?## "; category=""} \
		/^### / {category = substr($0, 5); next} \
		/^[a-zA-Z_-]+:.*?## / { \
			if (category != last_category) { \
				if (last_category != "") print ""; \
				print "$(GREEN)" category ":$(RESET)"; \
				last_category = category; \
			} \
			printf "  $(YELLOW)%-23s$(RESET) %s\n", $1, $2 \
		}' $(MAKEFILE_LIST)

########################################################
# App (server + CLI)
########################################################

### App
.PHONY: run build build-release dev

run: ## Run the HTTP API server (shbench serve)
	cargo run -p shbench -- serve

build: ## Build the whole workspace (debug)
	cargo build --workspace

build-release: ## Build the whole workspace (release)
	cargo build --workspace --release

dev: ## Run the optional frontend in development mode
	bun run dev

docs: ## Run docs with bun
	@echo "$(GREEN)📚Running docs...$(RESET)"
	@cd docs && bun run dev
	@echo "$(GREEN)✅ Docs run completed.$(RESET)"


########################################################
# Game Reports (self-contained HTML artifacts)
########################################################

### Game Reports
.PHONY: game-report game-report-json game-report-stored catan-game-report codenames-game-report

game-report-json: ## Render a fixed sample GameRecord JSON to a self-contained HTML report (reproducible, no DB). Vars: REC=, OUT=.
	@echo "$(YELLOW)🎲 Rendering report from $(or $(REC),crates/engine/fixtures/sample_game.json)...$(RESET)"
	@cargo run -q -p shbench -- call sh_export_game_report --args '{"record_path":"$(or $(REC),crates/engine/fixtures/sample_game.json)","output_path":"$(or $(OUT),media/game-report.html)"}'
	@echo "$(GREEN)✅ Report written to $(or $(OUT),media/game-report.html)$(RESET)"

game-report: ## Play a game and render a self-contained HTML report (no DB). Vars: OUT=, MODELS=, SEED=. Pass LLM MODELS to get discussion.
	@echo "$(YELLOW)🎲 Playing a game and rendering the report...$(RESET)"
	@cargo run -q -p shbench -- call sh_play_game --args '{"models":$(or $(MODELS),["bot:bayes-history"]),"seed":$(or $(SEED),7),"discussion_rounds":2,"report_path":"$(or $(OUT),media/game-report.html)"}'
	@echo "$(GREEN)✅ Report written to $(or $(OUT),media/game-report.html)$(RESET)"
	@echo "$(YELLOW)ℹ️  Bots don't talk — pass LLM seats for discussion, e.g. MODELS='[\"openai/gpt-4o-mini\"]'$(RESET)"

game-report-stored: ## Export a STORED game (real LLM discussion) to HTML. Requires GAME=<id> and Postgres. Var: OUT=.
	@if [ -z "$(GAME)" ]; then \
		echo "$(RED)Error: GAME=<game_id> required (list with a run's games)$(RESET)"; exit 1; \
	fi
	@echo "$(YELLOW)🎲 Exporting stored game $(GAME)...$(RESET)"
	@cargo run -q -p shbench -- call sh_export_game_report --args '{"game_id":"$(GAME)","output_path":"$(or $(OUT),media/game-report.html)"}'
	@echo "$(GREEN)✅ Report written to $(or $(OUT),media/game-report.html)$(RESET)"

catan-game-report: ## Export a stored (GAME=<id>, needs Postgres) or JSON (REC=<record.json>) Catan game to HTML. Var: OUT=.
	@if [ -z "$(GAME)" ] && [ -z "$(REC)" ]; then \
		echo "$(RED)Error: GAME=<game_id> or REC=<record.json> required$(RESET)"; exit 1; \
	fi
	@echo "$(YELLOW)🎲 Exporting Catan game $(or $(GAME),$(REC))...$(RESET)"
	@cargo run -q -p shbench -- call catan_export_game_report --args '{$(if $(GAME),"game_id":"$(GAME)","record_path":"$(REC)"),"output_path":"$(or $(OUT),media/catan-game-report.html)"}'
	@echo "$(GREEN)✅ Report written to $(or $(OUT),media/catan-game-report.html)$(RESET)"

codenames-game-report: ## Export a stored (GAME=<id>, needs Postgres) or JSON (REC=<record.json>) Codenames game to HTML. Var: OUT=.
	@if [ -z "$(GAME)" ] && [ -z "$(REC)" ]; then \
		echo "$(RED)Error: GAME=<game_id> or REC=<record.json> required$(RESET)"; exit 1; \
	fi
	@echo "$(YELLOW)🕵️  Exporting Codenames game $(or $(GAME),$(REC))...$(RESET)"
	@cargo run -q -p shbench -- call codenames_export_game_report --args '{$(if $(GAME),"game_id":"$(GAME)","record_path":"$(REC)"),"output_path":"$(or $(OUT),media/codenames-game-report.html)"}'
	@echo "$(GREEN)✅ Report written to $(or $(OUT),media/codenames-game-report.html)$(RESET)"


########################################################
# Initialization
########################################################

### Initialization
.PHONY: setup new banner logo

setup: ## Set up dev environment from scratch (installs deps, copies .env, checks tooling)
	@echo "$(BLUE)🔧 Setting up dev environment...$(RESET)"
	@if ! command -v rustup > /dev/null 2>&1; then \
		echo "$(RED)Error: rustup not found. Install from https://rustup.rs$(RESET)"; exit 1; \
	fi
	@rustup show > /dev/null 2>&1
	@echo "$(GREEN)✅ Rust toolchain ready$(RESET)"
	@if ! command -v bun > /dev/null 2>&1; then \
		echo "$(RED)Error: bun not found. Install from https://bun.sh$(RESET)"; exit 1; \
	fi
	@bun install
	@echo "$(GREEN)✅ Node dependencies installed$(RESET)"
	@if [ ! -f .env ]; then \
		cp .env.example .env; \
		echo "$(YELLOW)⚠️  Copied .env.example → .env (fill in API keys before running)$(RESET)"; \
	else \
		echo "$(GREEN)✅ .env already exists$(RESET)"; \
	fi
	@echo "$(GREEN)✅ Setup complete. Run 'make run' to start the server.$(RESET)"

new: ## Scaffold a new engine command (usage: make new name=fetch_url [description="..."])
	@if [ -z "$(name)" ]; then \
		echo "$(RED)Error: 'name' is required$(RESET)"; \
		echo "Usage: make new name=<command_name> [description=\"...\"]"; \
		exit 1; \
	fi
	@cargo run -q -p shbench -- new $(name) $(if $(description),--description "$(description)",)

### Asset Generation
.PHONY: banner logo

banner: ## Generate project banner image (requires APP__GEMINI_API_KEY)
	@echo "$(YELLOW)🔍Generating banner...$(RESET)"
	@cargo run -p assetgen --bin asset-gen -- banner
	@echo "$(GREEN)✅Banner generated at media/banner.png$(RESET)"

logo: ## Generate logo, icons, and favicon (requires APP__GEMINI_API_KEY)
	@echo "$(YELLOW)🔍Generating logo and favicon...$(RESET)"
	@cargo run -p assetgen --bin asset-gen -- logo
	@echo "$(GREEN)✅Logo assets saved to docs/public/$(RESET)"



########################################################
# Run Tests
########################################################

### Testing
test: ## Run Rust tests
	@echo "$(GREEN)🧪Running Rust Tests...$(RESET)"
	cargo test --workspace
	@echo "$(GREEN)✅Rust Tests Passed.$(RESET)"

test_fast: ## Run fast tests (Rust)
	@echo "$(GREEN)🧪Running Fast Rust Tests...$(RESET)"
	cargo test --workspace
	@echo "$(GREEN)✅Fast Rust Tests Passed.$(RESET)"

test_slow: ## Run slow tests (Rust placeholder)
	@echo "$(YELLOW)⚠️ No slow Rust tests defined yet.$(RESET)"

test_nondeterministic: ## Run nondeterministic tests (Rust placeholder)
	@echo "$(YELLOW)⚠️ No nondeterministic Rust tests defined yet.$(RESET)"

test_flaky: ## Repeat fast tests to detect flaky tests
	@echo "$(GREEN)🧪Running Flaky Test Detection (3 runs)...$(RESET)"
	@for i in 1 2 3; do \
		echo "Run $$i..."; \
		cargo test --workspace || exit 1; \
	done
	@echo "$(GREEN)✅Flaky Test Detection Passed.$(RESET)"


########################################################
# Code Quality
########################################################

### Code Quality
.PHONY: fmt lint knip audit link-check file_len_check brand_sync_check ci

fmt: ## Format code with Biome and rustfmt
	@echo "$(YELLOW)✨ Formatting and linting with Biome...$(RESET)"
	bunx @biomejs/biome check --write --unsafe .
	@echo "$(YELLOW)✨ Formatting Rust code...$(RESET)"
	cargo fmt --all
	@echo "$(GREEN)✅ Formatting completed.$(RESET)"

lint: ## Lint code with Biome and Clippy
	@echo "$(YELLOW)🔍 Checking with Biome...$(RESET)"
	bunx @biomejs/biome check .
	@echo "$(YELLOW)🔍 Linting Rust code with Clippy...$(RESET)"
	cargo clippy --workspace --all-targets -- -D warnings
	@echo "$(GREEN)✅ Linting completed.$(RESET)"

knip: ## Find unused files, dependencies, and exports
	@echo "$(YELLOW)🔍 Running Knip...$(RESET)"
	@bun install --force >/dev/null 2>&1 || true
	bun run knip
	@echo "$(GREEN)✅ Knip completed.$(RESET)"

audit: ## Audit dependencies for vulnerabilities
	@echo "$(YELLOW)🔍 Auditing frontend dependencies...$(RESET)"
	bun audit
	@echo "$(YELLOW)🔍 Auditing Rust dependencies...$(RESET)"
	@if command -v cargo-deny > /dev/null 2>&1; then \
		cargo deny check; \
	else \
		echo "$(YELLOW)⚠️ cargo-deny not installed. Skipping Rust audit.$(RESET)"; \
	fi
	@echo "$(GREEN)✅ Audit completed.$(RESET)"

link-check: ## Check for broken links in markdown files
	@echo "$(YELLOW)🔍 Checking links...$(RESET)"
	@if command -v lychee > /dev/null 2>&1; then \
		lychee .; \
	else \
		echo "$(YELLOW)⚠️ lychee not installed. Falling back to docs lint script...$(RESET)"; \
		cd docs && bun run lint:links; \
	fi
	@echo "$(GREEN)✅ Link check completed.$(RESET)"

file_len_check: ## Check TS/RS files don't exceed max line count
	@echo "$(YELLOW)🔍 Checking file lengths...$(RESET)"
	@bun run scripts/check_file_length.ts
	@echo "$(GREEN)✅ File length check completed.$(RESET)"

brand_sync_check: ## Verify the frontend brand tokens match the skill's canonical copy
	@echo "$(YELLOW)🔍 Checking brand token sync...$(RESET)"
	@node scripts/check_brand_sync.mjs
	@echo "$(GREEN)✅ Brand sync check completed.$(RESET)"

ci: fmt lint knip audit link-check test file_len_check brand_sync_check ## Run all CI checks
	@echo "$(GREEN)✅ CI checks completed.$(RESET)"


########################################################
# Release
########################################################

### Release
.PHONY: bump-version
bump-version: ## Bump version across all manifests (usage: make bump-version VERSION=x.y.z)
	@if [ -z "$(VERSION)" ]; then \
		echo "$(RED)Error: VERSION is required$(RESET)"; \
		echo "Usage: make bump-version VERSION=x.y.z"; \
		exit 1; \
	fi
	@for f in crates/cli/Cargo.toml crates/engine/Cargo.toml crates/config/Cargo.toml crates/assetgen/Cargo.toml; do \
		perl -i.bak -0pe 's/^version = "[^"]*"/version = "$(VERSION)"/m' $$f && rm $$f.bak; \
	done
	@jq --arg v "$(VERSION)" '.version = $$v' package.json > /tmp/_package.json && mv /tmp/_package.json package.json
	@cargo update --workspace
	@echo "$(GREEN)✅ Version bumped to $(VERSION) across all crate manifests and package.json$(RESET)"
	@echo "$(YELLOW)Next steps (cargo-dist cuts the release from the tag):$(RESET)"
	@echo "  git add crates/*/Cargo.toml package.json Cargo.lock"
	@echo "  git commit -m '⚙️ bump version to $(VERSION)'"
	@echo "  git tag v$(VERSION)"
	@echo "  git push origin main --tags"
