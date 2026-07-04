# Documentation Translation Operations

Automated translation of docs via the
[Jules Translation Sync](../.github/workflows/jules-sync-translations.yml)
GitHub Actions workflow.

## How it works

1. A push to `main` that changes English source docs (`docs/content/**/*.mdx`
   or `meta.json`, excluding `*.<lang>.mdx` and `meta.<lang>.json`) triggers the workflow.
2. The workflow detects the changed English files via `git diff` against the last
   successful sync run (or parent commit if none exists).
3. A single Jules API session is created with a prompt that instructs Jules to
   update (or create) every `.<lang>.mdx` translation for each changed file.
4. Jules opens **one PR** containing all locale updates (`zh`, `es`, `ja`).
5. A job summary is written with the Jules state and PR link.

## Required secrets

| Secret          | Description                                                          |
|-----------------|----------------------------------------------------------------------|
| `JULES_API_KEY` | Google Jules API key. Generate at https://jules.google Settings.     |

Set this in **Settings → Secrets and variables → Actions → New repository secret**.

## Adding or removing a language

Edit the `SUPPORTED_LANGS` env var in `.github/workflows/jules-sync-translations.yml`:

```yaml
env:
  SUPPORTED_LANGS: "zh,es,ja"
```

- **Add** a locale by appending its ISO 639-1 code (e.g. `"zh,es,ja,pt"`).
- **Remove** a locale by deleting it from the list.
- Also update `docs/lib/i18n.ts` to keep the app's language list in sync.

## Manual re-run

The workflow supports `workflow_dispatch`. Navigate to
**Actions → Jules Translation Sync → Run workflow** to trigger it manually
against the latest `main` commit.

## Failure modes and how to retry

| Failure                         | Symptom                                     | Resolution                                                   |
|---------------------------------|---------------------------------------------|--------------------------------------------------------------|
| Jules API key invalid/expired   | Step fails with HTTP 401/403                | Rotate key at https://jules.google, update secret.           |
| Jules source not found          | HTTP 404 on session creation                | Verify Jules GitHub App is installed; check source name.     |
| Jules session fails             | Poll step exits with FAILED state           | Check Jules session UI in logs; fix issue and re-run.        |
| Poll timeout                    | Workflow times out after ~30 min            | Check Jules UI; re-run workflow via `workflow_dispatch`.      |
| Transient 5xx errors            | Warnings in logs, then success              | Built-in retry with exponential back-off handles these.      |
| No English changes detected     | Workflow exits early with "nothing to do"   | Expected behaviour — only translation files changed.         |

## Verifying the Jules source identifier

```bash
curl -H "x-goog-api-key: $JULES_API_KEY" \
  https://jules.googleapis.com/v1alpha/sources
```

The source name used by this workflow is `sources/github/<owner>/<repo>`.

## Checking the translation glossary

The Jules prompt instructs Jules to read `docs/translation-guide.md` before
starting. Edit that file to:

- Add terms that must never be translated.
- Update the list of supported locales (informational — the workflow uses `SUPPORTED_LANGS`).
- Refine translation PR rules.
