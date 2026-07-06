// Role-conditioned leaderboard: μ±σ per role, conservative score, win rates,
// plus the objective metric summary. Values are text; identity stays in ink
// (no series colors needed — this is a table, not a chart).

import { useEffect, useState } from "react";
import { describeError } from "../api/client";
import {
	fetchLeaderboard,
	type LeaderboardRow,
	listRuns,
	type ModelMetricSummary,
	type RoleLine,
	type RunSummary,
} from "../api/sh";

function RoleCell({ line }: { line: RoleLine | null }) {
	if (!line) return <td className="sh-muted">—</td>;
	return (
		<td>
			<span className="sh-mono">
				{line.mu.toFixed(1)} ± {line.sigma.toFixed(1)}
			</span>
			<span className="sh-muted sh-small">
				{" "}
				· {(line.win_rate * 100).toFixed(0)}% of {line.games}
			</span>
		</td>
	);
}

function pct(num: number, den: number): string {
	return den > 0 ? `${((100 * num) / den).toFixed(0)}% (${num}/${den})` : "—";
}

export function Leaderboard() {
	const [runs, setRuns] = useState<RunSummary[]>([]);
	const [runId, setRunId] = useState<string>("");
	const [rows, setRows] = useState<LeaderboardRow[]>([]);
	const [metrics, setMetrics] = useState<ModelMetricSummary[]>([]);
	const [note, setNote] = useState("");
	const [anchors, setAnchors] = useState(false);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		listRuns()
			.then((r) => {
				setRuns(r.runs);
				if (r.runs.length > 0) setRunId((id) => id || r.runs[0].run_id);
			})
			.catch((e) => setError(describeError(e)));
	}, []);

	// Only the latest request may commit: quick run/filter changes would
	// otherwise race, and a failed fetch must not leave stale rows on screen.
	useEffect(() => {
		if (!runId) return;
		let current = true;
		fetchLeaderboard(runId, anchors)
			.then((r) => {
				if (!current) return;
				setRows(r.rows);
				setMetrics(r.metrics);
				setNote(r.uncertainty_note);
				setError(null);
			})
			.catch((e) => {
				if (!current) return;
				setRows([]);
				setMetrics([]);
				setError(describeError(e));
			});
		return () => {
			current = false;
		};
	}, [runId, anchors]);

	return (
		<section className="sh-panel">
			<div className="sh-controls">
				<label>
					Run{" "}
					<select value={runId} onChange={(e) => setRunId(e.target.value)}>
						{runs.map((r) => (
							<option key={r.run_id} value={r.run_id}>
								{r.run_id} · {r.mode} · {r.games_played} games · {r.status}
							</option>
						))}
					</select>
				</label>
				<label>
					<input
						type="checkbox"
						checked={anchors}
						onChange={(e) => setAnchors(e.target.checked)}
					/>{" "}
					show anchors
				</label>
			</div>
			{error && <p className="sh-error">{error}</p>}
			{rows.length > 0 && (
				<div className="sh-table-wrap">
					<table className="sh-table">
						<thead>
							<tr>
								<th>Model</th>
								<th>Liberal (μ±σ)</th>
								<th>Fascist (μ±σ)</th>
								<th>Hitler (μ±σ)</th>
								<th>Overall μ</th>
								<th>Conservative (μ−kσ)</th>
								<th>Games</th>
							</tr>
						</thead>
						<tbody>
							{rows.map((r) => (
								<tr key={r.model_id}>
									<td>
										{r.model_id}
										{r.is_anchor && <span className="sh-tag">anchor</span>}
										{r.high_uncertainty && (
											<span
												className="sh-tag sh-warn"
												title="σ too wide to separate from neighbours"
											>
												±wide
											</span>
										)}
									</td>
									<RoleCell line={r.liberal} />
									<RoleCell line={r.fascist} />
									<RoleCell line={r.hitler} />
									<td className="sh-mono">{r.overall_mu.toFixed(1)}</td>
									<td className="sh-mono">
										{r.overall_conservative.toFixed(1)}
									</td>
									<td className="sh-mono">{r.total_games}</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			)}
			{metrics.length > 0 && (
				<>
					<h3>Objective metrics (no LLM judge)</h3>
					<div className="sh-table-wrap">
						<table className="sh-table">
							<thead>
								<tr>
									<th>Model</th>
									<th title="Mean Brier score of private beliefs vs true roles; lower is better">
										Suspicion Brier (Lib)
									</th>
									<th title="Non-forced policy choices that advanced the player's faction, given the tiles held">
										Goal-aligned
									</th>
									<th title="Own-faction policies in governments where the player made the policy choice">
										Throughput
									</th>
									<th title="Executions that advanced the shooter's faction">
										Exec acc.
									</th>
									<th>Malformed</th>
									<th>Illegal</th>
									<th>Forced</th>
									<th>Tokens (in/out)</th>
								</tr>
							</thead>
							<tbody>
								{metrics.map((m) => (
									<tr key={m.model_id}>
										<td>{m.model_id}</td>
										<td className="sh-mono">
											{m.liberal_suspicion_brier?.toFixed(3) ?? "—"}
										</td>
										<td className="sh-mono">
											{pct(m.goal_aligned_num, m.goal_aligned_den)}
										</td>
										<td className="sh-mono">
											{pct(m.throughput_num, m.throughput_den)}
										</td>
										<td className="sh-mono">
											{pct(m.exec_hits, m.exec_shots)}
										</td>
										<td className="sh-mono">{m.malformed}</td>
										<td className="sh-mono">{m.illegal}</td>
										<td className="sh-mono">{m.forced}</td>
										<td className="sh-mono sh-small">
											{m.prompt_tokens.toLocaleString()} /{" "}
											{m.completion_tokens.toLocaleString()}
										</td>
									</tr>
								))}
							</tbody>
						</table>
					</div>
				</>
			)}
			{note && <p className="sh-muted sh-small">{note}</p>}
		</section>
	);
}
