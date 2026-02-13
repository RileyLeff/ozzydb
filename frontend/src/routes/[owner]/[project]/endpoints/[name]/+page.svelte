<script lang="ts">
	import { page } from '$app/state';
	import { getEndpoint, getEndpointDag, fetchEndpoint } from '$lib/api';
	import type { EndpointDetail, DagResponse } from '$lib/types';
	import { shortHash } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');
	let name = $derived(page.params.name ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let endpoint: EndpointDetail | null = $state(null);
	let dag: DagResponse | null = $state(null);
	let showDag = $state(false);
	let dagLoading = $state(false);

	// Parameter form state
	let paramValues: Record<string, string> = $state({});
	let executing = $state(false);
	let execError: string | null = $state(null);
	let execResult: { type: string; url: string } | null = $state(null);

	$effect(() => {
		if (!owner || !slug || !name) return;
		const cur = { owner, slug, name };

		loading = true;
		error = null;
		dag = null;
		showDag = false;
		// Revoke previous Object URL before discarding reference
		if (execResult) URL.revokeObjectURL(execResult.url);
		execResult = null;
		execError = null;

		getEndpoint(owner, slug, name)
			.then((data) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.name !== name) return;
				endpoint = data;
				// Initialize param values with defaults
				const defaults: Record<string, string> = {};
				for (const p of data.params) {
					if (p.default !== null && p.default !== undefined) {
						defaults[p.name] = String(p.default);
					} else {
						defaults[p.name] = '';
					}
				}
				paramValues = defaults;
				loading = false;
			})
			.catch((err) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.name !== name) return;
				error = err.message || 'Failed to load endpoint';
				loading = false;
			});
	});

	async function loadDag() {
		if (dag) {
			showDag = !showDag;
			return;
		}
		dagLoading = true;
		try {
			dag = await getEndpointDag(owner, slug, name);
			showDag = true;
		} catch (err: unknown) {
			const msg = err instanceof Error ? err.message : 'Failed to load DAG';
			alert(msg);
		} finally {
			dagLoading = false;
		}
	}

	async function handleRun() {
		if (!endpoint) return;
		executing = true;
		execError = null;
		// Revoke previous Object URL to prevent memory leak
		if (execResult) URL.revokeObjectURL(execResult.url);
		execResult = null;

		// Build param overrides (only non-empty values)
		const params: Record<string, string> = {};
		for (const [k, v] of Object.entries(paramValues)) {
			if (v !== '') params[k] = v;
		}

		try {
			const res = await fetchEndpoint(owner, slug, name, params);
			const contentType = res.headers.get('content-type') || 'application/octet-stream';
			const blob = await res.blob();
			const url = URL.createObjectURL(blob);
			execResult = { type: contentType, url };
		} catch (err: unknown) {
			execError = err instanceof Error ? err.message : 'Execution failed';
		} finally {
			executing = false;
		}
	}

	function downloadResult() {
		if (!execResult) return;
		const a = document.createElement('a');
		a.href = execResult.url;
		const ext = execResult.type.includes('parquet') ? '.parquet' : execResult.type.includes('csv') ? '.csv' : '';
		a.download = `${name}${ext}`;
		a.click();
	}
</script>

<svelte:head>
	<title>{name} - Endpoints - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}/endpoints">Endpoints</a>
			<span class="sep">/</span>
			<span class="current">{name}</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading"><div class="spinner"></div></div>
	{:else if error}
		<div class="error-state"><p class="error-msg">{error}</p></div>
	{:else if endpoint}
		<!-- Endpoint info -->
		<div class="ep-header">
			<h2 class="ep-name">{endpoint.name}</h2>
			{#if endpoint.description}
				<p class="ep-desc">{endpoint.description}</p>
			{/if}
			<div class="ep-meta">
				<span>Commit: <code>{shortHash(endpoint.commit_sha)}</code></span>
				<span>{Object.keys(endpoint.nodes).length} nodes</span>
				<span>{endpoint.edges.length} edges</span>
			</div>
		</div>

		<!-- Parameter form -->
		{#if endpoint.params.length > 0}
			<section class="section">
				<h3 class="section-title">Parameters</h3>
				<div class="param-form">
					{#each endpoint.params as param}
						<div class="param-field">
							<label for="param-{param.name}" class="param-label">
								{param.name}
								<span class="param-type-tag">{param.type}</span>
							</label>
							{#if param.description}
								<p class="param-desc">{param.description}</p>
							{/if}
							{#if param.enum && param.enum.length > 0}
								<select
									id="param-{param.name}"
									class="param-input"
									bind:value={paramValues[param.name]}
								>
									{#if param.default === null || param.default === undefined}
										<option value="">-- select --</option>
									{/if}
									{#each param.enum as opt}
										<option value={String(opt)}>{String(opt)}</option>
									{/each}
								</select>
							{:else if param.type === 'bool'}
								<select
									id="param-{param.name}"
									class="param-input"
									bind:value={paramValues[param.name]}
								>
									<option value="true">true</option>
									<option value="false">false</option>
								</select>
							{:else}
								<input
									id="param-{param.name}"
									type={param.type === 'int' || param.type === 'float' ? 'number' : 'text'}
									class="param-input"
									bind:value={paramValues[param.name]}
									placeholder={param.default !== null && param.default !== undefined ? `Default: ${param.default}` : ''}
									min={param.min ?? undefined}
									max={param.max ?? undefined}
									step={param.type === 'float' ? 'any' : undefined}
								/>
							{/if}
							{#if param.min !== null || param.max !== null}
								<span class="param-range">
									{#if param.min !== null}Min: {param.min}{/if}
									{#if param.min !== null && param.max !== null} | {/if}
									{#if param.max !== null}Max: {param.max}{/if}
								</span>
							{/if}
						</div>
					{/each}
				</div>
			</section>
		{/if}

		<!-- Actions -->
		<div class="actions">
			<button class="btn btn-primary" onclick={handleRun} disabled={executing}>
				{executing ? 'Running...' : 'Run endpoint'}
			</button>
			<button class="btn btn-outline" onclick={loadDag} disabled={dagLoading}>
				{dagLoading ? 'Loading...' : showDag ? 'Hide DAG' : 'Show DAG'}
			</button>
		</div>

		<!-- Execution result -->
		{#if execError}
			<div class="exec-error">
				<p>{execError}</p>
			</div>
		{/if}

		{#if execResult}
			<div class="exec-result">
				<div class="result-header">
					<span class="result-type">{execResult.type}</span>
					<button class="btn btn-outline btn-sm" onclick={downloadResult}>
						Download
					</button>
				</div>
			</div>
		{/if}

		<!-- DAG visualization -->
		{#if showDag && dag}
			<section class="section dag-section">
				<h3 class="section-title">Pipeline DAG</h3>
				{#if dag.format === 'mermaid'}
					<pre class="dag-code">{dag.content}</pre>
				{:else}
					<pre class="dag-code">{dag.content}</pre>
				{/if}
			</section>
		{/if}

		<!-- Nodes table -->
		<section class="section">
			<h3 class="section-title">Nodes</h3>
			<div class="table-wrap">
				<table>
					<thead>
						<tr>
							<th>Name</th>
							<th>Transform</th>
							<th>Machine</th>
						</tr>
					</thead>
					<tbody>
						{#each Object.entries(endpoint.nodes) as [nodeName, node]}
							<tr>
								<td class="node-name">{nodeName}</td>
								<td><code>{node.transform}</code></td>
								<td class="meta-cell">{node.machine ?? 'default'}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</section>

		<!-- Edges table -->
		{#if endpoint.edges.length > 0}
			<section class="section">
				<h3 class="section-title">Edges</h3>
				<div class="table-wrap">
					<table>
						<thead>
							<tr>
								<th>From</th>
								<th>To</th>
							</tr>
						</thead>
						<tbody>
							{#each endpoint.edges as edge}
								<tr>
									<td>{edge.from}</td>
									<td>{edge.to}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>
		{/if}
	{/if}
</div>

<style>
	.page {
		padding-top: var(--space-xl);
		padding-bottom: var(--space-3xl);
	}

	.project-header { margin-bottom: var(--space-lg); }

	.breadcrumb {
		display: flex;
		align-items: center;
		gap: var(--space-xs);
		font-size: 1rem;
		flex-wrap: wrap;
	}

	.breadcrumb a { font-weight: 500; }
	.sep { color: var(--text-muted); }
	.current { font-weight: 600; }

	.loading {
		display: flex;
		justify-content: center;
		padding: var(--space-3xl) 0;
	}

	.spinner {
		width: 32px;
		height: 32px;
		border: 3px solid var(--border);
		border-top-color: var(--pink);
		border-radius: 50%;
		animation: spin 0.8s linear infinite;
	}

	@keyframes spin { to { transform: rotate(360deg); } }

	.error-state { text-align: center; padding: var(--space-2xl) 0; }
	.error-msg { color: var(--error); }

	/* Endpoint header */
	.ep-header { margin-bottom: var(--space-xl); }

	.ep-name {
		font-size: 1.25rem;
		font-weight: 700;
		margin-bottom: var(--space-xs);
	}

	.ep-desc {
		color: var(--text-secondary);
		font-size: 0.9375rem;
		margin-bottom: var(--space-sm);
		line-height: 1.5;
	}

	.ep-meta {
		display: flex;
		gap: var(--space-lg);
		font-size: 0.8125rem;
		color: var(--text-muted);
	}

	.ep-meta code {
		color: var(--pink);
		font-size: 0.8125rem;
	}

	/* Sections */
	.section { margin-bottom: var(--space-xl); }

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
	}

	/* Parameter form */
	.param-form {
		display: flex;
		flex-direction: column;
		gap: var(--space-md);
	}

	.param-field {
		display: flex;
		flex-direction: column;
		gap: var(--space-xs);
	}

	.param-label {
		font-size: 0.875rem;
		font-weight: 500;
		display: flex;
		align-items: center;
		gap: var(--space-sm);
	}

	.param-type-tag {
		font-size: 0.75rem;
		padding: 1px var(--space-xs);
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text-muted);
		font-family: var(--font-mono, monospace);
	}

	.param-desc {
		font-size: 0.8125rem;
		color: var(--text-secondary);
	}

	.param-input {
		padding: var(--space-sm);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 0.875rem;
		background: var(--bg);
		color: var(--text);
		max-width: 400px;
	}

	.param-input:focus {
		border-color: var(--pink);
		outline: none;
		box-shadow: 0 0 0 2px color-mix(in srgb, var(--pink) 20%, transparent);
	}

	.param-range {
		font-size: 0.75rem;
		color: var(--text-muted);
	}

	/* Actions */
	.actions {
		display: flex;
		gap: var(--space-sm);
		margin-bottom: var(--space-lg);
	}

	.btn-sm {
		padding: var(--space-xs) var(--space-sm);
		font-size: 0.8125rem;
	}

	/* Execution result */
	.exec-error {
		padding: var(--space-sm) var(--space-md);
		background: color-mix(in srgb, var(--error) 8%, var(--bg));
		border: 1px solid color-mix(in srgb, var(--error) 30%, transparent);
		border-radius: var(--radius);
		color: var(--error);
		font-size: 0.875rem;
		margin-bottom: var(--space-lg);
	}

	.exec-result {
		padding: var(--space-md);
		background: color-mix(in srgb, var(--green) 5%, var(--bg));
		border: 1px solid color-mix(in srgb, var(--green) 30%, transparent);
		border-radius: var(--radius);
		margin-bottom: var(--space-lg);
	}

	.result-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.result-type {
		font-size: 0.875rem;
		color: var(--green);
		font-weight: 500;
	}

	/* DAG */
	.dag-section { margin-top: var(--space-lg); }

	.dag-code {
		padding: var(--space-md);
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 0.8125rem;
		overflow-x: auto;
		line-height: 1.6;
	}

	/* Tables */
	.table-wrap { overflow-x: auto; }

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.875rem;
	}

	th {
		text-align: left;
		padding: var(--space-sm) var(--space-md);
		font-weight: 600;
		color: var(--text-secondary);
		border-bottom: 2px solid var(--border);
		white-space: nowrap;
	}

	td {
		padding: var(--space-sm) var(--space-md);
		border-bottom: 1px solid var(--border);
	}

	tr:hover { background: var(--bg-secondary); }

	.node-name { font-weight: 500; }
	.meta-cell { color: var(--text-secondary); }
</style>
