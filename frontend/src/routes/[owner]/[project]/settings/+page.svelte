<script lang="ts">
	import { page } from '$app/state';
	import { auth } from '$lib/auth.svelte';
	import { listSecrets, setSecret, deleteSecret } from '$lib/api';
	import type { SecretInfo } from '$lib/types';
	import { relativeTime } from '$lib/utils';
	import ProjectTabs from '$lib/components/ProjectTabs.svelte';

	let owner = $derived(page.params.owner ?? '');
	let slug = $derived(page.params.project ?? '');

	let loading = $state(true);
	let error: string | null = $state(null);
	let secrets: SecretInfo[] = $state([]);

	// Add form
	let newName = $state('');
	let newValue = $state('');
	let adding = $state(false);
	let addError: string | null = $state(null);

	// Delete confirm
	let deletingName: string | null = $state(null);
	let deleting = $state(false);

	$effect(() => {
		if (!owner || !slug) return;
		const cur = { owner, slug };

		loading = true;
		error = null;

		listSecrets(owner, slug)
			.then((data) => {
				if (cur.owner !== owner || cur.slug !== slug) return;
				secrets = data;
				loading = false;
			})
			.catch((err) => {
				if (cur.owner !== owner || cur.slug !== slug) return;
				error = err.message || 'Failed to load secrets';
				loading = false;
			});
	});

	async function handleAdd() {
		if (!newName.trim() || !newValue.trim()) return;
		adding = true;
		addError = null;

		try {
			await setSecret(owner, slug, newName, newValue);
			newName = '';
			newValue = '';
			// Refresh list
			secrets = await listSecrets(owner, slug);
		} catch (err: unknown) {
			addError = err instanceof Error ? err.message : 'Failed to set secret';
		} finally {
			adding = false;
		}
	}

	async function handleDelete(name: string) {
		deleting = true;
		try {
			await deleteSecret(owner, slug, name);
			deletingName = null;
			secrets = await listSecrets(owner, slug);
		} catch (err: unknown) {
			const msg = err instanceof Error ? err.message : 'Failed to delete';
			alert(msg);
		} finally {
			deleting = false;
		}
	}
</script>

<svelte:head>
	<title>Settings - {owner}/{slug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="sep">/</span>
			<a href="/{owner}/{slug}">{slug}</a>
			<span class="sep">/</span>
			<span class="current">Settings</span>
		</div>
	</header>

	<ProjectTabs {owner} project={slug} />

	{#if !auth.isLoggedIn}
		<div class="empty-state">
			<p>Log in to manage project settings.</p>
		</div>
	{:else}
		<section class="section">
			<h2 class="section-title">Secrets</h2>
			<p class="section-desc">
				Secrets are encrypted environment variables available to transforms at runtime.
				Values are write-only and cannot be retrieved after creation.
			</p>

			{#if loading}
				<div class="loading"><div class="spinner"></div></div>
			{:else if error}
				<div class="error-state"><p class="error-msg">{error}</p></div>
			{:else}
				<!-- Secrets list -->
				{#if secrets.length > 0}
					<div class="table-wrap">
						<table>
							<thead>
								<tr>
									<th>Name</th>
									<th>Updated</th>
									<th></th>
								</tr>
							</thead>
							<tbody>
								{#each secrets as secret (secret.name)}
									<tr>
										<td class="secret-name"><code>{secret.name}</code></td>
										<td class="date-cell">{relativeTime(secret.updated_at)}</td>
										<td class="action-cell">
											{#if deletingName === secret.name}
												<div class="delete-confirm">
													<span class="delete-prompt">Delete?</span>
													<button
														class="btn btn-danger btn-xs"
														onclick={() => handleDelete(secret.name)}
														disabled={deleting}
													>
														{deleting ? '...' : 'Yes'}
													</button>
													<button
														class="btn btn-outline btn-xs"
														onclick={() => { deletingName = null; }}
													>
														No
													</button>
												</div>
											{:else}
												<button
													class="btn btn-outline btn-xs btn-danger-text"
													onclick={() => { deletingName = secret.name; }}
												>
													Delete
												</button>
											{/if}
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{:else}
					<p class="no-secrets">No secrets configured.</p>
				{/if}

				<!-- Add form -->
				<div class="add-form">
					<h3 class="form-title">Add secret</h3>
					{#if addError}
						<p class="error-msg">{addError}</p>
					{/if}
					<div class="form-row">
						<input
							type="text"
							class="form-input"
							bind:value={newName}
							placeholder="SECRET_NAME"
							pattern="[A-Za-z_][A-Za-z0-9_]*"
						/>
						<input
							type="password"
							class="form-input form-input-wide"
							bind:value={newValue}
							placeholder="Value"
						/>
						<button
							class="btn btn-primary"
							onclick={handleAdd}
							disabled={adding || !newName.trim() || !newValue.trim()}
						>
							{adding ? 'Adding...' : 'Add'}
						</button>
					</div>
				</div>
			{/if}
		</section>
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
		padding: var(--space-2xl) 0;
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

	.error-msg {
		color: var(--error);
		font-size: 0.875rem;
		margin-bottom: var(--space-sm);
	}

	.section { margin-bottom: var(--space-xl); }

	.section-title {
		font-size: 1.125rem;
		font-weight: 600;
		margin-bottom: var(--space-xs);
	}

	.section-desc {
		color: var(--text-secondary);
		font-size: 0.875rem;
		margin-bottom: var(--space-lg);
		max-width: 600px;
	}

	.no-secrets {
		color: var(--text-muted);
		font-size: 0.875rem;
		padding: var(--space-md) 0;
	}

	.table-wrap {
		overflow-x: auto;
		margin-bottom: var(--space-xl);
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

	tr:hover { background: var(--bg-secondary); }

	.secret-name code {
		font-size: 0.8125rem;
		font-weight: 500;
	}

	.date-cell {
		color: var(--text-secondary);
		white-space: nowrap;
	}

	.action-cell {
		text-align: right;
		white-space: nowrap;
	}

	.delete-confirm {
		display: flex;
		align-items: center;
		gap: var(--space-xs);
		justify-content: flex-end;
	}

	.delete-prompt {
		font-size: 0.8125rem;
		color: var(--error);
	}

	.btn-xs {
		padding: 2px var(--space-xs);
		font-size: 0.75rem;
	}

	.btn-danger-text { color: var(--error); }
	.btn-danger-text:hover {
		background: color-mix(in srgb, var(--error) 8%, transparent);
	}

	.btn-danger {
		color: var(--white);
		background: var(--error);
		border-color: var(--error);
	}

	/* Add form */
	.add-form {
		padding: var(--space-lg);
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius);
	}

	.form-title {
		font-size: 0.9375rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
	}

	.form-row {
		display: flex;
		gap: var(--space-sm);
		align-items: flex-start;
	}

	.form-input {
		padding: var(--space-sm);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 0.875rem;
		background: var(--bg);
		color: var(--text);
	}

	.form-input:focus {
		border-color: var(--pink);
		outline: none;
		box-shadow: 0 0 0 2px color-mix(in srgb, var(--pink) 20%, transparent);
	}

	.form-input-wide { flex: 1; }

	@media (max-width: 768px) {
		.form-row {
			flex-direction: column;
		}

		.form-input {
			width: 100%;
		}
	}
</style>
