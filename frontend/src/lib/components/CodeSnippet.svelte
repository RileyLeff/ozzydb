<script lang="ts">
	let {
		code,
		language = 'python',
		title,
	}: { code: string; language?: string; title?: string } = $props();

	let copied = $state(false);
	let timeout_id = $state<ReturnType<typeof setTimeout> | null>(null);

	function copy_to_clipboard() {
		navigator.clipboard.writeText(code);
		copied = true;

		if (timeout_id) {
			clearTimeout(timeout_id);
		}

		timeout_id = setTimeout(() => {
			copied = false;
			timeout_id = null;
		}, 2000);
	}
</script>

<div class="code-snippet">
	{#if title}
		<div class="header">
			<span class="title">{title}</span>
			<span class="language-badge">{language}</span>
		</div>
	{/if}

	<div class="code-container">
		<button class="copy-button" onclick={copy_to_clipboard} aria-label="Copy code to clipboard">
			{#if copied}
				<svg
					xmlns="http://www.w3.org/2000/svg"
					width="16"
					height="16"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					stroke-linejoin="round"
				>
					<polyline points="20 6 9 17 4 12" />
				</svg>
			{:else}
				<svg
					xmlns="http://www.w3.org/2000/svg"
					width="16"
					height="16"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					stroke-linejoin="round"
				>
					<rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
					<path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
				</svg>
			{/if}
		</button>

		<pre><code>{code}</code></pre>
	</div>
</div>

<style>
	.code-snippet {
		background: var(--gray-900, #1a1a1a);
		border-radius: var(--radius-lg, 12px);
		overflow: hidden;
		min-width: 0;
	}

	.header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 16px;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
	}

	.title {
		font-family: var(--font-mono, monospace);
		font-size: 12px;
		color: var(--gray-400, #999);
	}

	.language-badge {
		font-family: var(--font-mono, monospace);
		font-size: 11px;
		color: var(--gray-400, #999);
		background: rgba(255, 255, 255, 0.06);
		padding: 2px 8px;
		border-radius: var(--radius-sm, 4px);
		text-transform: lowercase;
	}

	.code-container {
		position: relative;
	}

	.copy-button {
		position: absolute;
		top: 12px;
		right: 12px;
		display: flex;
		align-items: center;
		justify-content: center;
		width: 32px;
		height: 32px;
		padding: 0;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: var(--radius-sm, 6px);
		color: var(--gray-400, #999);
		cursor: pointer;
		transition: background 0.15s ease, color 0.15s ease;
	}

	.copy-button:hover {
		background: rgba(255, 255, 255, 0.12);
		color: var(--gray-200, #ddd);
	}

	pre {
		margin: 0;
		padding: 16px;
		overflow-x: auto;
		-webkit-overflow-scrolling: touch;
	}

	code {
		font-family: var(--font-mono, monospace);
		font-size: 13px;
		line-height: 1.6;
		color: var(--gray-200, #ddd);
		white-space: pre;
		tab-size: 2;
	}

	@media (max-width: 480px) {
		code {
			font-size: 11px;
		}
	}
</style>
