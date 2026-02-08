<script lang="ts">
	import '../app.css';
	import Nav from '$lib/components/Nav.svelte';
	import Footer from '$lib/components/Footer.svelte';
	import favicon from '$lib/assets/favicon.svg';

	let { children } = $props();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
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
