# Translation Guide

Authoritative rules for the automated Jules translation workflow.
Jules **must** read this file before translating any docs.

## Glossary — NEVER translate these terms

### Product & brand names

- Tauri Template
- Tauri
- React
- TypeScript
- Vite
- Rust
- Bun
- MCP (Model Context Protocol)
- Fumadocs
- Biome
- Cargo

### Security & feature terms

- ACL / Access Control Level
- API, REST, GraphQL
- SSO / SCIM, webhook
- JWT, OAuth

### Technical identifiers (never translate)

- CLI commands, flags, and arguments (e.g. `bun install`, `cargo test`, `--verbose`)
- API endpoints and HTTP methods
- Environment variable names (e.g. `JULES_API_KEY`, `TAURI_PRIVATE_KEY`)
- File paths and URLs
- Code snippets and fenced code blocks
- Configuration keys (JSON / YAML / TOML keys)
- CSS class names and hex colour codes
- Rust crate names, TypeScript package names

## File naming convention

| Type             | Pattern                           | Example                                      |
|------------------|-----------------------------------|----------------------------------------------|
| English (source) | `<name>.mdx`                      | `docs/content/docs/index.mdx`                |
| Translation      | `<name>.<lang>.mdx`               | `docs/content/docs/index.ja.mdx`             |
| Section metadata | `meta.json` / `meta.<lang>.json`  | `docs/content/docs/meta.ja.json`             |

## Supported locales

| Code | Language            |
|------|---------------------|
| `zh` | Chinese (Simplified)|
| `es` | Spanish             |
| `ja` | Japanese            |

## Translation PR rules

1. Read this file fully before starting any translation work.
2. Preserve all Markdown / MDX structure: headings, tables, code blocks, admonitions, frontmatter, and JSX components.
3. Preserve anchor IDs, link targets, and `href` values verbatim.
4. If content was removed in English, remove it from every translation.
5. If a translation file is missing, create it from scratch by translating the English source.
6. Never modify English source files (`.mdx` without a language suffix).
7. Never modify any files outside `docs/content/`.
8. For `meta.json` files, create/update `meta.<lang>.json`. Translate `"title"` values but keep `"pages"` array values unchanged.
9. Open a single PR containing all locale updates — do not open one PR per locale.
10. The PR title must follow the pattern: `docs(i18n): sync translations for updated English docs`
