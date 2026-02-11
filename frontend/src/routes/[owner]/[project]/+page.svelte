<script lang="ts">
	import { page } from '$app/state';

	let owner = $derived(page.params.owner);
	let projectSlug = $derived(page.params.project);
</script>

<svelte:head>
	<title>{owner}/{projectSlug} - OzzyDB</title>
</svelte:head>

<div class="container page">
	<header class="project-header">
		<div class="breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="separator">/</span>
			<span class="project-name">{projectSlug}</span>
		</div>
	</header>

	<div class="placeholder">
		<h2>Project view coming in v2</h2>
		<p>
			The project detail page is being rebuilt for the new data management architecture.
		</p>
	</div>
</div>

<style>
	.page {
		padding-top: var(--space-xl);
		padding-bottom: var(--space-3xl);
	}

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

	.placeholder {
		text-align: center;
		padding: var(--space-3xl) var(--space-lg);
		margin-top: var(--space-xl);
		background: var(--bg-secondary);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
	}

	.placeholder h2 {
		font-size: 1.25rem;
		font-weight: 600;
		margin-bottom: var(--space-sm);
	}

	.placeholder p {
		color: var(--text-secondary);
		max-width: 420px;
		margin-left: auto;
		margin-right: auto;
	}
</style>
