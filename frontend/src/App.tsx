import { useState } from "react";
import "./App.css";
import "./catan.css";
import { CatanLeaderboard } from "./components/catan/CatanLeaderboard";
import { CatanReplay } from "./components/catan/CatanReplay";
import { Leaderboard } from "./components/Leaderboard";
import { Replay } from "./components/Replay";

type Game = "secret-hitler" | "catan";
type Tab = "leaderboard" | "replay";

function App() {
	const [game, setGame] = useState<Game>("secret-hitler");
	const [tab, setTab] = useState<Tab>("leaderboard");

	const view =
		game === "secret-hitler" ? (
			tab === "leaderboard" ? (
				<Leaderboard />
			) : (
				<Replay />
			)
		) : tab === "leaderboard" ? (
			<CatanLeaderboard />
		) : (
			<CatanReplay />
		);

	return (
		<div className="sh-app">
			<header className="sh-header">
				<h1>
					{game === "secret-hitler" ? "Secret Hitler Evals" : "Catan Evals"}
				</h1>
				<nav>
					<button
						type="button"
						className={
							game === "secret-hitler" ? "sh-tab sh-tab-active" : "sh-tab"
						}
						onClick={() => setGame("secret-hitler")}
					>
						Secret Hitler
					</button>
					<button
						type="button"
						className={game === "catan" ? "sh-tab sh-tab-active" : "sh-tab"}
						onClick={() => setGame("catan")}
					>
						Catan
					</button>
					<span className="sh-tab-divider" aria-hidden="true">
						·
					</span>
					<button
						type="button"
						className={
							tab === "leaderboard" ? "sh-tab sh-tab-active" : "sh-tab"
						}
						onClick={() => setTab("leaderboard")}
					>
						Leaderboard
					</button>
					<button
						type="button"
						className={tab === "replay" ? "sh-tab sh-tab-active" : "sh-tab"}
						onClick={() => setTab("replay")}
					>
						Replay
					</button>
				</nav>
			</header>
			{view}
		</div>
	);
}

export default App;
