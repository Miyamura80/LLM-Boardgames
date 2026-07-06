import { useState } from "react";
import "./App.css";
import { Leaderboard } from "./components/Leaderboard";
import { Replay } from "./components/Replay";

type Tab = "leaderboard" | "replay";

function App() {
	const [tab, setTab] = useState<Tab>("leaderboard");
	return (
		<div className="sh-app">
			<header className="sh-header">
				<h1>Secret Hitler Evals</h1>
				<nav>
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
			{tab === "leaderboard" ? <Leaderboard /> : <Replay />}
		</div>
	);
}

export default App;
