<script lang="ts">
	import { page } from '$app/state';
	import { auth } from '$lib/auth.svelte';
	import {
		getProject,
		listData,
		listCollections,
		listEndpoints,
		listCommits
	} from '$lib/api';
	import type { ProjectDetail, CommitSummary } from '$lib/types';
	import { relativeTime, shortHash } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let project: ProjectDetail | null = $state(null);
	let dataCount = $state(0);
	let collectionCount = $state(0);
	let endpointCount = $state(0);
	let recentCommits: CommitSummary[] = $state([]);
	let retryCount = $state(0);

	$effect(() => {
		if (!owner || !slug) return;
		// eslint-disable-next-line @typescript-eslint/no-unused-expressions
		retryCount; // Subscribe to retryCount so retry triggers re-fetch
		const currentOwner = owner;
		const currentSlug = slug;

		loading = true;
		error = null;

		Promise.all([
			getProject(owner, slug),
			listData(owner, slug).catch(() => []),
			listCollections(owner, slug).catch(() => []),
			listEndpoints(owner, slug).catch(() => []),
			listCommits(owner, slug, 5).catch(() => [])
		])
			.then(([proj, data, collections, endpoints, commits]) => {
				if (currentOwner !== owner || currentSlug !== slug) return;
				project = proj;
				dataCount = data.length;
				collectionCount = collections.length;
				endpointCount = endpoints.length;
				recentCommits = commits;
				loading = false;
			})
			.catch((err) => {
				if (currentOwner !== owner || currentSlug !== slug) return;
				error = err.message || 'Failed to load project';
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="separator">/</span>
			<span class="project-name">{slug}</span>
			{#if project}
				<span class="badge" class:badge-public={project.visibility === 'public'}
					class:badge-private={project.visibility === 'private'}>
					{project.visibility}
				</span>
			{/if}
		</div>
		{#if project?.description}
			<p class="description">{project.description}</p>
		{/if}
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading">
			<div class="spinner"></div>
			<p>Loading project...</p>
		</div>
	{:else if error}
		<div class="error-state">
			<p class="error-message">{error}</p>
			<button class="btn btn-outline" onclick={() => { retryCount++; }}>
				Retry
			</button>
		</div>
	{:else}
		<!-- Summary cards -->
		<div class="summary-grid">
			<a href="/{owner}/{slug}/data" class="summary-card">
				<span class="card-value">{dataCount}</span>
				<span class="card-label">Data atoms</span>
			</a>
			<a href="/{owner}/{slug}/collections" class="summary-card">
				<span class="card-value">{collectionCount}</span>
				<span class="card-label">Collections</span>
			</a>
			<a href="/{owner}/{slug}/endpoints" class="summary-card">
				<span class="card-value">{endpointCount}</span>
				<span class="card-label">Endpoints</span>
			</a>
			<a href="/{owner}/{slug}/commits" class="summary-card">
				<span class="card-value">{project?.commit_count ?? 0}</span>
				<span class="card-label">Commits</span>
			</a>
		</div>

		<!-- Project info -->
		{#if project?.refs && project.refs.length > 0}
			<section class="section">
				<h2 class="section-title">Refs</h2>
				<div class="ref-list">
					{#each project.refs as ref}
						<div class="ref-item">
							<span class="ref-type badge">{ref.ref_type}</span>
							<span class="ref-name">{ref.name}</span>
							{#if ref.commit_sha}
								<code class="ref-sha">{shortHash(ref.commit_sha)}</code>
							{/if}
						</div>
					{/each}
				</div>
			</section>
		{/if}

		<!-- Recent commits -->
		{#if recentCommits.length > 0}
			<section class="section">
				<div class="section-header">
					<h2 class="section-title">Recent commits</h2>
					<a href="/{owner}/{slug}/commits" class="view-all">View all</a>
				</div>
				<div class="commit-list">
					{#each recentCommits as commit}
						<a href="/{owner}/{slug}/commits/{commit.git_commit_sha}" class="commit-item">
							<code class="commit-sha">{shortHash(commit.git_commit_sha)}</code>
							<span class="commit-message">{commit.message || 'No message'}</span>
							<span class="commit-date">{relativeTime(commit.created_at)}</span>
						</a>
					{/each}
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

	.project-header {
		padding-bottom: var(--space-lg);
		margin-bottom: var(--space-lg);
	}

	.breadcrumb {
		display: flex;
		align-items: center;
		gap: var(--space-xs);
		font-size: 1.25rem;
	}

	.breadcrumb a {
		font-weight: 500;
	}

	.separator {
		color: var(--text-muted);
	}

	.project-name {
		font-weight: 700;
	}

	.breadcrumb .badge {
		margin-left: var(--space-sm);
		font-size: 0.75rem;
	}

	.description {
		color: var(--text-secondary);
		margin-top: var(--space-sm);
		max-width: 600px;
	}

	/* Loading / Error */
	.loading {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: var(--space-md);
		padding: var(--space-3xl) 0;
		color: var(--text-secondary);
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
		to { transform: rotate(360deg); }
	}

	.error-state {
		text-align: center;
		padding: var(--space-2xl) 0;
	}

	.error-message {
		color: var(--error);
		margin-bottom: var(--space-md);
	}

	/* Summary cards */
	.summary-grid {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: var(--space-md);
		margin-bottom: var(--space-xl);
	}

	.summary-card {
		display: flex;
		flex-direction: column;
		align-items: center;
		padding: var(--space-lg) var(--space-md);
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		text-decoration: none;
		transition: border-color 0.15s, box-shadow 0.15s;
	}

	.summary-card:hover {
		border-color: var(--pink);
		box-shadow: 0 2px 8px color-mix(in srgb, var(--pink) 10%, transparent);
	}

	.card-value {
		font-size: 2rem;
		font-weight: 700;
		color: var(--text);
		line-height: 1;
	}

	.card-label {
		font-size: 0.875rem;
		color: var(--text-secondary);
		margin-top: var(--space-xs);
	}

	/* Sections */
	.section {
		margin-bottom: var(--space-xl);
	}

	.section-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: var(--space-md);
	}

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
	}

	.section-header .section-title {
		margin-bottom: 0;
	}

	.view-all {
		font-size: 0.875rem;
		color: var(--pink);
	}

	/* Refs */
	.ref-list {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-sm);
	}

	.ref-item {
		display: flex;
		align-items: center;
		gap: var(--space-xs);
		padding: var(--space-xs) var(--space-sm);
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 0.875rem;
	}

	.ref-type {
		font-size: 0.75rem;
	}

	.ref-name {
		font-weight: 500;
	}

	.ref-sha {
		font-size: 0.75rem;
		color: var(--text-secondary);
	}

	/* Commits */
	.commit-list {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.commit-item {
		display: flex;
		align-items: center;
		gap: var(--space-md);
		padding: var(--space-sm) var(--space-md);
		text-decoration: none;
		border-bottom: 1px solid var(--border);
		transition: background 0.1s;
	}

	.commit-item:last-child {
		border-bottom: none;
	}

	.commit-item:hover {
		background: var(--bg-secondary);
	}

	.commit-sha {
		font-size: 0.8125rem;
		color: var(--pink);
		flex-shrink: 0;
	}

	.commit-message {
		flex: 1;
		color: var(--text);
		font-size: 0.875rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.commit-date {
		font-size: 0.8125rem;
		color: var(--text-secondary);
		flex-shrink: 0;
	}

	@media (max-width: 768px) {
		.summary-grid {
			grid-template-columns: repeat(2, 1fr);
		}

		.commit-date {
			display: none;
		}
	}
</style>
