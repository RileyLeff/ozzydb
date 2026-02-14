<script lang="ts">
	import { auth } from '$lib/auth.svelte';
	import { adminListJobs, adminCancelJob, adminGetRateLimits, adminListUsers } from '$lib/api';
	import type { AdminJob, AdminRateLimits, AdminUser } from '$lib/types';
	import { relativeTime } from '$lib/utils';
	import { goto } from '$app/navigation';

	let activeTab: 'jobs' | 'users' | 'limits' = $state('jobs');

	// Jobs state
	let jobs: AdminJob[] = $state([]);
	let jobsLoading = $state(true);
	let jobsError: string | null = $state(null);
	let jobStatusFilter: string = $state('');

	// Users state
	let users: AdminUser[] = $state([]);
	let usersLoading = $state(true);
	let usersError: string | null = $state(null);

	// Rate limits state
	let rateLimits: AdminRateLimits | null = $state(null);
	let limitsLoading = $state(true);
	let limitsError: string | null = $state(null);

	// Cancelling state
	let cancellingJob: string | null = $state(null);

	$effect(() => {
		if (!auth.isLoggedIn || !auth.user?.is_admin) {
			goto('/');
		}
	});

	async function loadJobs() {
		jobsLoading = true;
		jobsError = null;
		try {
			jobs = await adminListJobs(jobStatusFilter || undefined, 100);
		} catch (err: unknown) {
			jobsError = err instanceof Error ? err.message : 'Failed to load jobs';
		} finally {
			jobsLoading = false;
		}
	}

	async function loadUsers() {
		usersLoading = true;
		usersError = null;
		try {
			users = await adminListUsers(100);
		} catch (err: unknown) {
			usersError = err instanceof Error ? err.message : 'Failed to load users';
		} finally {
			usersLoading = false;
		}
	}

	async function loadLimits() {
		limitsLoading = true;
		limitsError = null;
		try {
			rateLimits = await adminGetRateLimits();
		} catch (err: unknown) {
			limitsError = err instanceof Error ? err.message : 'Failed to load rate limits';
		} finally {
			limitsLoading = false;
		}
	}

	async function handleCancel(jobId: string) {
		cancellingJob = jobId;
		try {
			await adminCancelJob(jobId);
			await loadJobs();
		} catch (err: unknown) {
			jobsError = err instanceof Error ? err.message : 'Failed to cancel job';
		} finally {
			cancellingJob = null;
		}
	}

	// Load data for active tab
	$effect(() => {
		if (activeTab === 'jobs') loadJobs();
		else if (activeTab === 'users') loadUsers();
		else if (activeTab === 'limits') loadLimits();
	});

	// Auto-refresh jobs every 10 seconds when on jobs tab
	$effect(() => {
		if (activeTab !== 'jobs') return;
		const interval = setInterval(loadJobs, 10_000);
		return () => clearInterval(interval);
	});

	function statusClass(status: string): string {
		switch (status) {
			case 'running':
				return 'status-running';
			case 'queued':
				return 'status-queued';
			case 'done':
				return 'status-done';
			case 'failed':
				return 'status-failed';
			default:
				return '';
		}
	}
</script>

<svelte:head>
	<title>Admin - OzzyDB</title>
</svelte:head>

{#if auth.user?.is_admin}
	<div class="container page">
		<header class="admin-header">
			<h1>Admin Dashboard</h1>
		</header>

		<div class="tabs">
			<button
				class="tab"
				class:active={activeTab === 'jobs'}
				onclick={() => (activeTab = 'jobs')}
			>
				Jobs
			</button>
			<button
				class="tab"
				class:active={activeTab === 'users'}
				onclick={() => (activeTab = 'users')}
			>
				Users
			</button>
			<button
				class="tab"
				class:active={activeTab === 'limits'}
				onclick={() => (activeTab = 'limits')}
			>
				Rate Limits
			</button>
		</div>

		{#if activeTab === 'jobs'}
			<div class="panel">
				<div class="toolbar">
					<select bind:value={jobStatusFilter}>
						<option value="">All statuses</option>
						<option value="queued">Queued</option>
						<option value="running">Running</option>
						<option value="done">Done</option>
						<option value="failed">Failed</option>
					</select>
				</div>

				{#if jobsLoading && jobs.length === 0}
					<div class="loading"><div class="spinner"></div></div>
				{:else if jobsError}
					<div class="error-state"><p class="error-msg">{jobsError}</p></div>
				{:else if jobs.length === 0}
					<p class="empty">No jobs found.</p>
				{:else}
					<div class="table-wrap">
						<table>
							<thead>
								<tr>
									<th>ID</th>
									<th>Endpoint</th>
									<th>Status</th>
									<th>Created</th>
									<th>Duration</th>
									<th></th>
								</tr>
							</thead>
							<tbody>
								{#each jobs as job (job.id)}
									<tr>
										<td class="mono">{job.id.slice(0, 8)}</td>
										<td>{job.endpoint_name}</td>
										<td>
											<span class="status-badge {statusClass(job.status)}">
												{job.status}
											</span>
										</td>
										<td class="muted">{relativeTime(job.created_at)}</td>
										<td class="muted">
											{#if job.completed_at && job.started_at}
												{Math.round(
													(new Date(job.completed_at).getTime() -
														new Date(job.started_at).getTime()) /
														1000
												)}s
											{:else if job.started_at}
												running...
											{:else}
												-
											{/if}
										</td>
										<td>
											{#if job.status === 'queued'}
												<button
													class="btn-cancel"
													disabled={cancellingJob === job.id}
													onclick={() => handleCancel(job.id)}
												>
													{cancellingJob === job.id ? 'Cancelling...' : 'Cancel'}
												</button>
											{/if}
										</td>
									</tr>
									{#if job.error_message}
										<tr class="error-row">
											<td colspan="6" class="error-detail">{job.error_message}</td>
										</tr>
									{/if}
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>
		{/if}

		{#if activeTab === 'users'}
			<div class="panel">
				{#if usersLoading}
					<div class="loading"><div class="spinner"></div></div>
				{:else if usersError}
					<div class="error-state"><p class="error-msg">{usersError}</p></div>
				{:else if users.length === 0}
					<p class="empty">No users found.</p>
				{:else}
					<div class="table-wrap">
						<table>
							<thead>
								<tr>
									<th>User</th>
									<th>Email</th>
									<th>Admin</th>
									<th>Joined</th>
								</tr>
							</thead>
							<tbody>
								{#each users as u (u.id)}
									<tr>
										<td>
											<div class="user-cell">
												{#if u.avatar_url}
													<img
														src={u.avatar_url}
														alt={u.username}
														class="user-avatar"
													/>
												{/if}
												<a href="/{u.username}">{u.username}</a>
											</div>
										</td>
										<td class="muted">{u.email ?? '-'}</td>
										<td>
											{#if u.is_admin}
												<span class="admin-badge">admin</span>
											{/if}
										</td>
										<td class="muted">{relativeTime(u.created_at)}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>
		{/if}

		{#if activeTab === 'limits'}
			<div class="panel">
				{#if limitsLoading}
					<div class="loading"><div class="spinner"></div></div>
				{:else if limitsError}
					<div class="error-state"><p class="error-msg">{limitsError}</p></div>
				{:else if rateLimits}
					<div class="limits-grid">
						<div class="limit-card">
							<div class="limit-label">Active Jobs</div>
							<div class="limit-value">{rateLimits.active_jobs_global}</div>
						</div>
						<div class="limit-card">
							<div class="limit-label">Global Max Concurrent</div>
							<div class="limit-value">
								{rateLimits.global_max_concurrent === 0
									? 'Unlimited'
									: rateLimits.global_max_concurrent}
							</div>
						</div>
						<div class="limit-card">
							<div class="limit-label">Per-User Max Concurrent</div>
							<div class="limit-value">
								{rateLimits.per_user_max_concurrent === 0
									? 'Unlimited'
									: rateLimits.per_user_max_concurrent}
							</div>
						</div>
					</div>
					<p class="limits-note">
						Rate limits are configured via environment variables on the server.
					</p>
				{/if}
			</div>
		{/if}
	</div>
{/if}

<style>
	.container {
		max-width: var(--max-width);
		margin: 0 auto;
		padding: var(--space-lg);
	}

	.page {
		padding-top: var(--space-xl);
	}

	.admin-header {
		margin-bottom: var(--space-lg);
	}

	.admin-header h1 {
		font-size: 24px;
		font-weight: 700;
	}

	.tabs {
		display: flex;
		gap: 0;
		border-bottom: 1px solid var(--border);
		margin-bottom: var(--space-lg);
	}

	.tab {
		padding: var(--space-sm) var(--space-md);
		font-size: 14px;
		font-weight: 500;
		color: var(--text-secondary);
		border-bottom: 2px solid transparent;
		cursor: pointer;
		transition:
			color 0.15s,
			border-color 0.15s;
	}

	.tab:hover {
		color: var(--text);
	}

	.tab.active {
		color: var(--text);
		border-bottom-color: var(--pink);
	}

	.panel {
		min-height: 200px;
	}

	.toolbar {
		margin-bottom: var(--space-md);
	}

	.toolbar select {
		padding: 6px 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-size: 14px;
		background: var(--bg);
		color: var(--text);
	}

	.table-wrap {
		overflow-x: auto;
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 14px;
	}

	th {
		text-align: left;
		padding: var(--space-sm) var(--space-md);
		font-weight: 600;
		font-size: 12px;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-secondary);
		border-bottom: 1px solid var(--border);
	}

	td {
		padding: var(--space-sm) var(--space-md);
		border-bottom: 1px solid var(--border);
		vertical-align: middle;
	}

	.mono {
		font-family: var(--font-mono);
		font-size: 13px;
	}

	.muted {
		color: var(--text-secondary);
	}

	.status-badge {
		display: inline-block;
		padding: 2px 8px;
		border-radius: 12px;
		font-size: 12px;
		font-weight: 600;
	}

	.status-running {
		background: color-mix(in srgb, var(--pink) 15%, transparent);
		color: var(--pink);
	}

	.status-queued {
		background: color-mix(in srgb, var(--gray-500) 15%, transparent);
		color: var(--gray-600);
	}

	.status-done {
		background: color-mix(in srgb, var(--green) 15%, transparent);
		color: var(--green-dim);
	}

	.status-failed {
		background: color-mix(in srgb, var(--error) 15%, transparent);
		color: var(--error);
	}

	.btn-cancel {
		padding: 4px 10px;
		font-size: 12px;
		border: 1px solid var(--error);
		border-radius: var(--radius);
		color: var(--error);
		background: transparent;
		cursor: pointer;
		transition: background 0.15s;
	}

	.btn-cancel:hover:not(:disabled) {
		background: color-mix(in srgb, var(--error) 10%, transparent);
	}

	.btn-cancel:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.error-row td {
		border-bottom: 1px solid var(--border);
		padding-top: 0;
	}

	.error-detail {
		font-size: 12px;
		color: var(--error);
		font-family: var(--font-mono);
	}

	.user-cell {
		display: flex;
		align-items: center;
		gap: var(--space-sm);
	}

	.user-avatar {
		width: 24px;
		height: 24px;
		border-radius: 50%;
	}

	.admin-badge {
		display: inline-block;
		padding: 2px 8px;
		border-radius: 12px;
		font-size: 12px;
		font-weight: 600;
		background: color-mix(in srgb, var(--pink) 15%, transparent);
		color: var(--pink);
	}

	.limits-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
		gap: var(--space-md);
	}

	.limit-card {
		padding: var(--space-lg);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		text-align: center;
	}

	.limit-label {
		font-size: 13px;
		color: var(--text-secondary);
		margin-bottom: var(--space-sm);
	}

	.limit-value {
		font-size: 28px;
		font-weight: 700;
		color: var(--text);
	}

	.limits-note {
		margin-top: var(--space-lg);
		font-size: 13px;
		color: var(--text-muted);
	}

	.loading {
		display: flex;
		justify-content: center;
		padding: var(--space-2xl);
	}

	.spinner {
		width: 24px;
		height: 24px;
		border: 2px solid var(--border);
		border-top-color: var(--pink);
		border-radius: 50%;
		animation: spin 0.6s linear infinite;
	}

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	.error-state {
		padding: var(--space-xl);
		text-align: center;
	}

	.error-msg {
		color: var(--error);
	}

	.empty {
		text-align: center;
		padding: var(--space-xl);
		color: var(--text-secondary);
	}
</style>
