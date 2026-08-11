// The Codenames replay console: run/game pickers (mirroring the Catan tab), a
// step slider over the record's events, the 5×5 word grid with the spymaster
// key overlay toggle, and the clue-history sidebar.
//
// Two escape hatches keep the tab usable without a populated database: an
// ad-hoc bot game (`codenames_play_game` needs no store) and, when the API is
// unreachable entirely, a bundled demo record so the view can be developed
// against `bun run dev` alone.

import { useCallback, useEffect, useMemo, useState } from "react";
import { describeError } from "../../api/client";
import {
	codenamesListGames,
	codenamesListRuns,
	codenamesPlayGame,
	codenamesReplay,
	type GameRecord,
	type GameSummary,
	type RunSummary,
	type Seat,
} from "../../api/codenames";
import { ClueHistory } from "./ClueHistory";
import demoGame from "./demo-game.json";
import {
	activeClue,
	agentsLeft,
	buildTurns,
	foldGrid,
	justRevealed,
	thoughtsByEvent,
	unknownKeyCards,
} from "./replay";
import { WordGrid } from "./WordGrid";

// A real serialized GameRecord (copy of crates/engine/fixtures/codenames_bots.json).
const DEMO_RECORD = demoGame as unknown as GameRecord;

export function CodenamesReplay() {
	const [runs, setRuns] = useState<RunSummary[]>([]);
	const [runId, setRunId] = useState("");
	const [games, setGames] = useState<GameSummary[]>([]);
	const [gameId, setGameId] = useState("");
	const [record, setRecord] = useState<GameRecord | null>(null);
	const [source, setSource] = useState("stored");
	const [step, setStep] = useState(0);
	const [spymaster, setSpymaster] = useState(false);
	const [seats, setSeats] = useState("bot:codenames-random");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [notice, setNotice] = useState<string | null>(null);

	const show = useCallback((rec: GameRecord, from: string) => {
		setRecord(rec);
		setSource(from);
		setStep(rec.events.length);
	}, []);

	useEffect(() => {
		codenamesListRuns()
			.then((r) => {
				setRuns(r.runs);
				if (r.runs.length > 0) setRunId(r.runs[0].run_id);
				else {
					setNotice("No stored runs — showing the bundled demo game.");
					show(DEMO_RECORD, "demo");
				}
			})
			.catch((e) => {
				setNotice(`${describeError(e)} — showing the bundled demo game.`);
				show(DEMO_RECORD, "demo");
			});
	}, [show]);

	useEffect(() => {
		if (!runId) return;
		codenamesListGames(runId)
			.then((r) => {
				setGames(r.games);
				setGameId(r.games.length > 0 ? r.games[0].game_id : "");
			})
			.catch((e) => setError(describeError(e)));
	}, [runId]);

	useEffect(() => {
		if (!gameId) return;
		setError(null);
		codenamesReplay(gameId)
			.then((r) => show(r.record, "stored"))
			.catch((e) => setError(describeError(e)));
	}, [gameId, show]);

	const playAdHoc = useCallback(() => {
		setBusy(true);
		setError(null);
		const models = seats
			.split(",")
			.map((s) => s.trim())
			.filter(Boolean);
		codenamesPlayGame({
			models,
			seed: Math.floor(Math.random() * 1_000_000),
			include_record: true,
		})
			.then((r) => {
				if (r.record) show(r.record, "ad-hoc");
				else setError("the engine returned no record");
			})
			.catch((e) => setError(describeError(e)))
			.finally(() => setBusy(false));
	}, [seats, show]);

	const cards = useMemo(
		() => (record ? foldGrid(record, step) : []),
		[record, step],
	);
	const turns = useMemo(() => (record ? buildTurns(record) : []), [record]);
	const thoughts = useMemo(
		() => (record ? thoughtsByEvent(record) : new Map()),
		[record],
	);
	const left = useMemo(() => agentsLeft(cards), [cards]);
	const active = record ? justRevealed(record, step) : null;
	const clue = record ? activeClue(record, step) : null;
	const unknown = unknownKeyCards(cards);

	const seatLabel = useCallback(
		(seat: Seat) => {
			const s = record?.seats.find((x) => x.seat === seat);
			return s ? `${s.team.toUpperCase()} ${s.role}` : `seat ${seat}`;
		},
		[record],
	);

	return (
		<main className="cn-root">
			<div className="cn-toolbar">
				<label>
					Run{" "}
					<select value={runId} onChange={(e) => setRunId(e.target.value)}>
						{runs.length === 0 && <option value="">—</option>}
						{runs.map((r) => (
							<option key={r.run_id} value={r.run_id}>
								{r.run_id} ({r.games_played} games)
							</option>
						))}
					</select>
				</label>
				<label>
					Game{" "}
					<select value={gameId} onChange={(e) => setGameId(e.target.value)}>
						{games.length === 0 && <option value="">—</option>}
						{games.map((g) => (
							<option key={g.game_id} value={g.game_id}>
								{g.schedule_label} — {g.winner.toUpperCase()} by {g.end_reason}{" "}
								in {g.turns}t
							</option>
						))}
					</select>
				</label>
				<label className="cn-adhoc">
					Ad-hoc seats{" "}
					<input
						onChange={(e) => setSeats(e.target.value)}
						size={26}
						value={seats}
					/>
				</label>
				<button
					className="cn-btn"
					disabled={busy}
					onClick={playAdHoc}
					type="button"
				>
					{busy ? "playing…" : "Play a game"}
				</button>
			</div>

			{error && <p className="cn-error">{error}</p>}
			{notice && <p className="cn-dim">{notice}</p>}

			{record && (
				<>
					<div className="cn-stage">
						<div>
							<div className="cn-scoreline">
								<span className="cn-score cn-side-a">
									Team A <b>{left.a}</b> left
								</span>
								<span className="cn-score cn-side-b">
									Team B <b>{left.b}</b> left
								</span>
								{clue && (
									<span
										className={clue.fresh ? "cn-now cn-now-fresh" : "cn-now"}
									>
										clue: <b>{clue.clue.word}</b> {clue.clue.number} ·{" "}
										{clue.team.toUpperCase()}
									</span>
								)}
								{step >= record.events.length && (
									<span className="cn-now">
										{record.winner.toUpperCase()} wins · {record.end_reason}
									</span>
								)}
							</div>

							<WordGrid
								activeWord={active}
								cards={cards}
								spymaster={spymaster}
							/>

							<div className="cn-scrub">
								<input
									max={record.events.length}
									min={0}
									onChange={(e) => setStep(Number(e.target.value))}
									type="range"
									value={step}
								/>
								<span className="cn-dim">
									event {step}/{record.events.length}
									{active && ` — flipped ${active}`}
								</span>
							</div>

							<div className="cn-viewbar">
								<span className="cn-dim">View</span>
								<button
									className={spymaster ? "cn-toggle" : "cn-toggle cn-toggle-on"}
									onClick={() => setSpymaster(false)}
									type="button"
								>
									Operative
								</button>
								<button
									className={spymaster ? "cn-toggle cn-toggle-on" : "cn-toggle"}
									onClick={() => setSpymaster(true)}
									type="button"
								>
									Spymaster key
								</button>
								{spymaster && (
									<span className="cn-dim">
										key read off the record's reveals
										{unknown > 0 && ` · ${unknown} card(s) never flipped`}
									</span>
								)}
							</div>
						</div>

						<ClueHistory
							onSeek={setStep}
							seatLabel={seatLabel}
							step={step}
							thoughts={thoughts}
							turns={turns}
						/>
					</div>

					<div className="cn-panel">
						<h3>
							Seats{" "}
							<span className="cn-dim">
								· {source} · {record.game_id}
							</span>
						</h3>
						<div className="cn-seats">
							{record.seats.map((s) => (
								<div className={`cn-seat cn-side-${s.team}`} key={s.seat}>
									<div className="cn-seat-role">
										{s.team.toUpperCase()} {s.role}
										{s.won && <span className="cn-chip">winner</span>}
										{s.is_anchor && <span className="cn-chip">anchor</span>}
									</div>
									<div className="cn-seat-model">{s.model_id}</div>
									<div className="cn-dim">
										{s.reliability.malformed_outputs}m /{" "}
										{s.reliability.illegal_moves}i /{" "}
										{s.reliability.forced_defaults}f ·{" "}
										{s.usage.prompt_tokens + s.usage.completion_tokens} tok
									</div>
								</div>
							))}
						</div>
						<p className="cn-dim">
							seed {record.seed} · {record.rules_version} · wordlist{" "}
							{record.wordlist_hash.slice(0, 12)} · {record.schedule_label} ·{" "}
							{record.turns} turns
						</p>
					</div>
				</>
			)}
		</main>
	);
}
