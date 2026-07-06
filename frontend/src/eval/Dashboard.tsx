// The eval dashboard: fetches the latest match from `shbench serve`, shows the
// leaderboard + metrics, and lets you pick a game to replay with its suspicion
// heatmap.

import { useEffect, useState } from "react";
import { fetchLeaderboard } from "./api";
import { GameReplay } from "./GameReplay";
import { Leaderboard } from "./Leaderboard";
import { SuspicionHeatmap } from "./SuspicionHeatmap";
import type { MatchOutcome } from "./types";
import "./eval.css";

export function Dashboard() {
	const [outcome, setOutcome] = useState<MatchOutcome | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [selected, setSelected] = useState(0);

	useEffect(() => {
		fetchLeaderboard()
			.then(setOutcome)
			.catch((e) => setError(String(e)));
	}, []);

	if (error) {
		return (
			<div className="eval-root">
				<h1>Secret Hitler Evals</h1>
				<p className="warn">Could not load results: {error}</p>
				<p className="muted">
					Start the API (`shbench serve` with DATABASE_URL set) and run an eval
					(`shbench eval …`) to populate it.
				</p>
			</div>
		);
	}

	if (!outcome) {
		return (
			<div className="eval-root">
				<h1>Secret Hitler Evals</h1>
				<p className="muted">Loading…</p>
			</div>
		);
	}

	const game = outcome.records[selected];

	return (
		<div className="eval-root">
			<header>
				<h1>Secret Hitler Evals</h1>
				<p className="muted">
					{outcome.leaderboard.total_games} game(s) · rated unit = model × role
					· Weng-Lin / OpenSkill
				</p>
			</header>

			<Leaderboard outcome={outcome} />

			{outcome.records.length > 0 && (
				<>
					<div className="game-picker">
						<label htmlFor="game-select">Game:</label>
						<select
							id="game-select"
							value={selected}
							onChange={(e) => setSelected(Number(e.target.value))}
						>
							{outcome.records.map((r, i) => (
								// biome-ignore lint/suspicious/noArrayIndexKey: records are a fixed ordered list
								<option key={i} value={i}>
									#{i + 1} — seed {r.seed}, {r.winner} win
								</option>
							))}
						</select>
					</div>
					{game && (
						<div className="game-views">
							<GameReplay game={game} />
							<SuspicionHeatmap game={game} />
						</div>
					)}
				</>
			)}
		</div>
	);
}
