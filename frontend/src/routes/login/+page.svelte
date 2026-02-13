<script lang="ts">
	import { auth } from '$lib/auth.svelte';
	import { initiateDeviceFlow, pollDeviceFlow } from '$lib/api';
	import { goto } from '$app/navigation';
	import type { DeviceCodeResponse } from '$lib/types';

	type Step = 'idle' | 'showing_code' | 'polling' | 'success' | 'error';

	let step = $state<Step>('idle');
	let deviceData = $state<DeviceCodeResponse | null>(null);
	let errorMessage = $state('');
	let copied = $state(false);

	// Redirect if already logged in
	$effect(() => {
		if (auth.isLoggedIn) {
			goto('/');
		}
	});

	async function startFlow() {
		step = 'showing_code';
		errorMessage = '';

		try {
			deviceData = await initiateDeviceFlow();
			step = 'showing_code';
		} catch (err) {
			errorMessage = err instanceof Error ? err.message : 'Failed to start login flow';
			step = 'error';
		}
	}

	async function copyAndOpenGithub() {
		if (!deviceData) return;

		// Copy code to clipboard
		try {
			await navigator.clipboard.writeText(deviceData.user_code);
			copied = true;
		} catch {
			// Fallback: user will copy manually
		}

		// Open GitHub in new tab
		window.open(deviceData.verification_uri, '_blank', 'noopener,noreferrer');
	}

	async function beginPolling() {
		if (!deviceData) return;

		step = 'polling';

		const interval = (deviceData.interval || 5) * 1000;
		const expiresAt = Date.now() + deviceData.expires_in * 1000;

		while (step === 'polling' && Date.now() < expiresAt) {
			try {
				const result = await pollDeviceFlow(deviceData.device_code);

				if (!result.pending && result.access_token && result.user) {
					auth.login(result.access_token, result.user);
					step = 'success';

					// Brief pause to show welcome message, then redirect
					setTimeout(() => goto('/'), 1500);
					return;
				}
			} catch (err) {
				errorMessage = err instanceof Error ? err.message : 'Polling failed';
				step = 'error';
				return;
			}

			// Wait for the interval before polling again
			await new Promise((resolve) => setTimeout(resolve, interval));
		}

		// Expired
		if (step === 'polling') {
			errorMessage = 'Authorization request expired. Please try again.';
			step = 'error';
		}
	}

	async function handleCopy() {
		if (!deviceData) return;

		try {
			await navigator.clipboard.writeText(deviceData.user_code);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			// Fallback: select the text visually (clipboard API may not be available)
		}
	}

	function reset() {
		step = 'idle';
		deviceData = null;
		errorMessage = '';
		copied = false;
	}
</script>

<div class="login-page">
	<div class="login-card">
		{#if step === 'idle'}
			<div class="card-header">
				<h1 class="card-title">Sign in to OzzyDB</h1>
				<p class="card-subtitle">
					Authenticate with your GitHub account to push, pull, and collaborate on data pipelines.
				</p>
			</div>

			<button class="btn btn-primary btn-github" onclick={startFlow}>
				<svg
					xmlns="http://www.w3.org/2000/svg"
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="currentColor"
					aria-hidden="true"
				>
					<path
						d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"
					/>
				</svg>
				Sign in with GitHub
			</button>

		{:else if step === 'showing_code' && deviceData}
			<div class="card-header">
				<h1 class="card-title">Sign in with GitHub</h1>
				<p class="card-subtitle">
					Copy the code below, paste it on GitHub, then come back here.
				</p>
			</div>

			<div class="step-list">
				<div class="step-item">
					<span class="step-number">1</span>
					<div class="step-content">
						<span class="step-label">Copy your code</span>
						<button class="user-code-box" onclick={handleCopy} aria-label="Copy code to clipboard">
							<span class="user-code">{deviceData.user_code}</span>
							<span class="copy-hint">{copied ? 'Copied!' : 'Click to copy'}</span>
						</button>
					</div>
				</div>

				<div class="step-item">
					<span class="step-number">2</span>
					<div class="step-content">
						<span class="step-label">Paste it on GitHub</span>
						<button class="btn btn-secondary" onclick={copyAndOpenGithub}>
							Open GitHub &rarr;
						</button>
					</div>
				</div>

				<div class="step-item">
					<span class="step-number">3</span>
					<div class="step-content">
						<span class="step-label">Confirm here</span>
						<button class="btn btn-primary" onclick={beginPolling}>
							I've authorized on GitHub
						</button>
					</div>
				</div>
			</div>

			<button class="btn-link" onclick={reset}>Cancel</button>

		{:else if step === 'showing_code' && !deviceData}
			<div class="card-header">
				<h1 class="card-title">Loading...</h1>
			</div>
			<div class="spinner-container">
				<div class="spinner"></div>
			</div>

		{:else if step === 'polling'}
			<div class="card-header">
				<h1 class="card-title">Waiting for authorization...</h1>
				<p class="card-subtitle">
					Complete the authorization on GitHub. This page will update automatically.
				</p>
			</div>

			<div class="spinner-container">
				<div class="spinner"></div>
			</div>

			{#if deviceData}
				<p class="code-reminder">
					Your code: <span class="mono">{deviceData.user_code}</span>
				</p>
			{/if}

			<button class="btn-link" onclick={reset}>Cancel</button>

		{:else if step === 'success'}
			<div class="card-header">
				<div class="success-icon">
					<svg
						xmlns="http://www.w3.org/2000/svg"
						width="32"
						height="32"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2"
						stroke-linecap="round"
						stroke-linejoin="round"
						aria-hidden="true"
					>
						<polyline points="20 6 9 17 4 12" />
					</svg>
				</div>
				<h1 class="card-title">
					Welcome, {auth.user?.username}!
				</h1>
				<p class="card-subtitle">Redirecting you now...</p>
			</div>

		{:else if step === 'error'}
			<div class="card-header">
				<div class="error-icon">
					<svg
						xmlns="http://www.w3.org/2000/svg"
						width="32"
						height="32"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2"
						stroke-linecap="round"
						stroke-linejoin="round"
						aria-hidden="true"
					>
						<circle cx="12" cy="12" r="10" />
						<line x1="15" y1="9" x2="9" y2="15" />
						<line x1="9" y1="9" x2="15" y2="15" />
					</svg>
				</div>
				<h1 class="card-title">Something went wrong</h1>
				<p class="card-subtitle">{errorMessage}</p>
			</div>

			<button class="btn btn-primary" onclick={reset}>Try again</button>
		{/if}
	</div>
</div>

<style>
	/* ── Page layout ──────────────────────────────────────── */

	.login-page {
		display: flex;
		align-items: center;
		justify-content: center;
		min-height: calc(100vh - var(--nav-height));
		padding: var(--space-xl);
		background: var(--bg-secondary);
	}

	.login-card {
		width: 100%;
		max-width: 480px;
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		box-shadow: 0 4px 24px rgba(0, 0, 0, 0.06);
		padding: var(--space-2xl);
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: var(--space-lg);
	}

	/* ── Card header ──────────────────────────────────────── */

	.card-header {
		text-align: center;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: var(--space-sm);
	}

	.card-title {
		font-size: 1.5rem;
		font-weight: 700;
		letter-spacing: -0.02em;
		color: var(--text);
	}

	.card-subtitle {
		font-size: 0.9375rem;
		color: var(--text-secondary);
		line-height: 1.6;
		max-width: 360px;
	}

	.card-subtitle a {
		font-weight: 500;
	}

	/* ── GitHub button ────────────────────────────────────── */

	.btn-github {
		width: 100%;
		justify-content: center;
		padding: 12px 20px;
		font-size: 15px;
		font-weight: 600;
		gap: var(--space-sm);
	}

	/* ── Step list ────────────────────────────────────────── */

	.step-list {
		width: 100%;
		display: flex;
		flex-direction: column;
		gap: var(--space-lg);
	}

	.step-item {
		display: flex;
		gap: var(--space-md);
		align-items: flex-start;
	}

	.step-number {
		flex-shrink: 0;
		width: 28px;
		height: 28px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 50%;
		background: var(--bg-secondary);
		border: 1px solid var(--border-strong);
		font-size: 13px;
		font-weight: 700;
		color: var(--text-secondary);
		margin-top: 2px;
	}

	.step-content {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: var(--space-sm);
	}

	.step-label {
		font-size: 14px;
		font-weight: 600;
		color: var(--text);
	}

	/* ── User code box ────────────────────────────────────── */

	.user-code-box {
		width: 100%;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: var(--space-xs);
		padding: var(--space-md) var(--space-lg);
		border: 2px dashed var(--border-strong);
		border-radius: var(--radius-lg);
		background: var(--bg-secondary);
		cursor: pointer;
		transition: border-color 0.15s, background 0.15s;
	}

	.user-code-box:hover {
		border-color: var(--pink);
		background: color-mix(in srgb, var(--pink) 4%, var(--bg-secondary));
	}

	.user-code {
		font-family: var(--font-mono);
		font-size: 28px;
		font-weight: 700;
		letter-spacing: 0.15em;
		color: var(--text);
		line-height: 1;
	}

	.copy-hint {
		font-size: 11px;
		color: var(--text-muted);
		transition: color 0.15s;
	}

	.user-code-box:hover .copy-hint {
		color: var(--pink);
	}

	/* ── Secondary button ────────────────────────────────── */

	.btn-secondary {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		padding: 10px 16px;
		font-size: 14px;
		font-weight: 600;
		color: var(--text);
		background: var(--bg-secondary);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		cursor: pointer;
		transition: background 0.15s, border-color 0.15s;
		gap: var(--space-xs);
	}

	.btn-secondary:hover {
		background: var(--bg-hover);
		border-color: var(--text-muted);
	}

	/* ── Buttons ──────────────────────────────────────────── */

	.login-card > .btn,
	.step-content > .btn {
		width: 100%;
		justify-content: center;
		padding: 12px 20px;
		font-size: 15px;
		font-weight: 600;
	}

	.btn-link {
		font-size: 13px;
		color: var(--text-muted);
		background: none;
		border: none;
		padding: var(--space-xs) var(--space-sm);
		cursor: pointer;
		transition: color 0.15s;
	}

	.btn-link:hover {
		color: var(--text-secondary);
		text-decoration: underline;
	}

	/* ── Spinner ──────────────────────────────────────────── */

	.spinner-container {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: var(--space-md) 0;
	}

	.spinner {
		width: 32px;
		height: 32px;
		border: 3px solid var(--border);
		border-top-color: var(--pink);
		border-radius: 50%;
		animation: spin 0.8s linear infinite;
	}

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	/* ── Code reminder ────────────────────────────────────── */

	.code-reminder {
		font-size: 13px;
		color: var(--text-muted);
	}

	.code-reminder .mono {
		font-weight: 600;
		color: var(--text);
		letter-spacing: 0.05em;
	}

	/* ── Status icons ─────────────────────────────────────── */

	.success-icon {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 56px;
		height: 56px;
		border-radius: 50%;
		background: color-mix(in srgb, var(--success) 12%, transparent);
		color: var(--success);
		margin-bottom: var(--space-sm);
	}

	.error-icon {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 56px;
		height: 56px;
		border-radius: 50%;
		background: color-mix(in srgb, var(--error) 12%, transparent);
		color: var(--error);
		margin-bottom: var(--space-sm);
	}

	/* ── Responsive ───────────────────────────────────────── */

	@media (max-width: 520px) {
		.login-card {
			padding: var(--space-xl);
		}

		.user-code {
			font-size: 28px;
		}

		.card-title {
			font-size: 1.25rem;
		}
	}
</style>
