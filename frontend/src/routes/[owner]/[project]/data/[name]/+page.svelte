<script lang="ts">
	import { page } from '$app/state';
	import { auth } from '$lib/auth.svelte';
	import { getDataAtom, downloadData, yankData } from '$lib/api';
	import type { DataAtomDetail } from '$lib/types';
	import { formatBytes, relativeTime, shortHash } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');
	let name = $derived(page.params.name ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let atom: DataAtomDetail | null = $state(null);
	let yanking = $state(false);
	let yankReason = $state('');
	let showYankConfirm = $state(false);

	$effect(() => {
		if (!owner || !slug || !name) return;
		const cur = { owner, slug, name };

		loading = true;
		error = null;

		getDataAtom(owner, slug, name)
			.then((data) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.name !== name) return;
				atom = data;
				loading = false;
			})
			.catch((err) => {
				if (cur.owner !== owner || cur.slug !== slug || cur.name !== name) return;
				error = err.message || 'Failed to load data atom';
				loading = false;
			});
	});

	async function handleDownload() {
		try {
			const res = await downloadData(owner, slug, name);
			const blob = await res.blob();
			const url = URL.createObjectURL(blob);
			const a = document.createElement('a');
			a.href = url;
			a.download = name;
			a.click();
			URL.revokeObjectURL(url);
		} catch (err: unknown) {
			const msg = err instanceof Error ? err.message : 'Download failed';
			alert(msg);
		}
	}

	async function handleYank() {
		if (!yankReason.trim()) return;
		yanking = true;
		try {
			await yankData(owner, slug, name, yankReason);
			showYankConfirm = false;
			yankReason = '';
			// Refresh
			atom = await getDataAtom(owner, slug, name);
		} catch (err: unknown) {
			const msg = err instanceof Error ? err.message : 'Failed to yank';
			alert(msg);
		} finally {
			yanking = false;
		}
	}
</script>

<svelte:head>
	<title>{name} - Data - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}/data">Data</a>
			<span class="sep">/</span>
			<span class="current">{name}</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if loading}
		<div class="loading"><div class="spinner"></div></div>
	{:else if error}
		<div class="error-state"><p class="error-msg">{error}</p></div>
	{:else if atom}
		{#if atom.yanked}
			<div class="yank-banner">
				This data atom has been yanked{atom.yank_reason ? `: ${atom.yank_reason}` : ''}.
			</div>
		{/if}

		<div class="detail-grid">
			<div class="detail-row">
				<span class="label">Name</span>
				<span class="value">{atom.name}</span>
			</div>
			<div class="detail-row">
				<span class="label">Hash</span>
				<code class="value hash">{atom.hash}</code>
			</div>
			<div class="detail-row">
				<span class="label">Content type</span>
				<span class="value">{atom.content_type}</span>
			</div>
			<div class="detail-row">
				<span class="label">Size</span>
				<span class="value">{formatBytes(atom.byte_size)}</span>
			</div>
			<div class="detail-row">
				<span class="label">Uploaded</span>
				<span class="value">{relativeTime(atom.created_at)}</span>
			</div>
			<div class="detail-row">
				<span class="label">Status</span>
				<span class="value">
					{#if atom.yanked}
						<span class="badge badge-yanked">yanked</span>
					{:else}
						<span class="badge badge-active">active</span>
					{/if}
				</span>
			</div>
		</div>

		<!-- Metadata -->
		{#if Object.keys(atom.metadata).length > 0}
			<section class="section">
				<h2 class="section-title">Metadata</h2>
				<div class="metadata-list">
					{#each Object.entries(atom.metadata) as [key, value]}
						<div class="metadata-row">
							<span class="meta-key">{key}</span>
							<code class="meta-value">{JSON.stringify(value)}</code>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		<!-- Actions -->
		<div class="actions">
			{#if !atom.yanked}
				<button class="btn btn-primary" onclick={handleDownload}>Download</button>
			{/if}
			{#if auth.isLoggedIn && !atom.yanked}
				<button class="btn btn-outline btn-danger" onclick={() => { showYankConfirm = true; }}>
					Yank
				</button>
			{/if}
		</div>

		<!-- Yank confirm -->
		{#if showYankConfirm}
			<div class="yank-modal">
				<div class="yank-dialog">
					<h3>Yank "{name}"?</h3>
					<p>Yanked data atoms cannot be used in new collections or computation. This cannot be undone.</p>
					<input
						type="text"
						bind:value={yankReason}
						placeholder="Reason for yanking..."
						class="yank-input"
					/>
					<div class="yank-actions">
						<button class="btn btn-outline" onclick={() => { showYankConfirm = false; }}>
							Cancel
						</button>
						<button
							class="btn btn-danger"
							disabled={yanking || !yankReason.trim()}
							onclick={handleYank}
						>
							{yanking ? 'Yanking...' : 'Confirm yank'}
						</button>
					</div>
				</div>
			</div>
		{/if}
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

	.hash {
		font-size: 0.8125rem;
	}

	.section {
		margin-bottom: var(--space-xl);
	}

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
	}

	.metadata-list {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.metadata-row {
		display: grid;
		grid-template-columns: 200px 1fr;
		border-bottom: 1px solid var(--border);
		font-size: 0.875rem;
	}

	.metadata-row:last-child {
		border-bottom: none;
	}

	.meta-key {
		padding: var(--space-sm) var(--space-md);
		font-weight: 500;
		background: var(--bg-secondary);
	}

	.meta-value {
		padding: var(--space-sm) var(--space-md);
		word-break: break-all;
		font-size: 0.8125rem;
	}

	.actions {
		display: flex;
		gap: var(--space-sm);
	}

	.btn-danger {
		color: var(--error);
		border-color: var(--error);
	}

	.btn-danger:hover {
		background: color-mix(in srgb, var(--error) 8%, transparent);
	}

	.badge-yanked {
		background: color-mix(in srgb, var(--error) 12%, transparent);
		color: var(--error);
	}

	.badge-active {
		background: color-mix(in srgb, var(--green) 12%, transparent);
		color: var(--green);
	}

	/* Yank modal */
	.yank-modal {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 200;
	}

	.yank-dialog {
		background: var(--bg);
		border-radius: var(--radius-lg);
		padding: var(--space-lg);
		max-width: 400px;
		width: 90%;
	}

	.yank-dialog h3 {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-sm);
	}

	.yank-dialog p {
		font-size: 0.875rem;
		color: var(--text-secondary);
		margin-bottom: var(--space-md);
	}

	.yank-input {
		width: 100%;
		padding: var(--space-sm);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 0.875rem;
		margin-bottom: var(--space-md);
	}

	.yank-actions {
		display: flex;
		gap: var(--space-sm);
		justify-content: flex-end;
	}
</style>
