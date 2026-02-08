<script lang="ts">
	import { page } from '$app/state';
	import { getProject, listTransforms, getTransformSource } from '$lib/api';
	import type { ProjectInfo, TransformInfo } from '$lib/types';

	let loading = $state(true);
	let error: string | null = $state(null);

	let project: ProjectInfo | null = $state(null);
	let transform: TransformInfo | null = $state(null);
	let source: string | null = $state(null);

	let owner = $derived(page.params.owner);
	let projectSlug = $derived(page.params.project);
	let transformName = $derived(page.params.name);

	let lineCount = $derived(source ? source.split('\n').length : 0);
	let lines = $derived(source ? source.split('\n') : []);

	let copied = $state(false);
	function copySource() {
		if (source) navigator.clipboard.writeText(source);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}

	$effect(() => {
		const currentOwner = owner;
		const currentProject = projectSlug;
		const currentName = transformName;

		if (!currentOwner || !currentProject || !currentName) return;

		loading = true;
		error = null;

		Promise.all([
			getProject(currentOwner, currentProject),
			listTransforms(currentOwner, currentProject),
			getTransformSource(currentOwner, currentProject, currentName)
		])
			.then(([projectData, transformsData, sourceData]) => {
				if (currentOwner !== owner || currentProject !== projectSlug || currentName !== transformName) return;
				project = projectData;
				source = sourceData;

				const match = transformsData.transforms.find((t) => t.name === currentName);
				if (match) {
					transform = match;
				} else {
					error = 'not_found';
				}
				loading = false;
			})
			.catch((err) => {
				if (currentOwner !== owner || currentProject !== projectSlug || currentName !== transformName) return;
				if (err.status === 404) {
					error = 'not_found';
				} else {
					error = err.message || 'Failed to load transform';
				}
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{transform ? transform.name : transformName} - {owner}/{projectSlug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	{#if loading}
		<div class="loading-state">
			<div class="spinner"></div>
			<p class="loading-text">Loading transform...</p>
		</div>
	{:else if error === 'not_found'}
		<div class="error-state">
			<h2>Transform not found</h2>
			<p>
				The transform <span class="mono">{transformName}</span> does not exist in
				<a href="/{owner}/{projectSlug}">{owner}/{projectSlug}</a>.
			</p>
			<a href="/{owner}/{projectSlug}" class="btn btn-outline">Back to project</a>
		</div>
	{:else if error}
		<div class="error-state">
			<h2>Something went wrong</h2>
			<p>{error}</p>
			<button class="btn btn-outline" onclick={() => location.reload()}>Try again</button>
		</div>
	{:else if transform}
		<!-- Breadcrumb -->
		<nav class="breadcrumb" aria-label="Breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="separator">/</span>
			<a href="/{owner}/{projectSlug}">{projectSlug}</a>
			<span class="separator">/</span>
			<span class="breadcrumb-segment">transforms</span>
			<span class="separator">/</span>
			<span class="breadcrumb-current">{transform.name}</span>
		</nav>

		<!-- Header -->
		<header class="transform-header">
			<h1 class="transform-name">{transform.name}</h1>
			<div class="meta-row">
				<span class="badge badge-runtime">{transform.runtime}</span>
				<span class="meta-item mono">{transform.function_name}()</span>
				<span class="meta-item mono hash">{transform.hash.slice(0, 8)}</span>
				{#if transform.reproducible}
					<span class="meta-item reproducible">reproducible</span>
				{/if}
			</div>
		</header>

		<!-- Source code viewer -->
		<section class="section">
			<div class="source-header">
				<h2 class="section-title">Source</h2>
				<div class="source-actions">
					<span class="line-count">{lineCount} lines</span>
					<button class="copy-btn" onclick={copySource}>
						{copied ? 'Copied!' : 'Copy'}
					</button>
				</div>
			</div>

			{#if source !== null}
				<div class="source-viewer">
					<table class="source-table">
						<tbody>
							{#each lines as line, i (i)}
								<tr>
									<td class="line-number">{i + 1}</td>
									<td class="line-content"><pre>{line}</pre></td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<div class="empty-state">Source code not available.</div>
			{/if}
		</section>

		<!-- Schema info if available -->
		{#if transform.input_schema || transform.output_schema}
			<section class="section">
				<h2 class="section-title">Schema</h2>
				<div class="schema-blocks">
					{#if transform.input_schema}
						<div class="schema-block">
							<h3 class="schema-label">Input</h3>
							<pre class="schema-code">{JSON.stringify(transform.input_schema, null, 2)}</pre>
						</div>
					{/if}
					{#if transform.output_schema}
						<div class="schema-block">
							<h3 class="schema-label">Output</h3>
							<pre class="schema-code">{JSON.stringify(transform.output_schema, null, 2)}</pre>
						</div>
					{/if}
				</div>
			</section>
		{/if}
	{/if}
</div>

<style>
	/* -- Page layout ---------------------------------------- */

	.page {
		padding-top: var(--space-xl);
		padding-bottom: var(--space-3xl);
	}

	/* -- Loading / Error states ----------------------------- */

	.loading-state {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		padding: var(--space-3xl) 0;
		gap: var(--space-md);
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

	.loading-text {
		color: var(--text-secondary);
	}

	.error-state {
		text-align: center;
		padding: var(--space-3xl) 0;
	}

	.error-state h2 {
		font-size: 1.5rem;
		font-weight: 700;
		margin-bottom: var(--space-sm);
	}

	.error-state p {
		color: var(--text-secondary);
		margin-bottom: var(--space-lg);
	}

	/* -- Breadcrumb ----------------------------------------- */

	.breadcrumb {
		display: flex;
		align-items: center;
		gap: var(--space-xs);
		font-size: 14px;
		margin-bottom: var(--space-lg);
	}

	.breadcrumb a {
		font-weight: 500;
	}

	.separator {
		color: var(--text-muted);
	}

	.breadcrumb-segment {
		color: var(--text-secondary);
	}

	.breadcrumb-current {
		font-family: var(--font-mono);
		font-weight: 600;
		color: var(--text);
	}

	/* -- Transform header ----------------------------------- */

	.transform-header {
		padding-bottom: var(--space-lg);
		border-bottom: 1px solid var(--border);
		margin-bottom: var(--space-xl);
	}

	.transform-name {
		font-family: var(--font-mono);
		font-size: 1.75rem;
		font-weight: 700;
		line-height: 1.3;
	}

	.meta-row {
		margin-top: var(--space-sm);
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-sm);
		align-items: center;
	}

	.meta-item {
		font-size: 13px;
		color: var(--text-secondary);
	}

	.meta-item.hash {
		color: var(--text-muted);
	}

	.badge {
		display: inline-flex;
		align-items: center;
		padding: 2px 8px;
		font-size: 12px;
		font-weight: 600;
		border-radius: 999px;
		line-height: 1.5;
	}

	.badge-runtime {
		background: color-mix(in srgb, var(--pink) 12%, transparent);
		color: var(--pink);
		border: 1px solid color-mix(in srgb, var(--pink) 25%, transparent);
		font-family: var(--font-mono);
	}

	.reproducible {
		background: color-mix(in srgb, var(--green) 12%, transparent);
		color: var(--green);
		border: 1px solid color-mix(in srgb, var(--green) 25%, transparent);
		padding: 2px 8px;
		font-size: 12px;
		font-weight: 600;
		border-radius: 999px;
		line-height: 1.5;
	}

	/* -- Sections ------------------------------------------- */

	.section {
		margin-bottom: var(--space-xl);
	}

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
		color: var(--text);
	}

	/* -- Source viewer --------------------------------------- */

	.source-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: var(--space-md);
	}

	.source-header .section-title {
		margin-bottom: 0;
	}

	.source-actions {
		display: flex;
		align-items: center;
		gap: var(--space-sm);
	}

	.line-count {
		font-size: 13px;
		color: var(--text-muted);
		font-family: var(--font-mono);
	}

	.copy-btn {
		font-family: var(--font-mono);
		font-size: 12px;
		padding: 4px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-secondary);
		color: var(--text-secondary);
		cursor: pointer;
		transition: color 0.15s, border-color 0.15s;
	}

	.copy-btn:hover {
		color: var(--text);
		border-color: var(--text-muted);
	}

	.source-viewer {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
		background: var(--bg-inset);
	}

	.source-table {
		width: 100%;
		border-collapse: collapse;
	}

	.source-table tr:hover {
		background: rgba(255, 255, 255, 0.02);
	}

	.line-number {
		text-align: right;
		padding: 0 12px;
		color: var(--text-muted);
		user-select: none;
		min-width: 44px;
		font-size: 13px;
		font-family: var(--font-mono);
		border-right: 1px solid rgba(255, 255, 255, 0.08);
		vertical-align: top;
		line-height: 1.6;
	}

	.line-content pre {
		margin: 0;
		padding: 0 16px;
		font-size: 13px;
		font-family: var(--font-mono);
		color: var(--text);
		white-space: pre;
		line-height: 1.6;
	}

	.empty-state {
		padding: var(--space-xl);
		text-align: center;
		color: var(--text-muted);
		border: 1px solid var(--border);
		border-radius: var(--radius);
	}

	/* -- Schema blocks -------------------------------------- */

	.schema-blocks {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-md);
	}

	.schema-block {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.schema-label {
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-secondary);
		padding: var(--space-sm) var(--space-md);
		background: var(--bg-secondary);
		border-bottom: 1px solid var(--border);
		margin: 0;
	}

	.schema-code {
		margin: 0;
		padding: var(--space-md);
		font-family: var(--font-mono);
		font-size: 13px;
		color: var(--text);
		background: var(--bg-inset);
		overflow-x: auto;
		line-height: 1.5;
	}

	/* -- Responsive ----------------------------------------- */

	@media (max-width: 768px) {
		.schema-blocks {
			grid-template-columns: 1fr;
		}
	}
</style>
