import { useEffect, useRef, useState } from "react";
import { callCommand } from "../api/client";
import { useConfig } from "../hooks/useConfig";
import { SettingsPanel } from "./SettingsPanel";

export interface ChatSettings {
	responseDelay: number;
	showTimestamps: boolean;
	compactBubbles: boolean;
}

const DEFAULT_SETTINGS: ChatSettings = {
	responseDelay: 1100,
	showTimestamps: false,
	compactBubbles: false,
};

type Role = "user" | "assistant";

interface Message {
	id: string;
	role: Role;
	content: string;
	timestamp: Date;
}

const MOCK_RESPONSES = [
	"I'd be happy to help with that. Could you tell me more about what you're trying to achieve?",
	"That's an interesting question. Based on what you've shared, I think the best approach would be to start small and iterate from there.",
	"Great point! One thing to consider is how this fits into the broader context of your project. Have you thought about edge cases?",
	"I understand what you're going for. Here's how I'd think through it: break the problem into smaller pieces, tackle each one, then integrate.",
	"This is a classic challenge in software development. The key insight is that the simple solution is usually the right one — resist over-engineering.",
	"Absolutely. The pattern you're describing is well-established. I'd recommend looking into how similar systems handle this at scale.",
	"Good thinking. To build on that — the tricky part is often not the implementation itself, but deciding the right interface for it.",
	"That makes sense given the constraints you're working within. My suggestion would be to prototype it first before committing to the design.",
	"Interesting! There are a few ways to approach this. The trade-offs mostly come down to simplicity vs. flexibility — depends on your priorities.",
	"I see what you mean. One thing that often gets overlooked here is the developer experience — make sure it's easy to test and debug later.",
];

export function Chat() {
	const [messages, setMessages] = useState<Message[]>([]);
	const [input, setInput] = useState("");
	const [isTyping, setIsTyping] = useState(false);
	const [messageCount, setMessageCount] = useState(0);
	const [sidebarOpen, setSidebarOpen] = useState(false);
	const [settings, setSettings] = useState<ChatSettings>(DEFAULT_SETTINGS);
	const [backendOnline, setBackendOnline] = useState<boolean | null>(null);
	const messagesEndRef = useRef<HTMLDivElement>(null);
	const textareaRef = useRef<HTMLTextAreaElement>(null);
	const { config } = useConfig();

	// biome-ignore lint/correctness/useExhaustiveDependencies: messages and isTyping are scroll triggers
	useEffect(() => {
		messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
	}, [messages, isTyping]);

	// Ping the engine over the HTTP API to show live connectivity. This is the
	// canonical example of the typed `callCommand` client hitting a real command.
	useEffect(() => {
		let cancelled = false;
		callCommand<{ pong: boolean }>("ping")
			.then(() => !cancelled && setBackendOnline(true))
			.catch(() => !cancelled && setBackendOnline(false));
		return () => {
			cancelled = true;
		};
	}, []);

	function resizeTextarea() {
		const ta = textareaRef.current;
		if (!ta) return;
		ta.style.height = "auto";
		const maxHeight = 120;
		ta.style.height = `${Math.min(ta.scrollHeight, maxHeight)}px`;
	}

	async function sendMessage() {
		const content = input.trim();
		if (!content || isTyping) return;

		const userMsg: Message = {
			id: crypto.randomUUID(),
			role: "user",
			content,
			timestamp: new Date(),
		};

		setMessages((prev) => [...prev, userMsg]);
		setInput("");
		setIsTyping(true);

		if (textareaRef.current) {
			textareaRef.current.style.height = "auto";
		}

		const delay = settings.responseDelay * (0.8 + Math.random() * 0.4);
		await new Promise((resolve) => setTimeout(resolve, delay));

		const assistantMsg: Message = {
			id: crypto.randomUUID(),
			role: "assistant",
			content: MOCK_RESPONSES[messageCount % MOCK_RESPONSES.length],
			timestamp: new Date(),
		};

		setMessageCount((c) => c + 1);
		setMessages((prev) => [...prev, assistantMsg]);
		setIsTyping(false);
	}

	function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
		if (e.key === "Enter" && !e.shiftKey) {
			e.preventDefault();
			sendMessage();
		}
	}

	const modelLabel = config?.default_llm?.default_model ?? "Assistant";
	const connectivity =
		backendOnline === null ? "unknown" : backendOnline ? "online" : "offline";
	const connectivityLabel = {
		unknown: "Checking API…",
		online: "API connected",
		offline: "API offline — run `appctl serve`",
	}[connectivity];

	return (
		<div className="chat-layout">
			<header className="chat-header">
				<span className="chat-header-title">Assistant</span>
				<div className="chat-header-right">
					<button
						type="button"
						className="chat-header-btn"
						onClick={() => setSidebarOpen(true)}
						aria-label="Open settings"
					>
						<svg
							width="18"
							height="18"
							viewBox="0 0 18 18"
							fill="none"
							aria-hidden="true"
						>
							<path
								d="M9 11.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z"
								stroke="currentColor"
								strokeWidth="1.5"
								strokeLinecap="round"
								strokeLinejoin="round"
							/>
							<path
								d="M14.85 11.15a1.25 1.25 0 0 0 .25 1.38l.04.04a1.52 1.52 0 0 1-2.15 2.15l-.04-.04a1.25 1.25 0 0 0-1.38-.25 1.25 1.25 0 0 0-.75 1.14V16a1.52 1.52 0 0 1-3.04 0v-.06a1.25 1.25 0 0 0-.82-1.14 1.25 1.25 0 0 0-1.38.25l-.04.04a1.52 1.52 0 0 1-2.15-2.15l.04-.04a1.25 1.25 0 0 0 .25-1.38 1.25 1.25 0 0 0-1.14-.75H2a1.52 1.52 0 0 1 0-3.04h.06a1.25 1.25 0 0 0 1.14-.82 1.25 1.25 0 0 0-.25-1.38l-.04-.04a1.52 1.52 0 0 1 2.15-2.15l.04.04a1.25 1.25 0 0 0 1.38.25h.06A1.25 1.25 0 0 0 7.29 2V2a1.52 1.52 0 0 1 3.04 0v.06a1.25 1.25 0 0 0 .75 1.14 1.25 1.25 0 0 0 1.38-.25l.04-.04a1.52 1.52 0 0 1 2.15 2.15l-.04.04a1.25 1.25 0 0 0-.25 1.38v.06a1.25 1.25 0 0 0 1.14.75H16a1.52 1.52 0 0 1 0 3.04h-.06a1.25 1.25 0 0 0-1.14.82Z"
								stroke="currentColor"
								strokeWidth="1.5"
								strokeLinecap="round"
								strokeLinejoin="round"
							/>
						</svg>
					</button>
					<output
						className={`chat-status-dot chat-status-dot--${connectivity}`}
						title={connectivityLabel}
					>
						<span className="sr-only">{connectivityLabel}</span>
					</output>
					<span className="chat-header-model">{modelLabel}</span>
				</div>
			</header>

			<main className="chat-messages">
				{messages.length === 0 && !isTyping && (
					<div className="chat-empty-state">
						<p>How can I help?</p>
					</div>
				)}

				{messages.map((msg) => (
					<div
						key={msg.id}
						className={`chat-message-row chat-message-row--${msg.role}`}
					>
						<div
							className={`chat-bubble chat-bubble--${msg.role}${settings.compactBubbles ? " chat-bubble--compact" : ""}`}
						>
							{msg.content}
							{settings.showTimestamps && (
								<time className="chat-message-time">
									{msg.timestamp.toLocaleTimeString([], {
										hour: "2-digit",
										minute: "2-digit",
									})}
								</time>
							)}
						</div>
					</div>
				))}

				{isTyping && (
					<div className="chat-message-row chat-message-row--assistant">
						<div className="chat-bubble chat-bubble--assistant chat-bubble--typing">
							<span className="typing-dot" />
							<span className="typing-dot" />
							<span className="typing-dot" />
						</div>
					</div>
				)}

				<div ref={messagesEndRef} />
			</main>

			<footer className="chat-input-area">
				<div className="chat-input-wrapper">
					<textarea
						ref={textareaRef}
						className="chat-textarea"
						value={input}
						onChange={(e) => {
							setInput(e.target.value);
							resizeTextarea();
						}}
						onKeyDown={handleKeyDown}
						placeholder="Message…"
						rows={1}
					/>
					<button
						type="button"
						className="chat-send-btn"
						onClick={sendMessage}
						disabled={!input.trim() || isTyping}
						aria-label="Send message"
					>
						<svg
							width="16"
							height="16"
							viewBox="0 0 16 16"
							fill="none"
							aria-hidden="true"
						>
							<path
								d="M8 2L8 14M8 2L4 6M8 2L12 6"
								stroke="currentColor"
								strokeWidth="2"
								strokeLinecap="round"
								strokeLinejoin="round"
							/>
						</svg>
					</button>
				</div>
			</footer>

			<SettingsPanel
				open={sidebarOpen}
				onClose={() => setSidebarOpen(false)}
				settings={settings}
				onChange={setSettings}
				config={config}
			/>
		</div>
	);
}
