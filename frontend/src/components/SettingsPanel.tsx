import { useEffect, useRef } from "react";
import type { AppConfig } from "../hooks/useConfig";
import type { ChatSettings } from "./Chat";

interface SettingsPanelProps {
	open: boolean;
	onClose: () => void;
	settings: ChatSettings;
	onChange: (s: ChatSettings) => void;
	config: AppConfig | null;
}

export function SettingsPanel({
	open,
	onClose,
	settings,
	onChange,
	config,
}: SettingsPanelProps) {
	const dialogRef = useRef<HTMLDialogElement>(null);

	useEffect(() => {
		const dialog = dialogRef.current;
		if (!dialog) return;
		if (open) {
			if (!dialog.open) dialog.showModal();
		} else {
			if (dialog.open) dialog.close();
		}
	}, [open]);

	function handleDialogClick(e: React.MouseEvent<HTMLDialogElement>) {
		if (e.target === e.currentTarget) onClose();
	}

	return (
		<>
			{/* biome-ignore lint/a11y/useKeyWithClickEvents: <dialog> handles keyboard via native onClose/Escape */}
			<dialog
				ref={dialogRef}
				className="settings-dialog"
				onClick={handleDialogClick}
				onClose={onClose}
			>
				<aside
					className={`settings-panel${open ? " settings-panel--open" : ""}`}
				>
					<div className="settings-header">
						<span>Settings</span>
						<button
							type="button"
							className="settings-close-btn"
							onClick={onClose}
							aria-label="Close settings"
						>
							✕
						</button>
					</div>

					<div className="settings-body">
						{/* Section 1: Chat Preferences (editable) */}
						<section className="settings-section">
							<h3 className="settings-section-title">Chat Preferences</h3>

							<div className="settings-row">
								<label className="settings-label" htmlFor="setting-delay">
									Response delay
								</label>
								<div className="settings-range-wrap">
									<input
										id="setting-delay"
										type="range"
										min={400}
										max={3000}
										step={100}
										value={settings.responseDelay}
										onChange={(e) =>
											onChange({
												...settings,
												responseDelay: Number(e.target.value),
											})
										}
									/>
									<span className="settings-value">
										{settings.responseDelay}ms
									</span>
								</div>
							</div>

							<div className="settings-row">
								<label className="settings-label" htmlFor="setting-timestamps">
									Show timestamps
								</label>
								<input
									id="setting-timestamps"
									type="checkbox"
									checked={settings.showTimestamps}
									onChange={(e) =>
										onChange({ ...settings, showTimestamps: e.target.checked })
									}
								/>
							</div>

							<div className="settings-row">
								<label className="settings-label" htmlFor="setting-compact">
									Compact bubbles
								</label>
								<input
									id="setting-compact"
									type="checkbox"
									checked={settings.compactBubbles}
									onChange={(e) =>
										onChange({ ...settings, compactBubbles: e.target.checked })
									}
								/>
							</div>
						</section>

						{/* Section 2: Model Info (read-only) */}
						<section className="settings-section">
							<h3 className="settings-section-title">Model Info</h3>
							{config ? (
								<>
									<div className="settings-row">
										<span className="settings-label">Model</span>
										<span className="settings-value">
											{config.default_llm.default_model}
										</span>
									</div>
									<div className="settings-row">
										<span className="settings-label">Fallback</span>
										<span className="settings-value">
											{config.default_llm.fallback_model ?? "—"}
										</span>
									</div>
									<div className="settings-row">
										<span className="settings-label">Temperature</span>
										<span className="settings-value">
											{config.default_llm.default_temperature}
										</span>
									</div>
									<div className="settings-row">
										<span className="settings-label">Max tokens</span>
										<span className="settings-value">
											{config.default_llm.default_max_tokens}
										</span>
									</div>
								</>
							) : (
								<span className="settings-value">Loading…</span>
							)}
						</section>

						{/* Section 3: LLM Config (read-only) */}
						<section className="settings-section">
							<h3 className="settings-section-title">LLM Config</h3>
							{config ? (
								<>
									<div className="settings-row">
										<span className="settings-label">Cache</span>
										<span
											className={
												config.llm_config.cache_enabled
													? "settings-badge settings-badge--true"
													: "settings-badge settings-badge--false"
											}
										>
											{config.llm_config.cache_enabled ? "enabled" : "disabled"}
										</span>
									</div>
									<div className="settings-row">
										<span className="settings-label">Max retries</span>
										<span className="settings-value">
											{config.llm_config.retry.max_attempts}
										</span>
									</div>
									<div className="settings-row">
										<span className="settings-label">Retry wait</span>
										<span className="settings-value">
											{config.llm_config.retry.min_wait_seconds}–
											{config.llm_config.retry.max_wait_seconds}s
										</span>
									</div>
								</>
							) : (
								<span className="settings-value">Loading…</span>
							)}
						</section>

						{/* Section 4: Feature Flags (read-only) */}
						<section className="settings-section">
							<h3 className="settings-section-title">Feature Flags</h3>
							{config ? (
								Object.entries(config.features).map(([key, val]) => (
									<div key={key} className="settings-row">
										<span className="settings-label">{key}</span>
										<span
											className={
												val
													? "settings-badge settings-badge--true"
													: "settings-badge settings-badge--false"
											}
										>
											{val ? "on" : "off"}
										</span>
									</div>
								))
							) : (
								<span className="settings-value">Loading…</span>
							)}
						</section>
					</div>
				</aside>
			</dialog>
		</>
	);
}
