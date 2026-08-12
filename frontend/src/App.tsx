import { type ComponentType, useState } from "react";
import "./App.css";
import "./catan.css";
import "./codenames.css";
import { CatanLeaderboard } from "./components/catan/CatanLeaderboard";
import { CatanReplay } from "./components/catan/CatanReplay";
import { CodenamesLeaderboard } from "./components/codenames/CodenamesLeaderboard";
import { CodenamesReplay } from "./components/codenames/CodenamesReplay";
import { Leaderboard } from "./components/Leaderboard";
import { Replay } from "./components/Replay";

type Tab = "leaderboard" | "replay";

interface GameViews {
	/** Tab label in the switcher. */
	label: string;
	/** Page title while this game is selected. */
	title: string;
	views: Record<Tab, ComponentType>;
}

// One entry per game; adding a fourth game is a new entry, not a new branch.
const GAMES = {
	"secret-hitler": {
		label: "Secret Hitler",
		title: "Secret Hitler Evals",
		views: { leaderboard: Leaderboard, replay: Replay },
	},
	catan: {
		label: "Catan",
		title: "Catan Evals",
		views: { leaderboard: CatanLeaderboard, replay: CatanReplay },
	},
	codenames: {
		label: "Codenames",
		title: "Codenames Evals",
		views: { leaderboard: CodenamesLeaderboard, replay: CodenamesReplay },
	},
} satisfies Record<string, GameViews>;

type Game = keyof typeof GAMES;

const GAME_KEYS = Object.keys(GAMES) as Game[];
const TABS: Tab[] = ["leaderboard", "replay"];

function App() {
	const [game, setGame] = useState<Game>("secret-hitler");
	const [tab, setTab] = useState<Tab>("leaderboard");

	const selected = GAMES[game];
	const View = selected.views[tab];

	return (
		<div className="sh-app">
			<header className="sh-header">
				<h1>{selected.title}</h1>
				<nav>
					{GAME_KEYS.map((key) => (
						<button
							className={game === key ? "sh-tab sh-tab-active" : "sh-tab"}
							key={key}
							onClick={() => setGame(key)}
							type="button"
						>
							{GAMES[key].label}
						</button>
					))}
					<span aria-hidden="true" className="sh-tab-divider">
						·
					</span>
					{TABS.map((key) => (
						<button
							className={tab === key ? "sh-tab sh-tab-active" : "sh-tab"}
							key={key}
							onClick={() => setTab(key)}
							type="button"
						>
							{key === "leaderboard" ? "Leaderboard" : "Replay"}
						</button>
					))}
				</nav>
			</header>
			<View />
		</div>
	);
}

export default App;
