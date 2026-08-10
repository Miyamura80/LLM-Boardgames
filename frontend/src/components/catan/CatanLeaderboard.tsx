// Seat-conditioned FFA leaderboard + per-model metric summary for a run.

import { useCallback, useEffect, useState } from "react";
import {
	catanLeaderboard,
	catanListRuns,
	type LeaderboardRow,
	type ModelMetricSummary,
	type RunSummary,
} from "../../api/catan";
import { describeError } from "../../api/client";

export function CatanLeaderboard() {
	const [runs, setRuns] = useState<RunSummary[]>([]);
	const [runId, setRunId] = useState<string>("");
	const [rows, setRows] = useState<LeaderboardRow[]>([]);
	const [metrics, setMetrics] = useState<ModelMetricSummary[]>([]);
	const [note, setNote] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		catanListRuns()
			.then((r) => {
				setRuns(r.runs);
				if (r.runs.length > 0) setRunId(r.runs[0].run_id);
			})
			.catch((e) => setError(describeError(e)));
	}, []);

	const load = useCallback((id: string) => {
		setError(null);
		catanLeaderboard(id)
			.then((r) => {
				setRows(r.rows);
				setMetrics(r.metrics);
				setNote(r.uncertainty_note);
			})
			.catch((e) => setError(describeError(e)));
	}, []);

	useEffect(() => {
		if (runId) load(runId);
	}, [runId, load]);

	return (
		<main className="catan-root">
			<div className="catan-toolbar">
				<label>
					Run{" "}
					<select value={runId} onChange={(e) => setRunId(e.target.value)}>
						{runs.map((r) => (
							<option key={r.run_id} value={r.run_id}>
								{r.run_id} ({r.mode}, {r.games_played} games, {r.status})
							</option>
						))}
					</select>
				</label>
			</div>
			{error && <p className="catan-error">{error}</p>}
			{note && <p className="catan-dim">{note}</p>}

			<div className="catan-panel">
				<h3>Leaderboard — seat-averaged μ−2σ</h3>
				<table className="catan-table">
					<thead>
						<tr>
							<th>#</th>
							<th>Model</th>
							<th>Score</th>
							<th>Games</th>
							<th>Win rate</th>
							<th>Per-seat μ±σ (seat 0 → 3)</th>
						</tr>
					</thead>
					<tbody>
						{rows.map((r, i) => (
							<tr key={r.model_id}>
								<td>{i + 1}</td>
								<td>
									{r.model_id}
									{r.is_anchor && <span className="catan-chip">anchor</span>}
								</td>
								<td>{r.overall.toFixed(2)}</td>
								<td>{r.games}</td>
								<td>{(r.win_rate * 100).toFixed(0)}%</td>
								<td className="catan-dim">
									{r.seats
										.map(
											(s) =>
												`s${s.seat} ${s.mu.toFixed(1)}±${s.sigma.toFixed(1)}`,
										)
										.join("  ")}
								</td>
							</tr>
						))}
					</tbody>
				</table>
			</div>

			<div className="catan-panel">
				<h3>Objective metrics</h3>
				<table className="catan-table">
					<thead>
						<tr>
							<th>Model</th>
							<th>Seats</th>
							<th>Avg VP</th>
							<th>Avg place</th>
							<th>Prod eff.</th>
							<th>Setup pips %ile</th>
							<th>Trades</th>
							<th>Robber→leader</th>
							<th>Reliability (m/i/f)</th>
						</tr>
					</thead>
					<tbody>
						{metrics.map((m) => (
							<tr key={m.model_id}>
								<td>{m.model_id}</td>
								<td>{m.seats}</td>
								<td>{m.avg_final_vp?.toFixed(1) ?? "—"}</td>
								<td>{m.avg_placement?.toFixed(2) ?? "—"}</td>
								<td>
									{m.production_expected > 0
										? (m.production_actual / m.production_expected).toFixed(2)
										: "—"}
								</td>
								<td>
									{m.placement_pips_pct !== null
										? `${(m.placement_pips_pct * 100).toFixed(0)}%`
										: "—"}
								</td>
								<td>
									{m.trades_executed}/{m.trades_proposed}
								</td>
								<td>
									{m.robber_moves > 0
										? `${((m.robber_on_leader / m.robber_moves) * 100).toFixed(0)}%`
										: "—"}
								</td>
								<td className="catan-dim">
									{m.malformed}/{m.illegal}/{m.forced}
								</td>
							</tr>
						))}
					</tbody>
				</table>
			</div>
		</main>
	);
}
