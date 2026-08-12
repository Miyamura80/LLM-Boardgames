// Role-conditioned Codenames leaderboard: the headline is role-averaged μ−2σ,
// but the spymaster and operative lines are always shown next to it (the split
// is the honest story), together with the per-side win-rate diagnostic and the
// per-model objective metrics.

import { useCallback, useEffect, useRef, useState } from "react";
import { describeError } from "../../api/client";
import {
	codenamesLeaderboard,
	codenamesListRuns,
	codenamesRunMatch,
	type LeaderboardRow,
	type MatchProgress,
	type ModelMetricSummary,
	type RoleLine,
	type RunSummary,
} from "../../api/codenames";

function ratio(num: number, den: number, digits = 2): string {
	return den > 0 ? (num / den).toFixed(digits) : "—";
}

function pct(num: number, den: number): string {
	return den > 0 ? `${((num / den) * 100).toFixed(0)}%` : "—";
}

function Role({ line, name }: { line: RoleLine | null; name: string }) {
	if (!line) return <div className="cn-dim">{name} —</div>;
	return (
		<div className="cn-roleline">
			<span className="cn-role-name">{name}</span> {line.mu.toFixed(1)}±
			{line.sigma.toFixed(1)}{" "}
			<span className="cn-dim">
				({line.conservative.toFixed(1)} · {line.wins}/{line.games})
			</span>
		</div>
	);
}

export function CodenamesLeaderboard() {
	const [runs, setRuns] = useState<RunSummary[]>([]);
	const [runId, setRunId] = useState("");
	const [rows, setRows] = useState<LeaderboardRow[]>([]);
	const [metrics, setMetrics] = useState<ModelMetricSummary[]>([]);
	const [note, setNote] = useState<string | null>(null);
	const [progress, setProgress] = useState<MatchProgress | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		codenamesListRuns()
			.then((r) => {
				setRuns(r.runs);
				if (r.runs.length > 0) setRunId(r.runs[0].run_id);
			})
			.catch((e) => setError(describeError(e)));
	}, []);

	// Monotonic claims, one per commit target: a leaderboard response only lands
	// while it is still the newest one asked for, so switching run mid-flight
	// can never let the previous run's rows win the race. `load` also bumps the
	// progress claim because it resets `progress` — an in-flight schedule check
	// belongs to the run the user just left.
	const boardRequest = useRef(0);
	const progressRequest = useRef(0);

	const load = useCallback((id: string) => {
		boardRequest.current += 1;
		progressRequest.current += 1;
		const token = boardRequest.current;
		setError(null);
		setProgress(null);
		codenamesLeaderboard(id)
			.then((r) => {
				if (boardRequest.current !== token) return;
				setRows(r.rows);
				setMetrics(r.metrics);
				setNote(r.uncertainty_note);
			})
			.catch((e) => {
				if (boardRequest.current !== token) return;
				setRows([]);
				setMetrics([]);
				setNote(null);
				setError(describeError(e));
			});
	}, []);

	useEffect(() => {
		if (runId) load(runId);
	}, [runId, load]);

	// `max_games: 0` plays nothing: it resolves the stored spec's schedule and
	// reports how much of it is already persisted.
	const checkProgress = useCallback(() => {
		if (!runId) return;
		progressRequest.current += 1;
		const token = progressRequest.current;
		codenamesRunMatch({ run_id: runId, max_games: 0 })
			.then((r) => {
				if (progressRequest.current === token) setProgress(r.progress);
			})
			.catch((e) => {
				if (progressRequest.current === token) setError(describeError(e));
			});
	}, [runId]);

	return (
		<main className="cn-root">
			<div className="cn-toolbar">
				<label>
					Run{" "}
					<select onChange={(e) => setRunId(e.target.value)} value={runId}>
						{runs.length === 0 && <option value="">—</option>}
						{runs.map((r) => (
							<option key={r.run_id} value={r.run_id}>
								{r.run_id} ({r.mode}, {r.games_played} games, {r.status})
							</option>
						))}
					</select>
				</label>
				<button className="cn-btn" onClick={checkProgress} type="button">
					Check schedule progress
				</button>
				{progress && (
					<span className="cn-dim">
						{progress.total_games - progress.remaining}/{progress.total_games}{" "}
						scheduled games stored · {progress.remaining} outstanding
					</span>
				)}
			</div>

			{error && <p className="cn-error">{error}</p>}
			{note && <p className="cn-dim">{note}</p>}

			<div className="cn-panel">
				<h3>Leaderboard — role-averaged μ−2σ</h3>
				<table className="cn-table">
					<thead>
						<tr>
							<th>#</th>
							<th>Model</th>
							<th>Score</th>
							<th>Games</th>
							<th>Per-role μ±σ (μ−kσ · wins/games)</th>
							<th>Side win rate</th>
						</tr>
					</thead>
					<tbody>
						{rows.map((r, i) => (
							<tr key={r.model_id}>
								<td>{i + 1}</td>
								<td>
									{r.model_id}
									{r.is_anchor && <span className="cn-chip">anchor</span>}
									{r.high_uncertainty && (
										<span className="cn-chip cn-chip-warn">uncertain</span>
									)}
								</td>
								<td>{r.overall_conservative.toFixed(2)}</td>
								<td>{r.total_games}</td>
								<td>
									<Role line={r.spymaster} name="spymaster" />
									<Role line={r.operative} name="operative" />
								</td>
								<td className="cn-dim">
									{r.sides.map((s) => (
										<div key={s.side}>
											{s.side} {(s.win_rate * 100).toFixed(0)}% ({s.wins}/
											{s.games})
										</div>
									))}
								</td>
							</tr>
						))}
					</tbody>
				</table>
			</div>

			<div className="cn-panel">
				<h3>Objective metrics</h3>
				<table className="cn-table">
					<thead>
						<tr>
							<th>Model</th>
							<th>Seats</th>
							<th>Wins</th>
							<th>Avg clue #</th>
							<th>Clue yield</th>
							<th>Guess acc.</th>
							<th>1st guess</th>
							<th>Bonus miss</th>
							<th>Caused e/b/☠</th>
							<th>Assassin losses</th>
							<th>Reliability (m/i/f)</th>
						</tr>
					</thead>
					<tbody>
						{metrics.map((m) => (
							<tr key={m.model_id}>
								<td>
									{m.model_id}
									{m.is_anchor && <span className="cn-chip">anchor</span>}
								</td>
								<td>{m.seats}</td>
								<td>{pct(m.wins, m.seats)}</td>
								<td>{ratio(m.clue_number_sum, m.clues_given, 1)}</td>
								<td>{ratio(m.agents_found, m.clue_yield_den)}</td>
								<td>{pct(m.guess_hits, m.guesses)}</td>
								<td>{pct(m.first_guess_hits, m.first_guesses)}</td>
								<td>{pct(m.bonus_misses, m.bonus_guesses)}</td>
								<td>
									{m.enemy_hits_caused}/{m.bystander_hits_caused}/
									{m.assassin_hits_caused}
								</td>
								<td>{m.assassin_losses}</td>
								<td className="cn-dim">
									{m.malformed}/{m.illegal}/{m.forced}
								</td>
							</tr>
						))}
					</tbody>
				</table>
				<p className="cn-dim">
					Clue yield = own agents found per clue. “Caused” counts the enemy,
					bystander, and assassin flips a spymaster's clues led to; passes and
					hazard are role-specific and read together with the guess columns.
				</p>
			</div>
		</main>
	);
}
