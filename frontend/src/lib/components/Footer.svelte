<footer>
	<div class="footer-inner">
		<nav class="links">
			<a href="/docs">Docs</a>
			<a href="https://github.com/rileyleff/ozzydb" target="_blank" rel="noopener noreferrer">GitHub</a>
			<a href="/cli">CLI</a>
		</nav>
		<p class="copyright">&copy; {new Date().getFullYear()} ozzydb</p>
	</div>
</footer>

<style>
	footer {
		background: var(--bg-secondary);
		padding: 2rem 1.5rem;
	}

	.footer-inner {
		max-width: var(--max-width);
		margin: 0 auto;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.75rem;
	}

	.links {
		display: flex;
		gap: 1.5rem;
	}

	.links a {
		color: var(--link);
		text-decoration: none;
		font-size: 12px;
	}

	.links a:hover {
		text-decoration: underline;
	}

	.copyright {
		font-size: 12px;
		color: var(--text-muted);
		margin: 0;
	}
</style>
