#!/usr/bin/env python3
"""Render the Gemini game bundle into a self-contained observability HTML page."""
import json, html, sys

BUNDLE = sys.argv[1] if len(sys.argv) > 1 else "eval-output/gemini-game.json"
OUT = sys.argv[2] if len(sys.argv) > 2 else "eval-output/observability.html"

d = json.load(open(BUNDLE))
rec = d["records"][0]
seats = rec["seats"]
entries = rec["log"]["entries"]
pub = [e for e in entries if e["private_to"] is None]

ROLE_LABEL = {"liberal": "Liberal", "fascist": "Fascist", "hitler": "Hitler"}

def esc(s): return html.escape(str(s))

# ---- summary stats ----
counts = {}
for e in pub:
    counts[e["event"]["kind"]] = counts.get(e["event"]["kind"], 0) + 1
board_final = None
for e in reversed(pub):
    k = e["event"]["kind"]
    if k in ("policy_enacted", "chaos_policy_enacted"):
        board_final = (e["event"]["liberal"], e["event"]["fascist"]); break
if board_final is None: board_final = (0, 0)
rel = rec["reliability"].values()
malformed = sum(v["malformed_outputs"] for v in rel)
illegal = sum(v["illegal_moves"] for v in rel)
forced = sum(v["forced_defaults"] for v in rel)
tokens = rec["usage"]["prompt_tokens"] + rec["usage"]["completion_tokens"]
calls = rec["usage"]["calls"]
powers = counts.get("power_granted", 0)
govs = counts.get("government_elected", 0)
utter = counts.get("utterance", 0)

WIN_REASON = {
    "liberal_policies": "5 Liberal policies enacted",
    "fascist_policies": "6 Fascist policies enacted",
    "hitler_executed": "Hitler executed",
    "hitler_chancellor": "Hitler elected Chancellor (≥3 Fascist policies)",
}
winner = rec["winner"]
reason = WIN_REASON.get(rec["win_reason"], rec["win_reason"])

# ---- suspicion heatmap: last checkpoint ----
beliefs = rec["beliefs"]
heat_html = "<p class='muted'>No beliefs elicited.</p>"
if beliefs:
    last = max(b["checkpoint"] for b in beliefs)
    snaps = {b["seat"]: b["beliefs"]["fascist_prob"] for b in beliefs if b["checkpoint"] == last}
    order = [s["seat"] for s in seats]
    def is_fasc(seat): return seats[seat]["role"] != "liberal"
    def heat(p):
        r = round(59 + (229 - 59) * p); g = round(130 + (72 - 130) * p); b = round(246 + (77 - 246) * p)
        return f"rgb({r},{g},{b})"
    head = "".join(f"<th>{t}{'★' if is_fasc(t) else ''}</th>" for t in order)
    rows = ""
    for obs in order:
        probs = snaps.get(obs, {})
        cells = ""
        for tgt in order:
            if obs == tgt:
                cells += "<td class='diag'></td>"; continue
            p = probs.get(str(tgt))
            if p is None:
                cells += "<td class='muted'>—</td>"
            else:
                cells += f"<td style='background:{heat(p)};color:#fff'>{p:.2f}</td>"
        rlabel = ROLE_LABEL[seats[obs]["role"]][0]
        rows += f"<tr><th>{obs} <span class='rl'>{rlabel}</span></th>{cells}</tr>"
    heat_html = f"""<div class="scroll"><table class="heat">
<thead><tr><th>obs&nbsp;\\&nbsp;tgt</th>{head}</tr></thead><tbody>{rows}</tbody></table></div>"""

# ---- leaderboard + metrics ----
lb = d["leaderboard"]["models"]
metrics = {(m["model"], m["role"]): m for m in d["metrics"]}
def pctv(x): return "—" if x is None else f"{x*100:.0f}%"
def numv(x, n=2): return "—" if x is None else f"{x:.{n}f}"
lb_rows = ""
for m in lb:
    byrole = {r["role"]: r for r in m["roles"]}
    def cell(role):
        r = byrole.get(role)
        return f"{r['mu']:.1f} ± {r['sigma']:.1f}" if r else "—"
    lb_rows += f"""<tr><td class="mono">{esc(m['model'])}</td>
      <td class="strong">{m['overall']:.1f}</td>
      <td>{cell('liberal')}</td><td>{cell('fascist')}</td><td>{cell('hitler')}</td>
      <td>{pctv(m['liberal_win_rate'])}</td><td>{pctv(m['fascist_win_rate'])}</td></tr>"""
met_rows = ""
for m in lb:
    for role in ("liberal", "fascist", "hitler"):
        mm = metrics.get((m["model"], role))
        if not mm or mm["games"] == 0: continue
        met_rows += f"""<tr><td>{role}</td><td>{mm['games']}</td>
          <td>{pctv(mm['goal_aligned_enactment'])}</td><td>{pctv(mm['faction_throughput'])}</td>
          <td>{pctv(mm['execution_accuracy'])}</td><td>{numv(mm['suspicion_brier'])}</td></tr>"""

# ---- seat roster ----
roster = ""
for s in seats:
    role = s["role"]
    roster += f"<span class='seat seat-{role}'><b>#{s['seat']}</b> {ROLE_LABEL[role]}</span>"

# ---- event timeline ----
def fmt_event(ev):
    k = ev["kind"]
    if k == "presidency_began":
        sp = " · special election" if ev["special_election"] else ""
        return "gov", f"President is seat {ev['president']}{sp}"
    if k == "chancellor_nominated":
        return "gov", f"President {ev['president']} nominates seat {ev['nominee']} as Chancellor"
    if k == "utterance":
        return "talk", (ev["seat"], ev["text"])
    if k == "votes_cast":
        v = " ".join(f"{s}:{'ja' if val else 'nein'}" for s, val in ev["votes"])
        res = "PASSED" if ev["passed"] else "failed"
        return ("pass" if ev["passed"] else "fail"), f"Vote {res} ({ev['ja']}/{ev['needed']}) — {v}"
    if k == "government_elected":
        return "pass", f"Government elected: President {ev['president']}, Chancellor {ev['chancellor']}"
    if k == "election_failed":
        return "fail", f"Election failed — tracker {ev['tracker']}/3"
    if k == "policy_enacted":
        return ev["policy"], f"{ROLE_LABEL[ev['policy']]} policy enacted → {ev['liberal']}L / {ev['fascist']}F"
    if k == "chaos_policy_enacted":
        return ev["policy"], f"CHAOS top-deck: {ROLE_LABEL[ev['policy']]} policy → {ev['liberal']}L / {ev['fascist']}F"
    if k == "power_granted":
        return "power", f"President {ev['president']} gains power: {ev['power']}"
    if k == "loyalty_investigated":
        return "power", f"President {ev['president']} investigates seat {ev['target']} (result private)"
    if k == "special_election_called":
        return "power", f"President {ev['president']} appoints seat {ev['appointed']} as next President"
    if k == "player_executed":
        return "power", f"President {ev['president']} executes seat {ev['target']}"
    if k == "veto_proposed":
        return "gov", f"Chancellor {ev['chancellor']} proposes a veto"
    if k == "veto_resolved":
        return "gov", f"President {ev['president']} {'accepts' if ev['consented'] else 'rejects'} the veto"
    if k == "forced_default":
        return "muted", f"Seat {ev['seat']} timed out → forced default ({ev['decision']})"
    if k == "game_over":
        return "over", f"GAME OVER — {ev['winner'].title()} win"
    if k == "game_started":
        return None, None
    return "muted", k

timeline = ""
for e in pub:
    tone, val = fmt_event(e["event"])
    if tone is None: continue
    if tone == "talk":
        seat, text = val
        role = seats[seat]["role"]
        timeline += f"""<div class="row talk"><span class="who seat-{role}">#{seat}</span>
          <span class="say">{esc(text)}</span></div>"""
    else:
        timeline += f"""<div class="row t-{tone}"><span class="ev">{esc(val)}</span></div>"""

# ---- template ----
tpl = f"""<div class="wrap">
<header class="hero">
  <div class="eyebrow">Secret Hitler Evals · live-game observability</div>
  <h1>One 7-player game, fully instrumented</h1>
  <p class="sub">Model <code>{esc(seats[0]['agent'])}</code> in every seat · engine-authoritative state · no LLM judge</p>
  <div class="verdict verdict-{winner}">
    <span class="flag"></span>
    <div><b>{winner.title()} victory</b><span>{esc(reason)}</span></div>
  </div>
</header>

<section class="tiles">
  <div class="tile"><span class="k">Governments</span><span class="v">{govs}</span><span class="s">{counts.get('election_failed',0)} failed</span></div>
  <div class="tile"><span class="k">Final board</span><span class="v">{board_final[0]}L · {board_final[1]}F</span><span class="s">{counts.get('policy_enacted',0)} enacted</span></div>
  <div class="tile"><span class="k">Powers fired</span><span class="v">{powers}</span><span class="s">investigate · special · execute</span></div>
  <div class="tile"><span class="k">Discussion</span><span class="v">{utter}</span><span class="s">utterances</span></div>
  <div class="tile"><span class="k">LLM cost</span><span class="v">{tokens:,}</span><span class="s">{calls} calls</span></div>
  <div class="tile"><span class="k">Reliability</span><span class="v">{malformed}·{illegal}·{forced}</span><span class="s">malformed · illegal · forced</span></div>
</section>

<div class="roster-row">{roster}<span class="hint">roles revealed post-hoc from engine ground truth</span></div>

<div class="grid">
  <section class="panel timeline-panel">
    <h2>Conversation &amp; event log</h2>
    <p class="muted">Full public transcript — simultaneous-reveal discussion in <span class="chip talk">purple</span>, engine events inline.</p>
    <div class="timeline">{timeline}</div>
  </section>

  <div class="side">
    <section class="panel">
      <h2>Suspicion heatmap</h2>
      <p class="muted">Final private belief P(target is Fascist) by observer. Red = suspects, blue = trusts. ★ = true Fascist/Hitler. Rows scored by Brier vs ground truth.</p>
      {heat_html}
    </section>

    <section class="panel">
      <h2>Rating &amp; objective metrics</h2>
      <p class="muted">Weng-Lin, rated unit = model × role (μ ± σ). Self-play here, so faction win-rates are degenerate — the transcript, heatmap, and per-decision metrics are the signal.</p>
      <div class="scroll"><table>
        <thead><tr><th>Model</th><th>Overall</th><th>Lib μ±σ</th><th>Fasc μ±σ</th><th>Hitler μ±σ</th><th>Lib WR</th><th>Fasc WR</th></tr></thead>
        <tbody>{lb_rows}</tbody></table></div>
      <div class="scroll" style="margin-top:.8rem"><table>
        <thead><tr><th>Role</th><th>Games</th><th>Goal-aligned enact</th><th>Throughput</th><th>Exec acc</th><th>Suspicion Brier↓</th></tr></thead>
        <tbody>{met_rows}</tbody></table></div>
    </section>
  </div>
</div>
<footer>Generated from the persisted <code>GameRecord</code> · seed {rec['seed']} · every number derives from engine state or declared private beliefs.</footer>
</div>"""

CSS = """
:root{
  --bg:#f4f5f8; --panel:#ffffff; --ink:#161a22; --muted:#5b6472; --line:#e2e6ee;
  --lib:#2f6fed; --fasc:#e0484d; --hitler:#1c2029; --gold:#c8912a;
  --pass:#1f9d63; --fail:#d98324; --power:#a855c7;
  --neutral:#eef1f6; --accent:var(--lib);
}
@media (prefers-color-scheme:dark){
  :root{ --bg:#0d0f14; --panel:#151922; --ink:#e7ebf3; --muted:#939db0; --line:#232a37;
    --lib:#5b8cff; --fasc:#ff6167; --hitler:#0a0c11; --neutral:#1a2029; }
}
:root[data-theme="light"]{ --bg:#f4f5f8; --panel:#fff; --ink:#161a22; --muted:#5b6472; --line:#e2e6ee; --lib:#2f6fed; --fasc:#e0484d; --neutral:#eef1f6; }
:root[data-theme="dark"]{ --bg:#0d0f14; --panel:#151922; --ink:#e7ebf3; --muted:#939db0; --line:#232a37; --lib:#5b8cff; --fasc:#ff6167; --neutral:#1a2029; }
*{box-sizing:border-box}
body{margin:0}
.wrap{max-width:1180px;margin:0 auto;padding:2rem 1.25rem 4rem;
  font-family:system-ui,-apple-system,"Segoe UI",sans-serif;color:var(--ink);background:var(--bg);
  -webkit-font-smoothing:antialiased}
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.9em;
  background:var(--neutral);padding:.08em .35em;border-radius:4px}
.hero{padding:.5rem 0 1.5rem;border-bottom:1px solid var(--line);margin-bottom:1.5rem}
.eyebrow{text-transform:uppercase;letter-spacing:.14em;font-size:.72rem;font-weight:700;color:var(--muted)}
.hero h1{font-size:clamp(1.7rem,4vw,2.5rem);margin:.35rem 0 .3rem;letter-spacing:-.02em;text-wrap:balance}
.sub{color:var(--muted);margin:0 0 1.1rem}
.verdict{display:inline-flex;align-items:center;gap:.8rem;padding:.7rem 1.1rem;border-radius:10px;
  border:1px solid var(--line);background:var(--panel)}
.verdict .flag{width:12px;height:34px;border-radius:3px;display:block}
.verdict-fascist .flag{background:var(--fasc)} .verdict-liberal .flag{background:var(--lib)}
.verdict b{display:block;font-size:1.05rem} .verdict span{color:var(--muted);font-size:.9rem}
.tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:.7rem;margin-bottom:1.1rem}
.tile{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:.75rem .85rem;display:flex;flex-direction:column;gap:.15rem}
.tile .k{font-size:.72rem;text-transform:uppercase;letter-spacing:.08em;color:var(--muted)}
.tile .v{font-size:1.5rem;font-weight:700;font-variant-numeric:tabular-nums;letter-spacing:-.01em}
.tile .s{font-size:.72rem;color:var(--muted)}
.roster-row{display:flex;flex-wrap:wrap;align-items:center;gap:.4rem;margin-bottom:1.4rem}
.seat{border:1.5px solid var(--line);border-radius:999px;padding:.15rem .7rem;font-size:.82rem;font-variant-numeric:tabular-nums}
.seat b{font-family:ui-monospace,monospace}
.seat-liberal{border-color:var(--lib)} .seat-fascist{border-color:var(--fasc)} .seat-hitler{border-color:var(--gold)}
.hint{color:var(--muted);font-size:.75rem;margin-left:.3rem}
.grid{display:grid;grid-template-columns:1fr;gap:1.1rem}
@media(min-width:940px){.grid{grid-template-columns:1.25fr .95fr;align-items:start}}
.panel{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:1.1rem 1.2rem}
.panel h2{font-size:1.05rem;margin:0 0 .3rem;letter-spacing:-.01em}
.muted{color:var(--muted);font-size:.85rem;margin:.2rem 0 .8rem}
.side{display:flex;flex-direction:column;gap:1.1rem}
.timeline{max-height:none;display:flex;flex-direction:column;gap:.3rem}
@media(min-width:940px){.timeline{max-height:1400px;overflow-y:auto;padding-right:.3rem}}
.row{font-size:.86rem;line-height:1.4;padding:.28rem .5rem;border-left:3px solid var(--line);border-radius:0 4px 4px 0}
.row .ev{color:var(--ink)}
.talk{display:flex;gap:.55rem;align-items:baseline;border-left-color:var(--power);background:color-mix(in srgb,var(--power) 8%,transparent)}
.talk .who{font-family:ui-monospace,monospace;font-weight:700;font-size:.8rem;padding:.02em .35em;border-radius:4px;border:1.5px solid var(--line);flex:none}
.talk .who.seat-liberal{border-color:var(--lib);color:var(--lib)}
.talk .who.seat-fascist{border-color:var(--fasc);color:var(--fasc)}
.talk .who.seat-hitler{border-color:var(--gold);color:var(--gold)}
.talk .say{color:var(--ink)}
.t-pass{border-left-color:var(--pass)} .t-fail{border-left-color:var(--fail)}
.t-fascist{border-left-color:var(--fasc);background:color-mix(in srgb,var(--fasc) 8%,transparent);font-weight:600}
.t-liberal{border-left-color:var(--lib);background:color-mix(in srgb,var(--lib) 8%,transparent);font-weight:600}
.t-power{border-left-color:var(--power);font-weight:600}
.t-over{border-left-color:var(--ink);background:var(--neutral);font-weight:700}
.t-gov{border-left-color:var(--line)} .t-muted{border-left-color:var(--line);color:var(--muted)}
.chip{padding:.02em .4em;border-radius:4px;font-size:.8em}
.chip.talk{background:color-mix(in srgb,var(--power) 22%,transparent);color:var(--ink);border:0}
.scroll{overflow-x:auto}
table{border-collapse:collapse;width:100%;font-size:.82rem}
th,td{padding:.32rem .55rem;text-align:left;border-bottom:1px solid var(--line);white-space:nowrap;font-variant-numeric:tabular-nums}
th{color:var(--muted);font-weight:600;font-size:.76rem;text-transform:uppercase;letter-spacing:.04em}
td.strong{font-weight:700} td.mono{font-family:ui-monospace,monospace;font-size:.76rem}
.heat td,.heat th{text-align:center;min-width:2.5rem}
.heat .diag{background:var(--neutral)} .heat .rl{color:var(--muted);font-size:.75em}
footer{margin-top:2rem;padding-top:1rem;border-top:1px solid var(--line);color:var(--muted);font-size:.8rem}
"""

doc = f"<style>{CSS}</style>\n{tpl}\n"
open(OUT, "w").write(doc)
print("wrote", OUT, len(doc), "bytes")
