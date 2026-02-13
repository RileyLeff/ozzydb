<script lang="ts">
	import { page } from '$app/state';
	import { listCommits } from '$lib/api';
	import type { CommitSummary } from '$lib/types';
	import { relativeTime, shortHash } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let commits: CommitSummary[] = $state([]);

	$effect(() => {
		if (!owner || !slug) return;
		const cur = { owner, slug };

		loading = true;
		error = null;

		listCommits(owner, slug)
			.then((data) => {
				if (cur.owner !== owner || cur.slug !== slug) return;
				commits = data;
				loading = false;
			})
			.catch((err) => {
				if (cur.owner !== owner || cur.slug !== slug) return;
				error = err.message || 'Failed to load commits';
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>Commits - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<span class="current">Commits</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading"><div class="spinner"></div></div>
	{:else if error}
		<div class="error-state"><p class="error-msg">{error}</p></div>
	{:else if commits.length === 0}
		<div class="empty-state">
			<h2>No commits</h2>
			<p>Push your first commit using the CLI:</p>
			<code class="cli-hint">ozzy push</code>
		</div>
	{:else}
		<div class="commit-list">
			{#each commits as commit (commit.id)}
				<a href="/{owner}/{slug}/commits/{commit.git_commit_sha}" class="commit-item">
					<div class="commit-main">
						<code class="commit-sha">{shortHash(commit.git_commit_sha)}</code>
						<span class="commit-message">{commit.message || 'No message'}</span>
					</div>
					<div class="commit-meta">
						<span class="commit-author">{commit.pushed_by}</span>
						<span class="commit-date">{relativeTime(commit.created_at)}</span>
					</div>
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
		margin-bottom: var(--space-sm);
	}

	.cli-hint {
		display: inline-block;
		padding: var(--space-xs) var(--space-sm);
		background: var(--gray-900);
		color: var(--white);
		border-radius: var(--radius);
		font-size: 0.8125rem;
	}

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
		justify-content: space-between;
		gap: var(--space-md);
		padding: var(--space-sm) var(--space-md);
		text-decoration: none;
		border-bottom: 1px solid var(--border);
		transition: background 0.1s;
	}

	.commit-item:last-child { border-bottom: none; }
	.commit-item:hover { background: var(--bg-secondary); }

	.commit-main {
		display: flex;
		align-items: center;
		gap: var(--space-md);
		flex: 1;
		min-width: 0;
	}

	.commit-sha {
		font-size: 0.8125rem;
		color: var(--pink);
		flex-shrink: 0;
	}

	.commit-message {
		font-size: 0.875rem;
		color: var(--text);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.commit-meta {
		display: flex;
		align-items: center;
		gap: var(--space-md);
		flex-shrink: 0;
	}

	.commit-author {
		font-size: 0.8125rem;
		color: var(--text-secondary);
	}

	.commit-date {
		font-size: 0.8125rem;
		color: var(--text-muted);
		white-space: nowrap;
	}

	@media (max-width: 768px) {
		.commit-item {
			flex-direction: column;
			align-items: flex-start;
			gap: var(--space-xs);
		}

		.commit-meta {
			width: 100%;
		}
	}
</style>
