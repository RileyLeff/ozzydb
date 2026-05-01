<script>
	import WavyDivider from '$lib/components/WavyDivider.svelte';
	import GraphBackground from '$lib/components/GraphBackground.svelte';

	const repoUrl = 'https://github.com/rileyleff/ozzydb';
	const guideUrl = `${repoUrl}/blob/main/docs/getting_started.md`;
</script>

<svelte:head>
	<title>OzzyDB</title>
	<meta
		name="description"
		content="OzzyDB keeps track of what happens to scientific data: raw observations, typed artifacts, versioned transforms, and reproducible fetches."
	/>
</svelte:head>

<section class="hero">
	<GraphBackground />
	<div class="container hero-inner">
		<div class="hero-copy">
			<img src="/logo.png" alt="OzzyDB" class="hero-logo" width="160" height="178" />
			<h1 class="hero-tagline">OzzyDB keeps track of what happens to your data.</h1>
			<p class="hero-subtitle">
				Start with raw observations. Attach typed artifacts to transforms. Stack those
				transforms into reproducible endpoints that can be queried, rerun, cached, and cited.
			</p>

			<div class="hero-ctas">
				<a href={guideUrl} class="btn btn-primary btn-lg" target="_blank" rel="noopener noreferrer">
					Get started
				</a>
				<a href={repoUrl} class="btn btn-ghost" target="_blank" rel="noopener noreferrer">
					View on GitHub
				</a>
			</div>

			<p class="access-note">
				The hosted service is live, but account access is currently restricted to my GitHub
				username. To try OzzyDB today, run it locally with Docker Compose.
			</p>
		</div>

		<div class="hero-demo" aria-label="OzzyDB workflow example">
			<div class="demo-panel">
				<div class="panel-bar">
					<span class="bar-title">ozzy.toml</span>
				</div>
				<pre class="panel-body"><code>[project]
name = "sensor-qc"

[types.RawReadings]
definition = "readings.schema.ozzy"

[transforms.clean]
source = "transforms/clean.py:clean"</code></pre>
			</div>

			<div class="demo-panel">
				<div class="panel-bar">
					<span class="bar-title">terminal</span>
				</div>
				<pre class="panel-body"><code><span class="prompt">$</span> ozzy push
<span class="prompt">$</span> ozzy artifact upload readings.csv
<span class="prompt">$</span> ozzy artifact conformance &lt;artifact-id&gt; --type RawReadings@1
<span class="prompt">$</span> ozzy fetch acme/sensor-qc/clean --input raw=&lt;artifact-id&gt;</code></pre>
			</div>
		</div>
	</div>
</section>

<WavyDivider />

<main>
	<section class="section">
		<div class="container narrow">
			<p class="section-kicker">What OzzyDB Is</p>
			<h2>Recipes, artifacts, and the durable relationship between them.</h2>
			<p>
				OzzyDB stores a project as a graph of reproducible data recipes. Your source data is
				recorded as artifacts. Your transforms live as ordinary text in git. OzzyDB records
				the environment, inputs, outputs, type claims, and execution history needed to make
				those recipes meaningful again later.
			</p>
			<p>
				That gives scientific data infrastructure a better compression scheme than giant
				file diffs. A unit conversion on a billion-row CSV should be described by the
				instructions that performed it, not by pretending the resulting billion changed rows
				are the most useful description of what happened. Somewhere, Kolmogorov has
				complaints.
			</p>
		</div>
	</section>

	<section class="section tinted">
		<div class="container split">
			<div>
				<p class="section-kicker">Why This Exists</p>
				<h2>Scientific data does not have the infrastructure it needs.</h2>
				<p>
					Git is wonderful for code, and transforms are code. That is the nice little
					"ah yeah, that makes sense" part: OzzyDB can store the instructions themselves
					in git, then separately track the artifacts those instructions consume and
					produce.
				</p>
				<p>
					I tried building earlier versions of this on top of GitHub Actions. The shape
					was close enough to be tempting, but too brittle to trust. Actions can run
					jobs, but they do not naturally understand the durable relationship between
					a versioned recipe, a typed input, the materialized output, and the cache that
					keeps those pieces in sync.
				</p>
			</div>
			<div class="callout">
				<h3>Fragmented by default</h3>
				<p>
					In science, every organization eventually builds its own database. The more
					general the database becomes, the more it tends to collapse into a wrapper
					around CSVs in S3 with we-trust-you metadata. OzzyDB is an attempt to add a
					useful standard without pulling an <a href="https://xkcd.com/927/" target="_blank" rel="noopener noreferrer">xkcd 927</a>.
				</p>
			</div>
		</div>
	</section>

	<section class="section">
		<div class="container status-grid">
			<div>
				<p class="section-kicker">What I Have Learned So Far</p>
				<h2>Status and direction</h2>
			</div>
			<article>
				<h3>OzzyDB needs types to become pleasant.</h3>
				<p>
					Bytes can go in and bytes can come out, but that is too inconvenient for the
					thing I actually want. Scientific workflows need to say what information is
					preserved, what information is destroyed, what assumptions were introduced,
					and what kind of output identity is being claimed.
				</p>
			</article>
			<article>
				<h3>Myco is the other half of the shape.</h3>
				<p>
					I have been building <a href="https://github.com/rileyleff/myco" target="_blank" rel="noopener noreferrer">Myco</a>
					as a language and compiler for declarative scientific models. Its layered
					e-graph approach is a promising way to eventually represent opaque external
					operations, lossiness, partial invertibility, overdetermination, and user policy.
					I expect the two projects to converge, but forcing that merge too early would
					shortcut what Myco still needs to learn on its own.
				</p>
			</article>
		</div>
	</section>

	<section class="section tinted">
		<div class="container">
			<p class="section-kicker">How OzzyDB Works Today</p>
			<h2>Current pieces</h2>
			<div class="feature-grid">
				<article class="feature">
					<h3>Projects live in repos.</h3>
					<p>
						A project repository contains transform code plus an <code>ozzy.toml</code>.
						That file currently defines types, transforms, and named endpoints. It works,
						but it is not ergonomic yet and will be replaced with something nicer.
					</p>
				</article>
				<article class="feature">
					<h3>Transforms are version controlled as text.</h3>
					<p>
						OzzyDB leans on git for recipe history, then records the graph that connects
						those recipes to typed artifacts, executions, and fetchable outputs.
					</p>
				</article>
				<article class="feature">
					<h3>Artifacts can be materialized or cached.</h3>
					<p>
						Frequently accessed or expensive transforms can be cached while the recipe
						remains the source of truth. The storage and compute tradeoff becomes a
						policy choice instead of an accidental mess.
					</p>
				</article>
				<article class="feature">
					<h3>Transforms can become endpoints.</h3>
					<p>
						Named endpoints give users a stable API-shaped surface for common workflows.
						In the long run, this also points toward DOI-like citation for exact
						transform versions and their outputs.
					</p>
				</article>
			</div>
		</div>
	</section>

	<section class="section final-section">
		<div class="container final-inner">
			<div>
				<p class="section-kicker">Try It Locally</p>
				<h2>Agent-friendly by design.</h2>
				<p>
					OzzyDB is CLI-driven, so it should be fairly friendly to coding agents. Let
					your favorite agent read the repository for context, then start the local stack
					with Docker Compose.
				</p>
			</div>
			<pre class="install"><code>git clone https://github.com/rileyleff/ozzydb.git
cd ozzydb
docker compose -f crates/ozzy-server/docker/docker-compose.yml up --build</code></pre>
		</div>
	</section>
</main>

<style>
	.hero {
		position: relative;
		overflow: hidden;
		background: var(--black);
		color: var(--white);
		padding: var(--space-3xl) 0 calc(var(--space-3xl) + 28px);
	}

	.hero-inner {
		position: relative;
		z-index: 1;
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(340px, 520px);
		gap: var(--space-3xl);
		align-items: center;
	}

	.hero-copy {
		max-width: 720px;
	}

	.hero-logo {
		width: 112px;
		height: auto;
		margin-bottom: var(--space-lg);
		image-rendering: pixelated;
		border-radius: var(--radius-lg);
	}

	.section-kicker {
		font-family: var(--font-mono);
		font-size: 0.75rem;
		font-weight: 700;
		letter-spacing: 0.08em;
		text-transform: uppercase;
	}

	.hero-tagline {
		max-width: 760px;
		font-size: clamp(2.4rem, 5vw, 4.8rem);
		font-weight: 800;
		letter-spacing: 0;
		line-height: 1.02;
	}

	.hero-subtitle {
		margin-top: var(--space-lg);
		max-width: 660px;
		font-size: clamp(1rem, 2vw, 1.25rem);
		color: var(--gray-300);
		line-height: 1.65;
	}

	.hero-ctas {
		display: flex;
		gap: var(--space-md);
		margin-top: var(--space-xl);
		flex-wrap: wrap;
	}

	.btn-lg,
	.btn-ghost {
		padding: 12px 22px;
		font-size: 15px;
	}

	.btn-ghost {
		display: inline-flex;
		align-items: center;
		color: var(--white);
		border: 1px solid var(--gray-600);
		border-radius: var(--radius);
		transition:
			background 0.15s,
			border-color 0.15s;
	}

	.btn-ghost:hover {
		background: rgba(255, 255, 255, 0.08);
		border-color: var(--gray-400);
		color: var(--white);
		text-decoration: none;
	}

	.access-note {
		margin-top: var(--space-lg);
		max-width: 580px;
		color: var(--gray-400);
		font-size: 0.94rem;
		line-height: 1.6;
	}

	.hero-demo {
		display: flex;
		flex-direction: column;
		gap: var(--space-md);
	}

	.demo-panel {
		overflow: hidden;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: var(--radius-lg);
		box-shadow: 0 20px 60px rgba(0, 0, 0, 0.22);
	}

	.panel-bar {
		display: flex;
		align-items: center;
		padding: 10px 14px;
		background: rgba(255, 255, 255, 0.04);
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
	}

	.bar-title {
		font-family: var(--font-mono);
		font-size: 0.78rem;
		color: var(--gray-300);
	}

	.panel-body {
		margin: 0;
		padding: 16px;
		overflow-x: auto;
		background: var(--gray-900);
		color: var(--gray-100);
		font-family: var(--font-mono);
		font-size: 0.86rem;
		line-height: 1.72;
		white-space: pre;
	}

	.panel-body code {
		font-size: inherit;
	}

	.prompt {
		color: var(--pink);
	}

	.section {
		padding: var(--space-3xl) 0;
		background: var(--bg);
	}

	.tinted {
		background: var(--bg-secondary);
	}

	.narrow {
		max-width: 880px;
	}

	.section-kicker {
		color: var(--pink);
		margin-bottom: var(--space-md);
	}

	h2 {
		max-width: 820px;
		font-size: clamp(1.7rem, 3vw, 2.5rem);
		font-weight: 760;
		letter-spacing: 0;
		line-height: 1.14;
		color: var(--text);
	}

	h3 {
		font-size: 1.08rem;
		font-weight: 700;
		letter-spacing: 0;
		color: var(--text);
	}

	p {
		font-size: 1rem;
		color: var(--text-secondary);
		line-height: 1.72;
	}

	h2 + p,
	h3 + p,
	p + p {
		margin-top: var(--space-md);
	}

	.split {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(280px, 420px);
		gap: var(--space-2xl);
		align-items: start;
	}

	.callout {
		padding: var(--space-xl);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--bg);
	}

	.status-grid {
		display: grid;
		grid-template-columns: minmax(240px, 0.9fr) repeat(2, minmax(0, 1fr));
		gap: var(--space-xl);
		align-items: start;
	}

	.status-grid article {
		padding-top: 6px;
	}

	.feature-grid {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: var(--space-lg);
		margin-top: var(--space-2xl);
	}

	.feature {
		padding: var(--space-lg);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--bg);
	}

	.feature p {
		font-size: 0.94rem;
	}

	.final-section {
		padding-bottom: calc(var(--space-3xl) + var(--space-lg));
	}

	.final-inner {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(320px, 560px);
		gap: var(--space-2xl);
		align-items: center;
	}

	.install {
		margin: 0;
		padding: var(--space-lg);
		overflow-x: auto;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--gray-900);
		color: var(--gray-100);
		font-family: var(--font-mono);
		font-size: 0.9rem;
		line-height: 1.7;
	}

	.install code {
		font-size: inherit;
	}

	@media (max-width: 1040px) {
		.hero-inner,
		.status-grid,
		.final-inner {
			grid-template-columns: 1fr;
		}

		.hero-demo,
		.final-inner {
			max-width: 720px;
		}

		.feature-grid {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
	}

	@media (max-width: 760px) {
		.hero {
			padding: var(--space-2xl) 0 calc(var(--space-2xl) + 20px);
		}

		.hero-inner,
		.split {
			grid-template-columns: 1fr;
			gap: var(--space-xl);
		}

		.hero-logo {
			width: 92px;
		}

		.hero-ctas {
			align-items: stretch;
			flex-direction: column;
		}

		.hero-ctas a {
			justify-content: center;
		}

		.feature-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
