<script lang="ts">
	import { page } from '$app/state';

	let { owner, project }: { owner: string; project: string } = $props();

	const tabs = [
		{ label: 'Overview', href: `/${owner}/${project}` },
		{ label: 'Data', href: `/${owner}/${project}/data` },
		{ label: 'Collections', href: `/${owner}/${project}/collections` },
		{ label: 'Endpoints', href: `/${owner}/${project}/endpoints` },
		{ label: 'Commits', href: `/${owner}/${project}/commits` },
		{ label: 'Settings', href: `/${owner}/${project}/settings` }
	];

	let currentPath = $derived(page.url.pathname);

	function isActive(href: string): boolean {
		if (href === `/${owner}/${project}`) {
			return currentPath === href;
		}
		return currentPath.startsWith(href);
	}
</script>

<nav class="project-tabs" aria-label="Project navigation">
	{#each tabs as tab}
		<a href={tab.href} class="tab" class:active={isActive(tab.href)}>
			{tab.label}
		</a>
	{/each}
</nav>

<style>
	.project-tabs {
		display: flex;
		gap: var(--space-xs);
		border-bottom: 1px solid var(--border);
		margin-bottom: var(--space-lg);
		overflow-x: auto;
		-webkit-overflow-scrolling: touch;
	}

	.tab {
		padding: var(--space-sm) var(--space-md);
		font-size: 0.875rem;
		font-weight: 500;
		color: var(--text-secondary);
		text-decoration: none;
		border-bottom: 2px solid transparent;
		white-space: nowrap;
		transition:
			color 0.15s,
			border-color 0.15s;
	}

	.tab:hover {
		color: var(--text);
	}

	.tab.active {
		color: var(--pink);
		border-bottom-color: var(--pink);
	}
</style>
