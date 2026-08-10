// The Catan replay console: run/game pickers, a step slider over the event
// log, the tabletop board, seat cards, the omniscient transcript with private
// thoughts, and the trade-flow matrix.

import { useCallback, useEffect, useMemo, useState } from "react";
import {
	catanListGames,
	catanListRuns,
	catanReplay,
	type GameRecord,
	type GameSummary,
	type RunSummary,
} from "../../api/catan";
import { describeError } from "../../api/client";
import { buildGeometry, foldBoard, PLAYER_COLORS } from "./board";
import { HexBoard } from "./HexBoard";
import { TradeMatrix } from "./TradeMatrix";

interface Replay {
	record: GameRecord;
	rendered: string[];
}

export function CatanReplay() {
	const [runs, setRuns] = useState<RunSummary[]>([]);
	const [runId, setRunId] = useState("");
	const [games, setGames] = useState<GameSummary[]>([]);
	const [gameId, setGameId] = useState("");
	const [replay, setReplay] = useState<Replay | null>(null);
	const [step, setStep] = useState(0);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		catanListRuns()
			.then((r) => {
				setRuns(r.runs);
				if (r.runs.length > 0) setRunId(r.runs[0].run_id);
			})
			.catch((e) => setError(describeError(e)));
	}, []);

	useEffect(() => {
		if (!runId) return;
		catanListGames(runId)
			.then((r) => {
				setGames(r.games);
				if (r.games.length > 0) setGameId(r.games[0].game_id);
			})
			.catch((e) => setError(describeError(e)));
	}, [runId]);

	const load = useCallback((id: string) => {
		setError(null);
		catanReplay(id)
			.then((r) => {
				setReplay(r);
				setStep(r.record.events.length);
			})
			.catch((e) => setError(describeError(e)));
	}, []);

	useEffect(() => {
		if (gameId) load(gameId);
	}, [gameId, load]);

	const visible = useMemo(
		() => replay?.record.events.slice(0, step) ?? [],
		[replay, step],
	);
	const boardEvent = useMemo(
		() =>
			replay?.record.events.find((e) => e.event.type === "BoardLaid")?.event,
		[replay],
	);
	const geometry = useMemo(
		() => (boardEvent ? buildGeometry(boardEvent) : null),
		[boardEvent],
	);
	const fold = useMemo(() => foldBoard(visible), [visible]);

	// Interleave private thoughts at their transcript positions.
	const thoughtsAt = useMemo(() => {
		const map = new Map<
			number,
			{ seat: number; decision: string; text: string }[]
		>();
		for (const seat of replay?.record.seats ?? []) {
			for (const t of seat.thoughts) {
				const list = map.get(t.at_event) ?? [];
				list.push({ seat: seat.seat, decision: t.decision, text: t.text });
				map.set(t.at_event, list);
			}
		}
		return map;
	}, [replay]);

	// Group visible events by turn for the collapsible transcript.
	const turns = useMemo(() => {
		const groups: { turn: number; entries: { idx: number; line: string }[] }[] =
			[];
		for (const [idx, record] of visible.entries()) {
			const line = replay?.rendered[idx] ?? record.event.type;
			if (
				groups.length === 0 ||
				record.round !== groups[groups.length - 1].turn
			) {
				groups.push({ turn: record.round, entries: [] });
			}
			groups[groups.length - 1].entries.push({ idx, line });
		}
		return groups;
	}, [visible, replay]);

	if (error)
		return (
			<main className="catan-root">
				<p className="catan-error">{error}</p>
			</main>
		);

	return (
		<main className="catan-root">
			<div className="catan-toolbar">
				<label>
					Run{" "}
					<select value={runId} onChange={(e) => setRunId(e.target.value)}>
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
						{games.map((g) => (
							<option key={g.game_id} value={g.game_id}>
								{g.schedule_label} — P{g.winner} wins in {g.turns}t
							</option>
						))}
					</select>
				</label>
			</div>

			{replay && geometry && boardEvent && (
				<>
					<div className="catan-stage">
						<div className="catan-board-wrap">
							<HexBoard geometry={geometry} board={boardEvent} fold={fold} />
							<div className="catan-scrub">
								<input
									type="range"
									min={0}
									max={replay.record.events.length}
									value={step}
									onChange={(e) => setStep(Number(e.target.value))}
								/>
								<span className="catan-dim">
									event {step}/{replay.record.events.length}
									{fold.lastDice &&
										` — last roll ${fold.lastDice[0]}+${fold.lastDice[1]}`}
								</span>
							</div>
						</div>

						<div className="catan-side">
							<div className="catan-panel">
								<h3>Seats</h3>
								{replay.record.seats.map((s) => (
									<div className="catan-seat" key={s.seat}>
										<span
											className="catan-swatch"
											style={{ background: PLAYER_COLORS[s.seat] }}
										/>
										<div>
											<div className="catan-seat-name">
												P{s.seat} · {s.model_id}
												{s.won && <span className="catan-chip">winner</span>}
												{s.is_anchor && (
													<span className="catan-chip">anchor</span>
												)}
											</div>
											<div className="catan-dim">
												{step >= replay.record.events.length
													? `final ${s.final_vp} VP (place ${s.placement})`
													: `${fold.publicVp[s.seat]} VP shown`}
												{" · "}
												{s.knights_played} knights ·{" "}
												{s.reliability.forced_defaults} forced
											</div>
										</div>
									</div>
								))}
								<div className="catan-dim">
									{fold.longestRoad !== null &&
										`Longest Road: P${fold.longestRoad}  `}
									{fold.largestArmy !== null &&
										`Largest Army: P${fold.largestArmy}`}
								</div>
							</div>
							<TradeMatrix events={visible} />
						</div>
					</div>

					<div className="catan-panel catan-transcript">
						<h3>Transcript</h3>
						{turns.map((g) => (
							<details key={g.turn} open={g === turns[turns.length - 1]}>
								<summary>
									{g.turn === 0 ? "Setup" : `Turn ${g.turn}`}{" "}
									<span className="catan-dim">({g.entries.length} events)</span>
								</summary>
								<ul>
									{g.entries.map((e) => (
										<li key={e.idx}>
											<span>{e.line}</span>
											{(thoughtsAt.get(e.idx + 1) ?? []).map((t) => (
												<details
													className="catan-thought"
													key={`${e.idx}-${t.seat}`}
												>
													<summary style={{ color: PLAYER_COLORS[t.seat] }}>
														💭 P{t.seat} ({t.decision})
													</summary>
													<p>{t.text}</p>
												</details>
											))}
										</li>
									))}
								</ul>
							</details>
						))}
					</div>
				</>
			)}
		</main>
	);
}
