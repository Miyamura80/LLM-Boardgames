# eval-output

Sample end-to-end evidence from one **live** 7-player game, self-play by
`gemini/gemini-3-flash-preview` (rounds=1 discussion, private beliefs on):

- `gemini-game.json` — the persisted `MatchOutcome` bundle (transcript with the
  full conversation log, per-player beliefs, reliability counters, rating, and
  the objective metric suite). Produced by `shbench eval … --out`.
- `observability.html` — a self-contained observability page rendered from that
  bundle by `scripts/render_observability.py`.

Regenerate the page:

```bash
python3 scripts/render_observability.py eval-output/gemini-game.json eval-output/observability.html
```

Run a fresh game (needs `APP__GEMINI_API_KEY` and a reachable `DATABASE_URL`):

```bash
shbench eval --self-play gemini/gemini-3-flash-preview --games 1 --rounds 1 \
  --reasoning low --out eval-output/gemini-game.json
```
