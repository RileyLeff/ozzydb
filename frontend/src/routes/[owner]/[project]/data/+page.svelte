<script lang="ts">
	import { page } from '$app/state';
	import { listData } from '$lib/api';
	import type { DataAtom } from '$lib/types';
	import { formatBytes, relativeTime } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let atoms: DataAtom[] = $state([]);

	$effect(() => {
		if (!owner || !slug) return;
		const currentOwner = owner;
		const currentSlug = slug;

		loading = true;
		error = null;

		listData(owner, slug)
			.then((data) => {
				if (currentOwner !== owner || currentSlug !== slug) return;
				atoms = data;
				loading = false;
			})
			.catch((err) => {
				if (currentOwner !== owner || currentSlug !== slug) return;
				error = err.message || 'Failed to load data';
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>Data - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<span class="current">Data</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading">
			<div class="spinner"></div>
		</div>
	{:else if error}
		<div class="error-state">
			<p class="error-msg">{error}</p>
		</div>
	{:else if atoms.length === 0}
		<div class="empty-state">
			<h2>No data atoms</h2>
			<p>Upload data using the CLI:</p>
			<code class="cli-hint">ozzy data upload myfile.parquet --name my-data</code>
		</div>
	{:else}
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<th>Name</th>
						<th>Type</th>
						<th>Size</th>
						<th>Uploaded</th>
						<th>Status</th>
					</tr>
				</thead>
				<tbody>
					{#each atoms as atom (atom.name)}
						<tr class:yanked={atom.yanked}>
							<td>
								<a href="/{owner}/{slug}/data/{atom.name}" class="atom-link">
									{atom.name}
								</a>
							</td>
							<td class="type-cell">{atom.content_type.split('/').pop()}</td>
							<td class="size-cell">{formatBytes(atom.byte_size)}</td>
							<td class="date-cell">{relativeTime(atom.created_at)}</td>
							<td>
								{#if atom.yanked}
									<span class="badge badge-yanked">yanked</span>
								{:else}
									<span class="badge badge-active">active</span>
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>

<style>
	.page {
		padding-top: var(--space-xl);
		padding-bottom: var(--space-3xl);
	}

	.project-header {
		margin-bottom: var(--space-lg);
	}

	.breadcrumb {
		display: flex;
		align-items: center;
		gap: var(--space-xs);
		font-size: 1rem;
	}

	.breadcrumb a {
		font-weight: 500;
	}

	.sep {
		color: var(--text-muted);
	}

	.current {
		font-weight: 600;
	}

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

	@keyframes spin {
		to { transform: rotate(360deg); }
	}

	.error-state, .empty-state {
		text-align: center;
		padding: var(--space-2xl) 0;
	}

	.error-msg {
		color: var(--error);
	}

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

	.table-wrap {
		overflow-x: auto;
	}

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

	tr:hover {
		background: var(--bg-secondary);
	}

	tr.yanked {
		opacity: 0.6;
	}

	tr.yanked .atom-link {
		text-decoration: line-through;
	}

	.atom-link {
		font-weight: 500;
		color: var(--pink);
	}

	.type-cell, .size-cell, .date-cell {
		color: var(--text-secondary);
		white-space: nowrap;
	}

	.badge-yanked {
		background: color-mix(in srgb, var(--error) 12%, transparent);
		color: var(--error);
	}

	.badge-active {
		background: color-mix(in srgb, var(--green) 12%, transparent);
		color: var(--green);
	}
</style>
