<script lang="ts">
	import { page } from '$app/state';
	import { listEndpoints } from '$lib/api';
	import type { EndpointSummary } from '$lib/types';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let endpoints: EndpointSummary[] = $state([]);

	$effect(() => {
		if (!owner || !slug) return;
		const cur = { owner, slug };

		loading = true;
		error = null;

		listEndpoints(owner, slug)
			.then((data) => {
				if (cur.owner !== owner || cur.slug !== slug) return;
				endpoints = data;
				loading = false;
			})
			.catch((err) => {
				if (cur.owner !== owner || cur.slug !== slug) return;
				error = err.message || 'Failed to load endpoints';
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>Endpoints - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<span class="current">Endpoints</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading"><div class="spinner"></div></div>
	{:else if error}
		<div class="error-state"><p class="error-msg">{error}</p></div>
	{:else if endpoints.length === 0}
		<div class="empty-state">
			<h2>No endpoints</h2>
			<p>Define endpoints in your ozzy.toml and push a commit.</p>
		</div>
	{:else}
		<div class="endpoint-grid">
			{#each endpoints as ep (ep.name)}
				<a href="/{owner}/{slug}/endpoints/{ep.name}" class="endpoint-card">
					<h3 class="ep-name">{ep.name}</h3>
					{#if ep.description}
						<p class="ep-desc">{ep.description}</p>
					{/if}
					<div class="ep-meta">
						<span class="meta-item">{ep.node_count} node{ep.node_count === 1 ? '' : 's'}</span>
						{#if ep.params.length > 0}
							<span class="meta-item">{ep.params.length} param{ep.params.length === 1 ? '' : 's'}</span>
						{/if}
					</div>
					{#if ep.params.length > 0}
						<div class="param-chips">
							{#each ep.params as p}
								<span class="param-chip">
									{p.name}<span class="param-type">: {p.type}</span>
								</span>
							{/each}
						</div>
					{/if}
				</a>
			{/each}
		</div>
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

	.error-state, .empty-state {
		text-align: center;
		padding: var(--space-2xl) 0;
	}

	.error-msg { color: var(--error); }

	.empty-state h2 {
		font-size: 1.125rem;
		font-weight: 600;
		margin-bottom: var(--space-sm);
	}

	.empty-state p {
		color: var(--text-secondary);
	}

	.endpoint-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
		gap: var(--space-md);
	}

	.endpoint-card {
		display: flex;
		flex-direction: column;
		padding: var(--space-lg);
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		text-decoration: none;
		transition: border-color 0.15s, box-shadow 0.15s;
	}

	.endpoint-card:hover {
		border-color: var(--pink);
		box-shadow: 0 2px 8px color-mix(in srgb, var(--pink) 10%, transparent);
	}

	.ep-name {
		font-size: 1rem;
		font-weight: 600;
		color: var(--text);
		margin-bottom: var(--space-xs);
	}

	.ep-desc {
		font-size: 0.875rem;
		color: var(--text-secondary);
		margin-bottom: var(--space-sm);
		line-height: 1.4;
	}

	.ep-meta {
		display: flex;
		gap: var(--space-md);
		font-size: 0.8125rem;
		color: var(--text-muted);
		margin-bottom: var(--space-sm);
	}

	.param-chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-xs);
	}

	.param-chip {
		padding: 2px var(--space-xs);
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 0.75rem;
		font-family: var(--font-mono, monospace);
	}

	.param-type {
		color: var(--text-muted);
	}
</style>
