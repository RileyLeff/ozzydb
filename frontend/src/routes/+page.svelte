<script>
	import WavyDivider from '$lib/components/WavyDivider.svelte';
	import CodeSnippet from '$lib/components/CodeSnippet.svelte';

	const pythonExample = `import ozzydb

# Fetch a remote endpoint as a Polars DataFrame
df = ozzydb.fetch("alice/weather/daily-forecast")
print(df.head())`;

	const cliExample = `# Initialize a project and run locally
ozzy init my-pipeline
ozzy data add measurements.parquet
ozzy transform add clean.py
ozzy endpoint create forecast --pipeline "measurements -> clean"
ozzy run forecast`;

	const quickStart = `uv add ozzydb
python -c "import ozzydb; print(ozzydb.fetch('alice/weather/daily-forecast'))"`;

</script>

<!-- Hero -->
<section class="hero">
	<div class="container hero-inner">
		<img src="/logo.png" alt="OzzyDB" class="hero-logo" width="160" height="178" />
		<h1 class="hero-tagline">Content-addressed data pipelines</h1>
		<p class="hero-subtitle">
			Version, share, and reproduce data pipelines with cryptographic guarantees.
			Git for data pipelines.
		</p>

		<div class="hero-ctas">
			<a href="/docs" class="btn btn-primary btn-lg">Get started</a>
			<a
				href="https://github.com/rileyleff/ozzydb"
				class="btn btn-ghost"
				target="_blank"
				rel="noopener noreferrer"
			>
				<svg
					xmlns="http://www.w3.org/2000/svg"
					width="18"
					height="18"
					viewBox="0 0 24 24"
					fill="currentColor"
					aria-hidden="true"
				>
					<path
						d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"
					/>
				</svg>
				View on GitHub
			</a>
		</div>

		<div class="hero-code">
			<CodeSnippet code={quickStart} language="bash" title="Quick start" />
		</div>
	</div>
</section>

<!-- Divider -->
<WavyDivider />

<!-- How it works -->
<section class="how-it-works">
	<div class="container">
		<h2 class="section-heading">How it works</h2>

		<div class="features-grid">
			<div class="feature-card">
				<div class="feature-icon">
					<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						<rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
						<path d="M7 11V7a5 5 0 0110 0v4" />
					</svg>
				</div>
				<h3 class="feature-title">Content-addressed</h3>
				<p class="feature-description">
					Every data source, transform, and output is hashed with BLAKE3.
					Same inputs always produce the same cache key.
				</p>
			</div>

			<div class="feature-card">
				<div class="feature-icon">
					<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						<polyline points="16 18 22 12 16 6" />
						<polyline points="8 6 2 12 8 18" />
					</svg>
				</div>
				<h3 class="feature-title">Reproducible</h3>
				<p class="feature-description">
					Platform fingerprinting and deterministic execution. Your pipeline
					runs the same everywhere.
				</p>
			</div>

			<div class="feature-card">
				<div class="feature-icon">
					<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						<path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2" />
						<circle cx="9" cy="7" r="4" />
						<path d="M23 21v-2a4 4 0 00-3-3.87" />
						<path d="M16 3.13a4 4 0 010 7.75" />
					</svg>
				</div>
				<h3 class="feature-title">Collaborative</h3>
				<p class="feature-description">
					Push and pull projects like git. Share endpoints across teams
					and organizations.
				</p>
			</div>
		</div>
	</div>
</section>

<!-- Code examples -->
<section class="code-examples">
	<div class="container">
		<h2 class="section-heading">See it in action</h2>

		<div class="examples-grid">
			<div class="example-block">
				<h3 class="example-label">Python client</h3>
				<CodeSnippet code={pythonExample} language="python" title="fetch.py" />
			</div>

			<div class="example-block">
				<h3 class="example-label">CLI workflow</h3>
				<CodeSnippet code={cliExample} language="bash" title="terminal" />
			</div>
		</div>
	</div>
</section>

<style>
	/* ── Hero ─────────────────────────────────────────────── */

	.hero {
		background: var(--black);
		color: var(--white);
		padding: var(--space-3xl) 0 calc(var(--space-3xl) + 24px);
	}

	.hero-inner {
		display: flex;
		flex-direction: column;
		align-items: center;
		text-align: center;
	}

	.hero-logo {
		width: 160px;
		height: auto;
		margin-bottom: var(--space-lg);
		image-rendering: pixelated;
		border-radius: var(--radius-lg);
	}

	.hero-tagline {
		font-size: clamp(2rem, 5vw, 3.5rem);
		font-weight: 800;
		letter-spacing: -0.03em;
		line-height: 1.1;
		max-width: 720px;
	}

	.hero-subtitle {
		margin-top: var(--space-lg);
		font-size: clamp(1rem, 2vw, 1.25rem);
		color: var(--gray-400);
		max-width: 540px;
		line-height: 1.6;
	}

	.hero-ctas {
		display: flex;
		gap: var(--space-md);
		margin-top: var(--space-xl);
		flex-wrap: wrap;
		justify-content: center;
	}

	.btn-lg {
		padding: 12px 28px;
		font-size: 15px;
		font-weight: 600;
		border-radius: var(--radius);
	}

	.btn-ghost {
		display: inline-flex;
		align-items: center;
		gap: var(--space-sm);
		padding: 12px 28px;
		font-size: 15px;
		font-weight: 500;
		color: var(--white);
		border: 1px solid var(--gray-600);
		border-radius: var(--radius);
		transition: background 0.15s, border-color 0.15s;
	}

	.btn-ghost:hover {
		background: rgba(255, 255, 255, 0.08);
		border-color: var(--gray-400);
		color: var(--white);
		text-decoration: none;
	}

	.hero-code {
		margin-top: var(--space-2xl);
		width: 100%;
		max-width: 520px;
	}

	/* ── How it works ─────────────────────────────────────── */

	.how-it-works {
		padding: var(--space-3xl) 0;
		background: var(--bg);
	}

	.section-heading {
		font-size: clamp(1.5rem, 3vw, 2rem);
		font-weight: 700;
		letter-spacing: -0.02em;
		text-align: center;
		margin-bottom: var(--space-2xl);
		color: var(--text);
	}

	.features-grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: var(--space-xl);
	}

	.feature-card {
		padding: var(--space-xl);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		transition: border-color 0.2s, box-shadow 0.2s;
	}

	.feature-card:hover {
		border-color: var(--border-strong);
		box-shadow: 0 4px 16px rgba(0, 0, 0, 0.06);
	}

	.feature-icon {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 44px;
		height: 44px;
		border-radius: var(--radius);
		background: color-mix(in srgb, var(--pink) 10%, transparent);
		color: var(--pink);
		margin-bottom: var(--space-md);
	}

	.feature-title {
		font-size: 1.125rem;
		font-weight: 600;
		letter-spacing: -0.01em;
		margin-bottom: var(--space-sm);
		color: var(--text);
	}

	.feature-description {
		font-size: 0.9375rem;
		color: var(--text-secondary);
		line-height: 1.6;
	}

	/* ── Code examples ────────────────────────────────────── */

	.code-examples {
		padding: var(--space-2xl) 0 var(--space-3xl);
		background: var(--bg-secondary);
	}

	.examples-grid {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: var(--space-xl);
	}

	.example-block {
		display: flex;
		flex-direction: column;
		gap: var(--space-md);
		min-width: 0;
	}

	.example-label {
		font-size: 0.875rem;
		font-weight: 600;
		color: var(--text-secondary);
		text-transform: uppercase;
		letter-spacing: 0.05em;
	}

	/* ── Responsive ───────────────────────────────────────── */

	@media (max-width: 768px) {
		.features-grid {
			grid-template-columns: 1fr;
		}

		.examples-grid {
			grid-template-columns: 1fr;
		}

		.hero {
			padding: var(--space-2xl) 0 calc(var(--space-2xl) + 16px);
		}

		.hero-code {
			max-width: 100%;
		}
	}
</style>
