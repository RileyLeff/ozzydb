<script lang="ts">
	import { page } from '$app/state';
	import {
		getProject,
		getDagSvg,
		listEndpoints,
		listTransforms,
		listCommits,
		listRefs,
		listCollaborators
	} from '$lib/api';
	import type {
		ProjectInfo,
		EndpointInfo,
		TransformInfo,
		CommitInfo,
		ListRefsResponse,
		CollaboratorInfo
	} from '$lib/types';
	import CodeSnippet from '$lib/components/CodeSnippet.svelte';

	type Tab = 'overview' | 'endpoints' | 'transforms' | 'commits';

	let activeTab: Tab = $state('overview');
	let loading = $state(true);
	let error: string | null = $state(null);

	let project: ProjectInfo | null = $state(null);
	let dagSvg: string | null = $state(null);
	let endpoints: EndpointInfo[] = $state([]);
	let transforms: TransformInfo[] = $state([]);
	let commits: CommitInfo[] = $state([]);
	let refs: ListRefsResponse | null = $state(null);
	let collaborators: CollaboratorInfo[] = $state([]);

	let owner = $derived(page.params.owner);
	let projectSlug = $derived(page.params.project);
	let firstEndpointName = $derived(endpoints.length > 0 ? endpoints[0].name : null);

	function relativeTime(iso: string): string {
		const now = Date.now();
		const then = new Date(iso).getTime();
		const seconds = Math.floor((now - then) / 1000);

		if (seconds < 60) return 'just now';
		const minutes = Math.floor(seconds / 60);
		if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'} ago`;
		const hours = Math.floor(minutes / 60);
		if (hours < 24) return `${hours} hour${hours === 1 ? '' : 's'} ago`;
		const days = Math.floor(hours / 24);
		if (days < 30) return `${days} day${days === 1 ? '' : 's'} ago`;
		const months = Math.floor(days / 30);
		if (months < 12) return `${months} month${months === 1 ? '' : 's'} ago`;
		const years = Math.floor(months / 12);
		return `${years} year${years === 1 ? '' : 's'} ago`;
	}

	function permissionLabel(perm: string): string {
		switch (perm) {
			case 'admin':
				return 'Admin';
			case 'write':
				return 'Write';
			case 'read':
				return 'Read';
			default:
				return perm;
		}
	}

	$effect(() => {
		const currentOwner = owner;
		const currentProject = projectSlug;

		if (!currentOwner || !currentProject) return;

		loading = true;
		error = null;

		Promise.all([
			getProject(currentOwner, currentProject),
			getDagSvg(currentOwner, currentProject).catch(() => null),
			listEndpoints(currentOwner, currentProject),
			listTransforms(currentOwner, currentProject),
			listCommits(currentOwner, currentProject),
			listRefs(currentOwner, currentProject),
			listCollaborators(currentOwner, currentProject)
		])
			.then(
				([
					projectData,
					dagData,
					endpointsData,
					transformsData,
					commitsData,
					refsData,
					collaboratorsData
				]) => {
					if (currentOwner !== owner || currentProject !== projectSlug) return;
					project = projectData;
					dagSvg = dagData;
					endpoints = endpointsData.endpoints;
					transforms = transformsData.transforms;
					commits = commitsData;
					refs = refsData;
					collaborators = collaboratorsData;
					loading = false;
				}
			)
			.catch((err) => {
				if (currentOwner !== owner || currentProject !== projectSlug) return;
				if (err.status === 404) {
					error = 'not_found';
				} else {
					error = err.message || 'Failed to load project';
				}
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{project ? `${project.owner}/${project.slug}` : `${owner}/${projectSlug}`} - OzzyDB</title>
</svelte:head>

<div class="container page">
	{#if loading}
		<div class="loading-state">
			<div class="spinner"></div>
			<p class="loading-text">Loading project...</p>
		</div>
	{:else if error === 'not_found'}
		<div class="error-state">
			<h2>Project not found</h2>
			<p>The project <span class="mono">{owner}/{projectSlug}</span> does not exist or you do not have access to it.</p>
			<a href="/" class="btn btn-outline">Back to home</a>
		</div>
	{:else if error}
		<div class="error-state">
			<h2>Something went wrong</h2>
			<p>{error}</p>
			<button class="btn btn-outline" onclick={() => location.reload()}>Try again</button>
		</div>
	{:else if project}
		<!-- Project header -->
		<header class="project-header">
			<div class="breadcrumb">
				<a href="/{project.owner}">{project.owner}</a>
				<span class="separator">/</span>
				<span class="project-name">{project.slug}</span>
				<span class="badge {project.visibility === 'public' ? 'badge-public' : 'badge-private'}">
					{project.visibility}
				</span>
			</div>
			{#if project.description}
				<p class="project-description">{project.description}</p>
			{/if}
		</header>

		<div class="project-layout">
			<!-- Main content -->
			<div class="main-content">
				<!-- Tabs -->
				<nav class="tabs">
					<button
						class="tab"
						class:active={activeTab === 'overview'}
						onclick={() => (activeTab = 'overview')}
					>
						Overview
					</button>
					<button
						class="tab"
						class:active={activeTab === 'endpoints'}
						onclick={() => (activeTab = 'endpoints')}
					>
						Endpoints
						{#if endpoints.length > 0}
							<span class="tab-count">{endpoints.length}</span>
						{/if}
					</button>
					<button
						class="tab"
						class:active={activeTab === 'transforms'}
						onclick={() => (activeTab = 'transforms')}
					>
						Transforms
						{#if transforms.length > 0}
							<span class="tab-count">{transforms.length}</span>
						{/if}
					</button>
					<button
						class="tab"
						class:active={activeTab === 'commits'}
						onclick={() => (activeTab = 'commits')}
					>
						Commits
						{#if commits.length > 0}
							<span class="tab-count">{commits.length}</span>
						{/if}
					</button>
				</nav>

				<!-- Tab panels -->
				<div class="tab-panel">
					{#if activeTab === 'overview'}
						<div class="overview-panel">
							{#if dagSvg}
								<div class="dag-container">
									<h3 class="section-title">Pipeline DAG</h3>
									<div class="dag-svg-wrapper">
										{@html dagSvg}
									</div>
								</div>
							{:else if endpoints.length === 0}
								<div class="empty-state">
									<h3>No endpoints yet</h3>
									<p>
										This project doesn't have any endpoints defined.
										Use the CLI to add data sources, transforms, and endpoints.
									</p>
									<CodeSnippet
										code={`ozzy data add my-data.parquet\nozzy transform add clean.py\nozzy endpoint create my-endpoint --pipeline "my-data -> clean"`}
										language="bash"
										title="Get started"
									/>
								</div>
							{:else}
								<div class="empty-state">
									<p>DAG visualization is not available for this project.</p>
								</div>
							{/if}

							{#if endpoints.length > 0}
								<div class="overview-section">
									<h3 class="section-title">Endpoints</h3>
									<ul class="overview-list">
										{#each endpoints as endpoint (endpoint.name)}
											<li>
												<a href="/{owner}/{projectSlug}/endpoint/{endpoint.name}">
													{endpoint.name}
												</a>
												{#if endpoint.description}
													<span class="text-secondary"> -- {endpoint.description}</span>
												{/if}
											</li>
										{/each}
									</ul>
								</div>
							{/if}

							{#if commits.length > 0}
								<div class="overview-section">
									<h3 class="section-title">Recent activity</h3>
									<ul class="overview-list">
										{#each commits.slice(0, 5) as commit (commit.hash)}
											<li class="commit-line">
												<span class="mono commit-hash">{commit.hash.slice(0, 7)}</span>
												<span class="truncate">{commit.message}</span>
												<span class="text-muted">{relativeTime(commit.created_at)}</span>
											</li>
										{/each}
									</ul>
								</div>
							{/if}
						</div>

					{:else if activeTab === 'endpoints'}
						<div class="endpoints-panel">
							{#if endpoints.length === 0}
								<div class="empty-state">
									<h3>No endpoints</h3>
									<p>This project has no endpoints defined yet.</p>
								</div>
							{:else}
								<div class="card-list">
									{#each endpoints as endpoint (endpoint.name)}
										<a class="card" href="/{owner}/{projectSlug}/endpoint/{endpoint.name}">
											<div class="card-header">
												<h4 class="card-title">{endpoint.name}</h4>
											</div>
											{#if endpoint.description}
												<p class="card-description">{endpoint.description}</p>
											{/if}
											<div class="card-meta">
												<span>{endpoint.definition.nodes.length} node{endpoint.definition.nodes.length === 1 ? '' : 's'}</span>
												<span class="dot-sep"></span>
												<span>{endpoint.definition.edges.length} edge{endpoint.definition.edges.length === 1 ? '' : 's'}</span>
											</div>
										</a>
									{/each}
								</div>
							{/if}
						</div>

					{:else if activeTab === 'transforms'}
						<div class="transforms-panel">
							{#if transforms.length === 0}
								<div class="empty-state">
									<h3>No transforms</h3>
									<p>This project has no transforms defined yet.</p>
								</div>
							{:else}
								<div class="card-list">
									{#each transforms as transform (transform.name)}
										<div class="card">
											<div class="card-header">
												<h4 class="card-title">{transform.name}</h4>
												<span class="badge badge-runtime">{transform.runtime}</span>
											</div>
											<div class="card-meta">
												<span class="mono">{transform.function_name}</span>
											</div>
										</div>
									{/each}
								</div>
							{/if}
						</div>

					{:else if activeTab === 'commits'}
						<div class="commits-panel">
							{#if commits.length === 0}
								<div class="empty-state">
									<h3>No commits</h3>
									<p>This project has no commits yet.</p>
								</div>
							{:else}
								<div class="commit-list">
									{#each commits as commit, i (commit.hash)}
										<div class="commit-row" class:alt={i % 2 === 1}>
											<span class="mono commit-hash">{commit.hash.slice(0, 7)}</span>
											<span class="commit-message truncate">{commit.message}</span>
											<span class="commit-author">{commit.author}</span>
											<span class="commit-time text-muted">{relativeTime(commit.created_at)}</span>
										</div>
									{/each}
								</div>
							{/if}
						</div>
					{/if}
				</div>
			</div>

			<!-- Sidebar -->
			<aside class="sidebar">
				<!-- Refs -->
				{#if refs && (refs.branches.length > 0 || refs.tags.length > 0)}
					<div class="sidebar-section">
						<h3 class="sidebar-title">Refs</h3>

						{#if refs.branches.length > 0}
							<div class="ref-group">
								<h4 class="ref-group-title">Branches</h4>
								<ul class="ref-list">
									{#each refs.branches as branch (branch.name)}
										<li class="ref-item">
											<span class="ref-name truncate">{branch.name}</span>
											<span class="mono ref-hash">{branch.commit_hash.slice(0, 7)}</span>
										</li>
									{/each}
								</ul>
							</div>
						{/if}

						{#if refs.tags.length > 0}
							<div class="ref-group">
								<h4 class="ref-group-title">Tags</h4>
								<ul class="ref-list">
									{#each refs.tags as tag (tag.name)}
										<li class="ref-item">
											<span class="ref-name truncate">{tag.name}</span>
											<span class="mono ref-hash">{tag.commit_hash.slice(0, 7)}</span>
										</li>
									{/each}
								</ul>
							</div>
						{/if}
					</div>
				{/if}

				<!-- Collaborators -->
				{#if collaborators.length > 0}
					<div class="sidebar-section">
						<h3 class="sidebar-title">Collaborators</h3>
						<ul class="collaborator-list">
							{#each collaborators as collab (collab.username)}
								<li class="collaborator-item">
									<span class="collaborator-name">{collab.username}</span>
									<span class="badge badge-permission">{permissionLabel(collab.permission)}</span>
								</li>
							{/each}
						</ul>
					</div>
				{/if}

				<!-- Usage -->
				<div class="sidebar-section">
					<h3 class="sidebar-title">Usage</h3>
					<CodeSnippet
						code={`ozzy fetch ${owner}/${projectSlug}${firstEndpointName ? `/${firstEndpointName}` : ''}`}
						language="bash"
						title="CLI"
					/>
				</div>
			</aside>
		</div>
	{/if}
</div>

<style>
	/* ── Page layout ────────────────────────────────────── */

	.page {
		padding-top: var(--space-xl);
		padding-bottom: var(--space-3xl);
	}

	.project-layout {
		display: grid;
		grid-template-columns: 1fr 280px;
		gap: var(--space-xl);
		margin-top: var(--space-lg);
	}

	/* ── Loading / Error states ─────────────────────────── */

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

	.error-state {
		text-align: center;
		padding: var(--space-3xl) 0;
	}

	.error-state h2 {
		font-size: 1.5rem;
		font-weight: 700;
		margin-bottom: var(--space-sm);
	}

	.error-state p {
		color: var(--text-secondary);
		margin-bottom: var(--space-lg);
	}

	/* ── Project header ─────────────────────────────────── */

	.project-header {
		padding-bottom: var(--space-lg);
		border-bottom: 1px solid var(--border);
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
	}

	.project-description {
		margin-top: var(--space-sm);
		color: var(--text-secondary);
	}

	/* ── Tabs ───────────────────────────────────────────── */

	.tabs {
		display: flex;
		gap: 0;
		border-bottom: 1px solid var(--border);
	}

	.tab {
		display: inline-flex;
		align-items: center;
		gap: var(--space-xs);
		padding: var(--space-sm) var(--space-md);
		font-size: 14px;
		font-weight: 500;
		color: var(--text-secondary);
		border-bottom: 2px solid transparent;
		margin-bottom: -1px;
		transition: color 0.15s, border-color 0.15s;
	}

	.tab:hover {
		color: var(--text);
	}

	.tab.active {
		color: var(--text);
		border-bottom-color: var(--pink);
	}

	.tab-count {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 20px;
		height: 20px;
		padding: 0 6px;
		font-size: 11px;
		font-weight: 600;
		background: var(--bg-inset);
		border-radius: 999px;
		color: var(--text-secondary);
	}

	/* ── Tab panels ─────────────────────────────────────── */

	.tab-panel {
		padding-top: var(--space-lg);
	}

	/* ── Overview panel ─────────────────────────────────── */

	.dag-container {
		margin-bottom: var(--space-xl);
	}

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
		color: var(--text);
	}

	.dag-svg-wrapper {
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: var(--space-lg);
		overflow-x: auto;
	}

	.dag-svg-wrapper :global(svg) {
		max-width: 100%;
		height: auto;
	}

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

	.overview-section {
		margin-top: var(--space-xl);
	}

	.overview-list {
		list-style: none;
	}

	.overview-list li {
		padding: var(--space-sm) 0;
		border-bottom: 1px solid var(--border);
	}

	.overview-list li:last-child {
		border-bottom: none;
	}

	.commit-line {
		display: flex;
		align-items: center;
		gap: var(--space-md);
	}

	.text-secondary {
		color: var(--text-secondary);
	}

	.text-muted {
		color: var(--text-muted);
		font-size: 12px;
		white-space: nowrap;
	}

	/* ── Card list (endpoints, transforms) ──────────────── */

	.card-list {
		display: flex;
		flex-direction: column;
		gap: var(--space-sm);
	}

	.card {
		display: block;
		padding: var(--space-md);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		transition: border-color 0.15s, box-shadow 0.15s;
		color: var(--text);
	}

	a.card:hover {
		border-color: var(--border-strong);
		box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
		text-decoration: none;
	}

	.card-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-sm);
	}

	.card-title {
		font-size: 14px;
		font-weight: 600;
	}

	.card-description {
		margin-top: var(--space-xs);
		color: var(--text-secondary);
		font-size: 13px;
	}

	.card-meta {
		display: flex;
		align-items: center;
		gap: var(--space-sm);
		margin-top: var(--space-sm);
		font-size: 12px;
		color: var(--text-muted);
	}

	.dot-sep::before {
		content: '\00b7';
		font-weight: 700;
	}

	.badge-runtime {
		background: color-mix(in srgb, var(--pink) 12%, transparent);
		color: var(--pink);
		border: 1px solid color-mix(in srgb, var(--pink) 25%, transparent);
	}

	/* ── Commits panel ──────────────────────────────────── */

	.commit-list {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.commit-row {
		display: grid;
		grid-template-columns: 72px 1fr auto auto;
		gap: var(--space-md);
		align-items: center;
		padding: var(--space-sm) var(--space-md);
	}

	.commit-row.alt {
		background: var(--bg-secondary);
	}

	.commit-hash {
		font-size: 12px;
		color: var(--link);
	}

	.commit-message {
		font-size: 13px;
	}

	.commit-author {
		font-size: 12px;
		color: var(--text-secondary);
		white-space: nowrap;
	}

	.commit-time {
		font-size: 12px;
	}

	/* ── Sidebar ────────────────────────────────────────── */

	.sidebar {
		display: flex;
		flex-direction: column;
		gap: var(--space-lg);
	}

	.sidebar-section {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: var(--space-md);
	}

	.sidebar-title {
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-secondary);
		margin-bottom: var(--space-md);
	}

	.ref-group {
		margin-bottom: var(--space-md);
	}

	.ref-group:last-child {
		margin-bottom: 0;
	}

	.ref-group-title {
		font-size: 12px;
		font-weight: 500;
		color: var(--text-muted);
		margin-bottom: var(--space-xs);
	}

	.ref-list {
		list-style: none;
	}

	.ref-item {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-sm);
		padding: 3px 0;
		font-size: 13px;
	}

	.ref-name {
		flex: 1;
		min-width: 0;
	}

	.ref-hash {
		font-size: 11px;
		color: var(--text-muted);
	}

	.collaborator-list {
		list-style: none;
	}

	.collaborator-item {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-sm);
		padding: 3px 0;
		font-size: 13px;
	}

	.collaborator-name {
		font-weight: 500;
	}

	.badge-permission {
		background: var(--bg-inset);
		color: var(--text-secondary);
		border: 1px solid var(--border);
		font-size: 11px;
	}

	/* ── Responsive ─────────────────────────────────────── */

	@media (max-width: 768px) {
		.project-layout {
			grid-template-columns: 1fr;
		}

		.commit-row {
			grid-template-columns: 72px 1fr;
			gap: var(--space-xs);
		}

		.commit-author,
		.commit-time {
			grid-column: 2;
		}

		.tabs {
			overflow-x: auto;
			-webkit-overflow-scrolling: touch;
		}

		.tab {
			white-space: nowrap;
		}
	}
</style>
