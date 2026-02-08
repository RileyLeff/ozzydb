<script lang="ts">
	import '../app.css';
	import Nav from '$lib/components/Nav.svelte';
	import Footer from '$lib/components/Footer.svelte';
	let { children } = $props();
</script>

<svelte:head>
	<link rel="icon" type="image/png" href="/favicon.png" />
	<meta property="og:image" content="/logo.png" />
</svelte:head>

<Nav />

<main>
	{@render children()}
</main>

<Footer />

<style>
	main {
		min-height: calc(100vh - var(--nav-height));
		padding-top: var(--nav-height);
	}
</style>
