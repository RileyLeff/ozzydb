<script lang="ts">
	import { page } from '$app/state';
	import { auth } from '$lib/auth.svelte';
	import { getCollection, getCollectionLog, flattenCollection } from '$lib/api';
	import type { CollectionDetail, VersionLogEntry, FlattenedAtom } from '$lib/types';
	import { relativeTime, shortHash } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');
	let name = $derived(page.params.name ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let collection: CollectionDetail | null = $state(null);
	let versionLog: VersionLogEntry[] = $state([]);
	let flattenedAtoms: FlattenedAtom[] | null = $state(null);
	let showFlatten = $state(false);
	let flattenLoading = $state(false);

	$effect(() => {
		if (!owner || !slug || !name) return;
		const cur = { owner, slug, name };

		loading = true;
		error = null;

		Promise.all([
			getCollection(owner, slug, name),
			getCollectionLog(owner, slug, name).catch(() => [])
		])
			.then(([col, log]) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.name !== name) return;
				collection = col;
				versionLog = log;
				loading = false;
			})
			.catch((err) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.name !== name) return;
				error = err.message || 'Failed to load collection';
				loading = false;
			});
	});

	async function handleFlatten() {
		if (flattenedAtoms) {
			showFlatten = !showFlatten;
			return;
		}
		flattenLoading = true;
		try {
			flattenedAtoms = await flattenCollection(owner, slug, name);
			showFlatten = true;
		} catch (err: unknown) {
			const msg = err instanceof Error ? err.message : 'Failed to flatten';
			alert(msg);
		} finally {
			flattenLoading = false;
		}
	}
</script>

<svelte:head>
	<title>{name} - Collections - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}/collections">Collections</a>
			<span class="sep">/</span>
			<span class="current">{name}</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading"><div class="spinner"></div></div>
	{:else if error}
		<div class="error-state"><p class="error-msg">{error}</p></div>
	{:else if collection}
		{#if collection.yanked}
			<div class="yank-banner">
				This collection has been yanked{collection.yank_reason ? `: ${collection.yank_reason}` : ''}.
			</div>
		{/if}

		<!-- Current version info -->
		<div class="detail-grid">
			<div class="detail-row">
				<span class="label">Name</span>
				<span class="value">{collection.name}</span>
			</div>
			{#if collection.version}
				<div class="detail-row">
					<span class="label">Version</span>
					<span class="value">v{collection.version.version_number}</span>
				</div>
				<div class="detail-row">
					<span class="label">Hash</span>
					<code class="value hash">{collection.version.hash}</code>
				</div>
				<div class="detail-row">
					<span class="label">Created by</span>
					<span class="value">{collection.version.created_by}</span>
				</div>
				<div class="detail-row">
					<span class="label">Created</span>
					<span class="value">{relativeTime(collection.version.created_at)}</span>
				</div>
			{/if}
			<div class="detail-row">
				<span class="label">Status</span>
				<span class="value">
					{#if collection.yanked}
						<span class="badge badge-yanked">yanked</span>
					{:else}
						<span class="badge badge-active">active</span>
					{/if}
				</span>
			</div>
		</div>

		<!-- Members -->
		{#if collection.version && collection.version.members.length > 0}
			<section class="section">
				<div class="section-header">
					<h2 class="section-title">Members ({collection.version.members.length})</h2>
					<button class="btn btn-outline btn-sm" onclick={handleFlatten} disabled={flattenLoading}>
						{flattenLoading ? 'Loading...' : showFlatten ? 'Hide flattened' : 'Flatten'}
					</button>
				</div>
				<div class="table-wrap">
					<table>
						<thead>
							<tr>
								<th>#</th>
								<th>Type</th>
								<th>Reference</th>
								<th>Hash</th>
							</tr>
						</thead>
						<tbody>
							{#each collection.version.members as member}
								<tr>
									<td class="num-cell">{member.ordinal}</td>
									<td>
										<span class="badge badge-type">{member.member_type}</span>
									</td>
									<td>
										{#if member.member_type === 'data'}
											<a href="/{owner}/{slug}/data/{member.member_ref}" class="member-link">
												{member.member_ref}
											</a>
										{:else if member.member_type === 'collection'}
											<a href="/{owner}/{slug}/collections/{member.member_ref}" class="member-link">
												{member.member_ref}
											</a>
										{:else}
											{member.member_ref}
										{/if}
									</td>
									<td><code class="hash">{shortHash(member.member_hash)}</code></td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>
		{/if}

		<!-- Flattened view -->
		{#if showFlatten && flattenedAtoms}
			<section class="section">
				<h2 class="section-title">Flattened atoms ({flattenedAtoms.length})</h2>
				<div class="table-wrap">
					<table>
						<thead>
							<tr>
								<th>Name</th>
								<th>Path</th>
								<th>Hash</th>
							</tr>
						</thead>
						<tbody>
							{#each flattenedAtoms as atom}
								<tr>
									<td>
										<a href="/{owner}/{slug}/data/{atom.name}" class="member-link">
											{atom.name}
										</a>
									</td>
									<td class="path-cell">{atom.path.join(' / ')}</td>
									<td><code class="hash">{shortHash(atom.hash)}</code></td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>
		{/if}

		<!-- Version history -->
		{#if versionLog.length > 1}
			<section class="section">
				<h2 class="section-title">Version history</h2>
				<div class="version-list">
					{#each versionLog as ver}
						<div class="version-item" class:current={collection.version && ver.version_number === collection.version.version_number}>
							<span class="ver-number">v{ver.version_number}</span>
							<code class="ver-hash">{shortHash(ver.hash)}</code>
							<span class="ver-by">{ver.created_by}</span>
							<span class="ver-date">{relativeTime(ver.created_at)}</span>
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

	.yank-banner {
		padding: var(--space-sm) var(--space-md);
		background: color-mix(in srgb, var(--error) 8%, var(--bg));
		border: 1px solid color-mix(in srgb, var(--error) 30%, transparent);
		border-radius: var(--radius);
		color: var(--error);
		font-size: 0.875rem;
		margin-bottom: var(--space-lg);
	}

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
	}

	.hash { font-size: 0.8125rem; }

	.section { margin-bottom: var(--space-xl); }

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

	.section-header .section-title { margin-bottom: 0; }

	.btn-sm {
		padding: var(--space-xs) var(--space-sm);
		font-size: 0.8125rem;
	}

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

	.num-cell { color: var(--text-secondary); }

	.badge-type {
		background: color-mix(in srgb, var(--pink) 10%, transparent);
		color: var(--pink);
		font-size: 0.75rem;
	}

	.member-link {
		font-weight: 500;
		color: var(--pink);
	}

	.path-cell {
		color: var(--text-secondary);
		font-size: 0.8125rem;
	}

	.badge-yanked {
		background: color-mix(in srgb, var(--error) 12%, transparent);
		color: var(--error);
	}

	.badge-active {
		background: color-mix(in srgb, var(--green) 12%, transparent);
		color: var(--green);
	}

	/* Version history */
	.version-list {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.version-item {
		display: flex;
		align-items: center;
		gap: var(--space-md);
		padding: var(--space-sm) var(--space-md);
		border-bottom: 1px solid var(--border);
		font-size: 0.875rem;
	}

	.version-item:last-child { border-bottom: none; }

	.version-item.current {
		background: color-mix(in srgb, var(--pink) 5%, var(--bg));
	}

	.ver-number {
		font-weight: 600;
		flex-shrink: 0;
		min-width: 32px;
	}

	.ver-hash {
		font-size: 0.8125rem;
		color: var(--pink);
		flex-shrink: 0;
	}

	.ver-by {
		flex: 1;
		color: var(--text-secondary);
	}

	.ver-date {
		font-size: 0.8125rem;
		color: var(--text-secondary);
		flex-shrink: 0;
	}
</style>
