// The Codenames replay console: run/game pickers (mirroring the Catan tab), a
// step slider over the record's events, the 5×5 word grid with the spymaster
// key overlay toggle, and the clue-history sidebar.
//
// Two escape hatches keep the tab usable without a populated database: an
// ad-hoc bot game (`codenames_play_game` needs no store) and, when the API is
// unreachable entirely, a bundled demo record so the view can be developed
// against `bun run dev` alone.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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

	// Every request that can commit a replay record claims this monotonic token
	// first, and only commits while its claim is still the newest one. Switching
	// run or game, and racing an ad-hoc game against a stored one, therefore can
	// never let a late response overwrite the state the user has since asked
	// for. The per-effect `live` flags below layer ordinary effect cleanup on
	// top, so a superseded response also stops touching the pickers.
	const recordRequest = useRef(0);
	const claimRecord = useCallback(() => {
		recordRequest.current += 1;
		return recordRequest.current;
	}, []);
	const isCurrent = useCallback(
		(token: number) => recordRequest.current === token,
		[],
	);

	/** Commit a record, unless a newer request has already claimed the view. */
	const show = useCallback(
		(token: number, rec: GameRecord, from: string) => {
			if (!isCurrent(token)) return false;
			setRecord(rec);
			setSource(from);
			setStep(rec.events.length);
			return true;
		},
		[isCurrent],
	);

	/** Drop whatever is on screen — a failed or empty load must not leave the
	 * previous game's replay visible next to a selector that no longer names it. */
	const clearRecord = useCallback(
		(token: number) => {
			if (!isCurrent(token)) return;
			setRecord(null);
			setStep(0);
		},
		[isCurrent],
	);

	useEffect(() => {
		const token = claimRecord();
		codenamesListRuns()
			.then((r) => {
				setRuns(r.runs);
				if (r.runs.length > 0) setRunId(r.runs[0].run_id);
				else if (show(token, DEMO_RECORD, "demo"))
					setNotice("No stored runs — showing the bundled demo game.");
			})
			.catch((e) => {
				const why = describeError(e);
				setNotice(
					show(token, DEMO_RECORD, "demo")
						? `${why} — showing the bundled demo game.`
						: why,
				);
			});
	}, [show, claimRecord]);

	useEffect(() => {
		if (!runId) return;
		let live = true;
		const token = claimRecord();
		codenamesListGames(runId)
			.then((r) => {
				if (!live) return;
				setGames(r.games);
				setGameId(r.games.length > 0 ? r.games[0].game_id : "");
				// A run with no stored games must not keep the previous run's
				// replay on screen beside an empty Game picker.
				if (r.games.length === 0) clearRecord(token);
			})
			.catch((e) => {
				if (!live) return;
				setGames([]);
				setGameId("");
				clearRecord(token);
				setError(describeError(e));
			});
		return () => {
			live = false;
		};
	}, [runId, claimRecord, clearRecord]);

	useEffect(() => {
		if (!gameId) return;
		let live = true;
		const token = claimRecord();
		setError(null);
		codenamesReplay(gameId)
			.then((r) => {
				if (live) show(token, r.record, "stored");
			})
			.catch((e) => {
				if (!live) return;
				clearRecord(token);
				setError(describeError(e));
			});
		return () => {
			live = false;
		};
	}, [gameId, show, clearRecord, claimRecord]);

	const playAdHoc = useCallback(() => {
		setBusy(true);
		setError(null);
		const token = claimRecord();
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
				if (!isCurrent(token)) return;
				if (r.record) show(token, r.record, "ad-hoc");
				else setError("the engine returned no record");
			})
			.catch((e) => {
				if (isCurrent(token)) setError(describeError(e));
			})
			.finally(() => setBusy(false));
	}, [seats, show, claimRecord, isCurrent]);

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
							record={record}
							seatLabel={seatLabel}
							spymaster={spymaster}
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
