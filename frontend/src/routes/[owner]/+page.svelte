<script lang="ts">
	import { page } from '$app/state';
	import { auth } from '$lib/auth.svelte';
	import { listProjects } from '$lib/api';
	import type { ProjectInfo } from '$lib/types';
	import { relativeTime } from '$lib/utils';

	let loading = $state(true);
	let error: string | null = $state(null);
	let projects: ProjectInfo[] = $state([]);

	let owner = $derived(page.params.owner ?? '');
	let isOwnProfile = $derived(auth.user?.username === owner);

	function avatarInitial(username: string): string {
		return username.charAt(0).toUpperCase();
	}

	$effect(() => {
		if (!owner) return;
		const currentOwner = owner;

		loading = true;
		error = null;

		listProjects(owner)
			.then((data) => {
				if (currentOwner !== owner) return;
				projects = data;
				loading = false;
			})
			.catch((err) => {
				if (currentOwner !== owner) return;
				error = err.message || 'Failed to load projects';
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{owner} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<!-- Profile header -->
	<header class="profile-header">
		<div class="avatar-wrapper">
			{#if isOwnProfile && auth.user?.avatar_url}
				<img
					class="avatar"
					src={auth.user.avatar_url}
					alt="{owner}'s avatar"
				/>
			{:else}
				<div class="avatar avatar-fallback">
					{avatarInitial(owner)}
				</div>
			{/if}
		</div>
		<div class="profile-info">
			<h1 class="username">{owner}</h1>
			{#if isOwnProfile}
				<p class="profile-label">Your profile</p>
			{/if}
		</div>
	</header>

	<!-- Projects section -->
	<section class="projects-section">
		<h2 class="section-title">Projects</h2>

		{#if loading}
			<div class="loading-state">
				<div class="spinner"></div>
				<p class="loading-text">Loading projects...</p>
			</div>
		{:else if error}
			<div class="empty-state">
				<h3>Something went wrong</h3>
				<p>{error}</p>
				<button class="btn btn-outline" onclick={() => location.reload()}>Try again</button>
			</div>
		{:else if projects.length === 0}
			<div class="empty-state">
				{#if isOwnProfile}
					<h3>No projects yet</h3>
					<p>
						You haven't pushed any projects to OzzyDB yet.
						Use the CLI to create and push your first project.
					</p>
					<div class="empty-state-code">
						<code>ozzy init my-project && ozzy push</code>
					</div>
				{:else}
					<h3>No public projects</h3>
					<p>{owner} hasn't published any public projects yet.</p>
				{/if}
			</div>
		{:else}
			<div class="projects-grid">
				{#each projects as project (project.slug)}
					<a class="project-card" href="/{project.owner}/{project.slug}">
						<div class="project-card-header">
							<h3 class="project-name">{project.slug}</h3>
							<span class="badge {project.visibility === 'public' ? 'badge-public' : 'badge-private'}">
								{project.visibility}
							</span>
						</div>
						{#if project.description}
							<p class="project-description">{project.description}</p>
						{/if}
						<div class="project-meta">
							<span class="project-time">Updated {relativeTime(project.updated_at)}</span>
						</div>
					</a>
				{/each}
			</div>
		{/if}
	</section>
</div>

<style>
	/* -- Page layout ---------------------------------------- */

	.page {
		padding-top: var(--space-xl);
		padding-bottom: var(--space-3xl);
	}

	/* -- Profile header ------------------------------------- */

	.profile-header {
		display: flex;
		align-items: center;
		gap: var(--space-lg);
		padding-bottom: var(--space-xl);
		border-bottom: 1px solid var(--border);
	}

	.avatar-wrapper {
		flex-shrink: 0;
	}

	.avatar {
		width: 80px;
		height: 80px;
		border-radius: 50%;
		object-fit: cover;
		border: 2px solid var(--border);
	}

	.avatar-fallback {
		display: flex;
		align-items: center;
		justify-content: center;
		background: var(--bg-inset);
		color: var(--text-secondary);
		font-size: 32px;
		font-weight: 700;
		letter-spacing: -0.02em;
	}

	.profile-info {
		display: flex;
		flex-direction: column;
		gap: var(--space-xs);
	}

	.username {
		font-size: 1.75rem;
		font-weight: 700;
		letter-spacing: -0.02em;
		color: var(--text);
		line-height: 1.2;
	}

	.profile-label {
		font-size: 14px;
		color: var(--text-muted);
	}

	/* -- Projects section ----------------------------------- */

	.projects-section {
		margin-top: var(--space-xl);
	}

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-lg);
		color: var(--text);
	}

	/* -- Loading state -------------------------------------- */

	.loading-state {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		padding: var(--space-3xl) 0;
		gap: var(--space-md);
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
		to {
			transform: rotate(360deg);
		}
	}

	.loading-text {
		color: var(--text-secondary);
	}

	/* -- Empty state ---------------------------------------- */

	.empty-state {
		text-align: center;
		padding: var(--space-2xl) var(--space-lg);
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
	}

	.empty-state h3 {
		font-size: 1.125rem;
		font-weight: 600;
		margin-bottom: var(--space-sm);
	}

	.empty-state p {
		color: var(--text-secondary);
		margin-bottom: var(--space-lg);
		max-width: 420px;
		margin-left: auto;
		margin-right: auto;
	}

	.empty-state-code {
		display: inline-block;
		padding: var(--space-sm) var(--space-md);
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		font-family: var(--font-mono);
		font-size: 13px;
		color: var(--text);
	}

	/* -- Projects grid -------------------------------------- */

	.projects-grid {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: var(--space-md);
	}

	.project-card {
		display: flex;
		flex-direction: column;
		padding: var(--space-md);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text);
		transition: border-color 0.15s, box-shadow 0.15s;
	}

	.project-card:hover {
		border-color: var(--border-strong);
		box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
		text-decoration: none;
	}

	.project-card-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-sm);
	}

	.project-name {
		font-size: 15px;
		font-weight: 600;
		color: var(--link);
	}

	.project-card:hover .project-name {
		color: var(--link-hover);
	}

	.project-description {
		margin-top: var(--space-sm);
		font-size: 13px;
		color: var(--text-secondary);
		line-height: 1.5;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}

	.project-meta {
		margin-top: auto;
		padding-top: var(--space-sm);
	}

	.project-time {
		font-size: 12px;
		color: var(--text-muted);
	}

	/* -- Responsive ----------------------------------------- */

	@media (max-width: 768px) {
		.profile-header {
			gap: var(--space-md);
		}

		.avatar {
			width: 64px;
			height: 64px;
			font-size: 26px;
		}

		.username {
			font-size: 1.5rem;
		}

		.projects-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
