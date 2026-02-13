<script lang="ts">
	import { page } from '$app/state';
	import { getCommit } from '$lib/api';
	import type { CommitDetail } from '$lib/types';
	import { relativeTime } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');
	let sha = $derived(page.params.sha ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let commit: CommitDetail | null = $state(null);

	$effect(() => {
		if (!owner || !slug || !sha) return;
		const cur = { owner, slug, sha };

		loading = true;
		error = null;

		getCommit(owner, slug, sha)
			.then((data) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.sha !== sha) return;
				commit = data;
				loading = false;
			})
			.catch((err) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.sha !== sha) return;
				error = err.message || 'Failed to load commit';
				loading = false;
			});
	});

</script>

<svelte:head>
	<title>{sha.slice(0, 8)} - Commits - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}/commits">Commits</a>
			<span class="sep">/</span>
			<span class="current">{sha.slice(0, 8)}</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading"><div class="spinner"></div></div>
	{:else if error}
		<div class="error-state"><p class="error-msg">{error}</p></div>
	{:else if commit}
		<div class="detail-grid">
			<div class="detail-row">
				<span class="label">Git SHA</span>
				<span class="value">
					<code class="sha-full">{commit.git_commit_sha}</code>
					{#if commit.git_provider === 'github'}
						<a href="https://github.com/{commit.git_repo}/commit/{commit.git_commit_sha}" class="ext-link" target="_blank" rel="noopener">View on GitHub</a>
					{/if}
				</span>
			</div>
			<div class="detail-row">
				<span class="label">Message</span>
				<span class="value">{commit.message || 'No message'}</span>
			</div>
			<div class="detail-row">
				<span class="label">Pushed by</span>
				<span class="value">
					<a href="/{commit.pushed_by}">{commit.pushed_by}</a>
				</span>
			</div>
			<div class="detail-row">
				<span class="label">Pushed</span>
				<span class="value">{relativeTime(commit.created_at)}</span>
			</div>
			<div class="detail-row">
				<span class="label">Provider</span>
				<span class="value">{commit.git_provider}</span>
			</div>
			<div class="detail-row">
				<span class="label">Repository</span>
				<span class="value">{commit.git_repo}</span>
			</div>
			<div class="detail-row">
				<span class="label">ozzy.toml hash</span>
				<code class="value hash">{commit.ozzy_toml_hash}</code>
			</div>
		</div>

		<!-- Commit state sections -->
		{#if Object.keys(commit.environments).length > 0}
			<section class="section">
				<h3 class="section-title">Environments ({Object.keys(commit.environments).length})</h3>
				<div class="state-grid">
					{#each Object.entries(commit.environments) as [envName, env]}
						<div class="state-card">
							<span class="state-name">{envName}</span>
							<pre class="state-json">{JSON.stringify(env, null, 2)}</pre>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		{#if Object.keys(commit.transforms).length > 0}
			<section class="section">
				<h3 class="section-title">Transforms ({Object.keys(commit.transforms).length})</h3>
				<div class="state-grid">
					{#each Object.entries(commit.transforms) as [txName, tx]}
						<div class="state-card">
							<span class="state-name">{txName}</span>
							<pre class="state-json">{JSON.stringify(tx, null, 2)}</pre>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		{#if Object.keys(commit.endpoints).length > 0}
			<section class="section">
				<h3 class="section-title">Endpoints ({Object.keys(commit.endpoints).length})</h3>
				<div class="state-grid">
					{#each Object.entries(commit.endpoints) as [epName, ep]}
						<div class="state-card">
							<span class="state-name">
								<a href="/{owner}/{slug}/endpoints/{epName}">{epName}</a>
							</span>
							<pre class="state-json">{JSON.stringify(ep, null, 2)}</pre>
						</div>
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

	.detail-grid {
		display: grid;
		gap: 1px;
		background: var(--border);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
		margin-bottom: var(--space-xl);
	}

	.detail-row {
		display: grid;
		grid-template-columns: 140px 1fr;
		background: var(--bg);
	}

	.label {
		padding: var(--space-sm) var(--space-md);
		font-size: 0.875rem;
		font-weight: 500;
		color: var(--text-secondary);
		background: var(--bg-secondary);
	}

	.value {
		padding: var(--space-sm) var(--space-md);
		font-size: 0.875rem;
		word-break: break-all;
		display: flex;
		align-items: center;
		gap: var(--space-sm);
		flex-wrap: wrap;
	}

	.sha-full { font-size: 0.8125rem; }
	.hash { font-size: 0.8125rem; }

	.ext-link {
		font-size: 0.8125rem;
		color: var(--pink);
	}

	.section { margin-bottom: var(--space-xl); }

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
	}

	.state-grid {
		display: flex;
		flex-direction: column;
		gap: var(--space-sm);
	}

	.state-card {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.state-name {
		display: block;
		padding: var(--space-sm) var(--space-md);
		font-weight: 600;
		font-size: 0.875rem;
		background: var(--bg-secondary);
		border-bottom: 1px solid var(--border);
	}

	.state-name a {
		color: var(--pink);
	}

	.state-json {
		padding: var(--space-sm) var(--space-md);
		font-size: 0.8125rem;
		overflow-x: auto;
		line-height: 1.5;
		margin: 0;
	}
</style>
