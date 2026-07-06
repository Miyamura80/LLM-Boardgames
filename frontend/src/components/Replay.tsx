// Step-through game replay: pick a run and game, scrub through the omniscient
// transcript (private events are marked), and inspect the who-suspected-who
// heatmap at each belief checkpoint.

import { useCallback, useEffect, useMemo, useState } from "react";
import { describeError } from "../api/client";
import {
	type EventRecord,
	fetchReplay,
	type GameRecord,
	type GameSummary,
	listGames,
	listRuns,
	type RunSummary,
} from "../api/sh";
import { Heatmap } from "./Heatmap";

// Event wording comes from the engine's canonical omniscient renderer
// (ShGameReplayOutput.rendered); only discussion speech gets extra treatment.
function renderEvent(rec: EventRecord, rendered: string[]): string {
	const e = rec.event;
	if (e.type === "Utterance" && !e.pass) {
		return `P${String(e.seat)}: ${String(e.text)}`;
	}
	return rendered[rec.idx] ?? e.type;
}

/** Board state derived by folding events up to `step`. */
function foldBoard(record: GameRecord, step: number) {
	let liberal = 0;
	let fascist = 0;
	let tracker = 0;
	const alive = record.roles.map(() => true);
	for (const rec of record.events.slice(0, step)) {
		const e = rec.event;
		if (e.type === "PolicyEnacted") {
			if (e.policy === "Liberal") liberal++;
			else fascist++;
			tracker = 0;
		} else if (e.type === "ElectionTrackerAdvanced") tracker = Number(e.value);
		else if (e.type === "TopDeckEnacted") tracker = 0;
		else if (e.type === "Executed") alive[Number(e.target)] = false;
	}
	return { liberal, fascist, tracker, alive };
}

export function Replay() {
	const [runs, setRuns] = useState<RunSummary[]>([]);
	const [runId, setRunId] = useState("");
	const [games, setGames] = useState<GameSummary[]>([]);
	const [gameId, setGameId] = useState("");
	const [record, setRecord] = useState<GameRecord | null>(null);
	const [rendered, setRendered] = useState<string[]>([]);
	const [step, setStep] = useState(0);
	const [checkpoint, setCheckpoint] = useState<number | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		listRuns()
			.then((r) => {
				setRuns(r.runs);
				if (r.runs.length > 0) setRunId((id) => id || r.runs[0].run_id);
			})
			.catch((e) => setError(describeError(e)));
	}, []);

	useEffect(() => {
		if (!runId) return;
		listGames(runId)
			.then((r) => {
				setGames(r.games);
				if (r.games.length > 0) setGameId(r.games[0].game_id);
			})
			.catch((e) => setError(describeError(e)));
	}, [runId]);

	const load = useCallback(() => {
		if (!gameId) return;
		fetchReplay(gameId)
			.then((r) => {
				setRecord(r.record);
				setRendered(r.rendered);
				setStep(r.record.events.length);
				const cps = [...new Set(r.record.beliefs.map((b) => b.checkpoint))];
				setCheckpoint(cps.length > 0 ? cps[cps.length - 1] : null);
				setError(null);
			})
			.catch((e) => setError(describeError(e)));
	}, [gameId]);
	useEffect(load, [load]);

	const board = useMemo(
		() => (record ? foldBoard(record, step) : null),
		[record, step],
	);
	const checkpoints = useMemo(
		() => (record ? [...new Set(record.beliefs.map((b) => b.checkpoint))] : []),
		[record],
	);
	// Thoughts keyed by the transcript position they precede: a thought with
	// at_event = N was formed when the log held N events, i.e. just before
	// event index N.
	const thoughtsByEvent = useMemo(() => {
		const map = new Map<
			number,
			{ seat: number; decision: string; text: string }[]
		>();
		for (const s of record?.seats ?? []) {
			for (const t of s.thoughts ?? []) {
				const list = map.get(t.at_event) ?? [];
				list.push({ seat: s.seat, decision: t.decision, text: t.text });
				map.set(t.at_event, list);
			}
		}
		return map;
	}, [record]);

	return (
		<section className="sh-panel">
			<div className="sh-controls">
				<label>
					Run{" "}
					<select value={runId} onChange={(e) => setRunId(e.target.value)}>
						{runs.map((r) => (
							<option key={r.run_id} value={r.run_id}>
								{r.run_id}
							</option>
						))}
					</select>
				</label>
				<label>
					Game{" "}
					<select value={gameId} onChange={(e) => setGameId(e.target.value)}>
						{games.map((g) => (
							<option key={g.game_id} value={g.game_id}>
								{g.schedule_label} · {g.winner} ({g.win_condition})
							</option>
						))}
					</select>
				</label>
			</div>
			{error && <p className="sh-error">{error}</p>}
			{record && board && (
				<>
					<div className="sh-board">
						<span>
							Liberal <strong>{board.liberal}</strong>/5
						</span>
						<span>
							Fascist <strong>{board.fascist}</strong>/6
						</span>
						<span>
							Tracker <strong>{board.tracker}</strong>/3
						</span>
						<span className="sh-muted sh-small">
							seed <span className="sh-mono">{record.seed}</span>
						</span>
					</div>
					<div className="sh-seats">
						{record.seats.map((s) => (
							<div
								key={s.seat}
								className={`sh-seat sh-role-${s.role.toLowerCase()}${board.alive[s.seat] ? "" : " sh-dead"}`}
								title={`${s.model_id} (${s.agent_kind})`}
							>
								<strong>P{s.seat}</strong> {s.role}
								{!board.alive[s.seat] && " †"}
								<div className="sh-small sh-muted">{s.model_id}</div>
							</div>
						))}
					</div>
					<label className="sh-slider">
						Transcript step {step}/{record.events.length}
						<input
							type="range"
							min={0}
							max={record.events.length}
							value={step}
							onChange={(e) => setStep(Number(e.target.value))}
						/>
					</label>
					<ol className="sh-log">
						{record.events.slice(0, step).flatMap((rec) => [
							...(thoughtsByEvent.get(rec.idx) ?? []).map((t) => (
								<li
									key={`t-${rec.idx}-${t.seat}-${t.decision}`}
									className="sh-thought"
								>
									<details>
										<summary>
											💭 P{t.seat} thinking before {t.decision}
										</summary>
										<p>{t.text}</p>
									</details>
								</li>
							)),
							<li
								key={rec.idx}
								className={rec.visibility === "Public" ? "" : "sh-private"}
							>
								<span className="sh-muted sh-mono sh-small">r{rec.round}</span>{" "}
								{rec.visibility !== "Public" && (
									<span className="sh-tag">
										private P{(rec.visibility as { Private: number }).Private}
									</span>
								)}{" "}
								{renderEvent(rec, rendered)}
							</li>,
						])}
					</ol>
					{checkpoints.length > 0 && checkpoint !== null && (
						<>
							<h3>Who suspected whom</h3>
							<div className="sh-controls">
								<label>
									Checkpoint{" "}
									<select
										value={checkpoint}
										onChange={(e) => setCheckpoint(Number(e.target.value))}
									>
										{checkpoints.map((c) => (
											<option key={c} value={c}>
												{c === 255 ? "game end" : `after policy ${c}`}
											</option>
										))}
									</select>
								</label>
							</div>
							<Heatmap
								beliefs={record.beliefs}
								checkpoint={checkpoint}
								roles={record.roles}
								alive={board.alive}
							/>
						</>
					)}
				</>
			)}
		</section>
	);
}
