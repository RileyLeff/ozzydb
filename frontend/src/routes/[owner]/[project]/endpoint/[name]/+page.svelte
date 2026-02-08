<script lang="ts">
	import { page } from '$app/state';
	import { getProject, getDag, listEndpoints } from '$lib/api';
	import type { ProjectInfo, DagResponse, EndpointInfo } from '$lib/types';
	import CodeSnippet from '$lib/components/CodeSnippet.svelte';

	let loading = $state(true);
	let error: string | null = $state(null);

	let project: ProjectInfo | null = $state(null);
	let dag: DagResponse | null = $state(null);
	let endpoint: EndpointInfo | null = $state(null);

	let owner = $derived(page.params.owner);
	let projectSlug = $derived(page.params.project);
	let endpointName = $derived(page.params.name);

	// Derive data sources used by this endpoint from the DAG edges
	let endpointDataSources = $derived.by(() => {
		if (!endpoint || !dag) return [];
		const sourceNames = new Set<string>();
		for (const edge of endpoint.definition.edges) {
			if (edge.source_type === 'data_source') {
				sourceNames.add(edge.source_ref);
			}
		}
		return dag.data_sources.filter((ds) => sourceNames.has(ds.name));
	});

	// Parse schema fields from data sources
	interface SchemaField {
		name: string;
		type: string;
		nullable: boolean;
		source: string;
	}

	let schemaFields = $derived.by(() => {
		const fields: SchemaField[] = [];
		for (const ds of endpointDataSources) {
			if (!ds.schema) continue;
			try {
				const schema = typeof ds.schema === 'string' ? JSON.parse(ds.schema) : ds.schema;
				// Handle Arrow/Parquet-style schema with "fields" array
				if (schema.fields && Array.isArray(schema.fields)) {
					for (const field of schema.fields) {
						fields.push({
							name: field.name,
							type: formatFieldType(field.type || field.data_type || 'unknown'),
							nullable: field.nullable ?? true,
							source: ds.name
						});
					}
				}
				// Handle simple key-value schema
				else if (typeof schema === 'object') {
					for (const [key, value] of Object.entries(schema)) {
						if (key === 'fields' || key === 'metadata') continue;
						fields.push({
							name: key,
							type: typeof value === 'string' ? value : String(value),
							nullable: true,
							source: ds.name
						});
					}
				}
			} catch {
				// Skip unparseable schemas
			}
		}
		return fields;
	});

	function formatFieldType(type: unknown): string {
		if (typeof type === 'string') return type;
		if (typeof type === 'object' && type !== null) {
			const t = type as Record<string, unknown>;
			if (t.name) return String(t.name);
			if (t.type) return String(t.type);
			return JSON.stringify(type);
		}
		return String(type);
	}

	function typeColor(type: string): string {
		const lower = type.toLowerCase();
		if (lower.includes('int') || lower.includes('float') || lower.includes('double') || lower.includes('decimal') || lower.includes('number')) {
			return 'type-numeric';
		}
		if (lower.includes('str') || lower.includes('utf8') || lower.includes('text') || lower.includes('varchar')) {
			return 'type-string';
		}
		if (lower.includes('bool')) {
			return 'type-bool';
		}
		if (lower.includes('date') || lower.includes('time') || lower.includes('timestamp')) {
			return 'type-temporal';
		}
		if (lower.includes('list') || lower.includes('array') || lower.includes('struct') || lower.includes('map')) {
			return 'type-complex';
		}
		return 'type-other';
	}

	function sourceLabel(edge: { source_type: string; source_ref: string }): string {
		if (edge.source_type === 'data_source') return edge.source_ref;
		if (edge.source_type === 'node') return edge.source_ref;
		return `${edge.source_type}:${edge.source_ref}`;
	}

	$effect(() => {
		const currentOwner = owner;
		const currentProject = projectSlug;
		const currentName = endpointName;

		if (!currentOwner || !currentProject || !currentName) return;

		loading = true;
		error = null;

		Promise.all([
			getProject(currentOwner, currentProject),
			getDag(currentOwner, currentProject),
			listEndpoints(currentOwner, currentProject)
		])
			.then(([projectData, dagData, endpointsData]) => {
				if (currentOwner !== owner || currentProject !== projectSlug || currentName !== endpointName) return;
				project = projectData;
				dag = dagData;

				const match = endpointsData.endpoints.find((e) => e.name === currentName);
				if (match) {
					endpoint = match;
				} else {
					error = 'not_found';
				}
				loading = false;
			})
			.catch((err) => {
				if (currentOwner !== owner || currentProject !== projectSlug || currentName !== endpointName) return;
				if (err.status === 404) {
					error = 'not_found';
				} else {
					error = err.message || 'Failed to load endpoint';
				}
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{endpoint ? `${owner}/${projectSlug}/${endpoint.name}` : `${owner}/${projectSlug}/${endpointName}`} - OzzyDB</title>
</svelte:head>

<div class="container page">
	{#if loading}
		<div class="loading-state">
			<div class="spinner"></div>
			<p class="loading-text">Loading endpoint...</p>
		</div>
	{:else if error === 'not_found'}
		<div class="error-state">
			<h2>Endpoint not found</h2>
			<p>
				The endpoint <span class="mono">{endpointName}</span> does not exist in
				<a href="/{owner}/{projectSlug}">{owner}/{projectSlug}</a>.
			</p>
			<a href="/{owner}/{projectSlug}" class="btn btn-outline">Back to project</a>
		</div>
	{:else if error}
		<div class="error-state">
			<h2>Something went wrong</h2>
			<p>{error}</p>
			<button class="btn btn-outline" onclick={() => location.reload()}>Try again</button>
		</div>
	{:else if endpoint}
		<!-- Breadcrumb -->
		<nav class="breadcrumb" aria-label="Breadcrumb">
			<a href="/{owner}">{owner}</a>
			<span class="separator">/</span>
			<a href="/{owner}/{projectSlug}">{projectSlug}</a>
			<span class="separator">/</span>
			<span class="breadcrumb-segment">endpoints</span>
			<span class="separator">/</span>
			<span class="breadcrumb-current">{endpoint.name}</span>
		</nav>

		<!-- Header -->
		<header class="endpoint-header">
			<h1 class="endpoint-name">{endpoint.name}</h1>
			{#if endpoint.description}
				<p class="endpoint-description">{endpoint.description}</p>
			{/if}
		</header>

		<!-- Pipeline section -->
		<section class="section">
			<h2 class="section-title">Pipeline</h2>

			{#if endpoint.definition.edges.length > 0}
				<div class="pipeline-flow">
					{#each endpoint.definition.edges as edge, i (`${edge.source_ref}-${edge.target_node}-${i}`)}
						<div class="flow-step">
							<div class="flow-source" class:flow-data={edge.source_type === 'data_source'} class:flow-node={edge.source_type === 'node'} class:flow-external={edge.source_type === 'external'}>
								{#if edge.source_type === 'data_source'}
									<span class="flow-label">data</span>
								{:else if edge.source_type === 'node'}
									<span class="flow-label">node</span>
								{:else}
									<span class="flow-label">{edge.source_type}</span>
								{/if}
								<span class="flow-name">{sourceLabel(edge)}</span>
							</div>
							<div class="flow-arrow">
								<svg width="24" height="12" viewBox="0 0 24 12" fill="none">
									<path d="M0 6h20M16 1l5 5-5 5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
								</svg>
								{#if edge.target_input !== 'default'}
									<span class="flow-input-label">{edge.target_input}</span>
								{/if}
							</div>
							<div class="flow-target">
								<span class="flow-label">node</span>
								<span class="flow-name">{edge.target_node}</span>
							</div>
						</div>
					{/each}
				</div>
			{/if}

			{#if endpoint.definition.nodes.length > 0}
				<div class="nodes-table-wrapper">
					<table class="nodes-table">
						<thead>
							<tr>
								<th>Node</th>
								<th>Transform</th>
								<th>Params</th>
							</tr>
						</thead>
						<tbody>
							{#each endpoint.definition.nodes as node (node.node_name)}
								<tr>
									<td class="mono">{node.node_name}</td>
									<td class="mono">{node.transform_name}</td>
									<td>
										{#if node.params && Object.keys(node.params).length > 0}
											<div class="params-list">
												{#each Object.entries(node.params) as [key, value] (key)}
													<span class="param-chip">
														<span class="param-key">{key}</span>=<span class="param-value">{JSON.stringify(value)}</span>
													</span>
												{/each}
											</div>
										{:else}
											<span class="text-muted">none</span>
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</section>

		<!-- Schema section -->
		{#if schemaFields.length > 0}
			<section class="section">
				<h2 class="section-title">Schema</h2>
				<div class="schema-table-wrapper">
					<table class="schema-table">
						<thead>
							<tr>
								<th>Field</th>
								<th>Type</th>
								<th>Nullable</th>
								{#if endpointDataSources.length > 1}
									<th>Source</th>
								{/if}
							</tr>
						</thead>
						<tbody>
							{#each schemaFields as field (field.name + field.source)}
								<tr>
									<td class="mono field-name">{field.name}</td>
									<td>
										<span class="type-badge {typeColor(field.type)}">{field.type}</span>
									</td>
									<td>
										{#if field.nullable}
											<span class="nullable-yes">yes</span>
										{:else}
											<span class="nullable-no">no</span>
										{/if}
									</td>
									{#if endpointDataSources.length > 1}
										<td class="mono text-muted">{field.source}</td>
									{/if}
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>
		{/if}

		<!-- Usage section -->
		<section class="section">
			<h2 class="section-title">Usage</h2>
			<div class="usage-grid">
				<CodeSnippet
					code={`import ozzydb\n\ndf = ozzydb.fetch("${owner}/${projectSlug}/${endpoint.name}")\nprint(df.head())`}
					language="python"
					title="Python"
				/>
				<CodeSnippet
					code={`ozzy fetch ${owner}/${projectSlug}/${endpoint.name}`}
					language="bash"
					title="CLI"
				/>
			</div>
		</section>
	{/if}
</div>

<style>
	/* -- Page layout ---------------------------------------- */

	.page {
		padding-top: var(--space-xl);
		padding-bottom: var(--space-3xl);
	}

	/* -- Loading / Error states ----------------------------- */

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

	/* -- Breadcrumb ----------------------------------------- */

	.breadcrumb {
		display: flex;
		align-items: center;
		gap: var(--space-xs);
		font-size: 14px;
		margin-bottom: var(--space-lg);
	}

	.breadcrumb a {
		font-weight: 500;
	}

	.separator {
		color: var(--text-muted);
	}

	.breadcrumb-segment {
		color: var(--text-secondary);
	}

	.breadcrumb-current {
		font-family: var(--font-mono);
		font-weight: 600;
		color: var(--text);
	}

	/* -- Endpoint header ------------------------------------ */

	.endpoint-header {
		padding-bottom: var(--space-lg);
		border-bottom: 1px solid var(--border);
		margin-bottom: var(--space-xl);
	}

	.endpoint-name {
		font-family: var(--font-mono);
		font-size: 1.75rem;
		font-weight: 700;
		line-height: 1.3;
	}

	.endpoint-description {
		margin-top: var(--space-sm);
		color: var(--text-secondary);
		font-size: 15px;
	}

	/* -- Sections ------------------------------------------- */

	.section {
		margin-bottom: var(--space-xl);
	}

	.section-title {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: var(--space-md);
		color: var(--text);
	}

	/* -- Pipeline flow -------------------------------------- */

	.pipeline-flow {
		display: flex;
		flex-direction: column;
		gap: var(--space-sm);
		margin-bottom: var(--space-lg);
	}

	.flow-step {
		display: flex;
		align-items: center;
		gap: var(--space-md);
	}

	.flow-source,
	.flow-target {
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: var(--space-sm) var(--space-md);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg);
		min-width: 120px;
	}

	.flow-source.flow-data {
		border-color: color-mix(in srgb, var(--green) 50%, var(--border));
		background: color-mix(in srgb, var(--green) 4%, var(--bg));
	}

	.flow-source.flow-node {
		border-color: color-mix(in srgb, var(--pink) 40%, var(--border));
		background: color-mix(in srgb, var(--pink) 4%, var(--bg));
	}

	.flow-source.flow-external {
		border-color: color-mix(in srgb, var(--gray-400) 50%, var(--border));
		background: var(--bg-secondary);
	}

	.flow-target {
		border-color: color-mix(in srgb, var(--pink) 40%, var(--border));
		background: color-mix(in srgb, var(--pink) 4%, var(--bg));
	}

	.flow-label {
		font-size: 10px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-muted);
	}

	.flow-name {
		font-family: var(--font-mono);
		font-size: 13px;
		font-weight: 500;
	}

	.flow-arrow {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		color: var(--text-muted);
		flex-shrink: 0;
	}

	.flow-input-label {
		font-size: 10px;
		color: var(--text-muted);
		font-family: var(--font-mono);
	}

	/* -- Nodes table ---------------------------------------- */

	.nodes-table-wrapper {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.nodes-table {
		width: 100%;
		border-collapse: collapse;
	}

	.nodes-table th {
		text-align: left;
		padding: var(--space-sm) var(--space-md);
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-secondary);
		background: var(--bg-secondary);
		border-bottom: 1px solid var(--border);
	}

	.nodes-table td {
		padding: var(--space-sm) var(--space-md);
		font-size: 13px;
		border-bottom: 1px solid var(--border);
	}

	.nodes-table tr:last-child td {
		border-bottom: none;
	}

	.params-list {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-xs);
	}

	.param-chip {
		display: inline-flex;
		align-items: center;
		padding: 1px 6px;
		font-family: var(--font-mono);
		font-size: 12px;
		background: var(--bg-inset);
		border: 1px solid var(--border);
		border-radius: 4px;
	}

	.param-key {
		color: var(--text-secondary);
	}

	.param-value {
		color: var(--pink);
	}

	/* -- Schema table --------------------------------------- */

	.schema-table-wrapper {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}

	.schema-table {
		width: 100%;
		border-collapse: collapse;
	}

	.schema-table th {
		text-align: left;
		padding: var(--space-sm) var(--space-md);
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-secondary);
		background: var(--bg-secondary);
		border-bottom: 1px solid var(--border);
	}

	.schema-table td {
		padding: var(--space-sm) var(--space-md);
		font-size: 13px;
		border-bottom: 1px solid var(--border);
	}

	.schema-table tr:last-child td {
		border-bottom: none;
	}

	.field-name {
		font-weight: 500;
	}

	.type-badge {
		display: inline-flex;
		align-items: center;
		padding: 1px 8px;
		font-family: var(--font-mono);
		font-size: 12px;
		font-weight: 500;
		border-radius: 999px;
		line-height: 1.5;
	}

	.type-numeric {
		background: color-mix(in srgb, #5b8def 12%, transparent);
		color: #4a7de0;
		border: 1px solid color-mix(in srgb, #5b8def 25%, transparent);
	}

	.type-string {
		background: color-mix(in srgb, var(--green) 12%, transparent);
		color: var(--green-dim);
		border: 1px solid color-mix(in srgb, var(--green) 25%, transparent);
	}

	.type-bool {
		background: color-mix(in srgb, #d4a843 12%, transparent);
		color: #b8922e;
		border: 1px solid color-mix(in srgb, #d4a843 25%, transparent);
	}

	.type-temporal {
		background: color-mix(in srgb, #9b6fcf 12%, transparent);
		color: #8a5fc0;
		border: 1px solid color-mix(in srgb, #9b6fcf 25%, transparent);
	}

	.type-complex {
		background: color-mix(in srgb, var(--pink) 12%, transparent);
		color: var(--pink);
		border: 1px solid color-mix(in srgb, var(--pink) 25%, transparent);
	}

	.type-other {
		background: var(--bg-inset);
		color: var(--text-secondary);
		border: 1px solid var(--border);
	}

	.nullable-yes {
		color: var(--text-muted);
		font-size: 12px;
	}

	.nullable-no {
		color: var(--text);
		font-size: 12px;
		font-weight: 500;
	}

	/* -- Usage ---------------------------------------------- */

	.usage-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-md);
	}

	/* -- Shared --------------------------------------------- */

	.text-muted {
		color: var(--text-muted);
		font-size: 12px;
	}

	/* -- Responsive ----------------------------------------- */

	@media (max-width: 768px) {
		.usage-grid {
			grid-template-columns: 1fr;
		}

		.flow-step {
			flex-wrap: wrap;
		}

		.flow-source,
		.flow-target {
			min-width: 0;
			flex: 1;
		}
	}
</style>
