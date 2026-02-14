<script lang="ts">
	import { auth } from '$lib/auth.svelte';

	let dropdownOpen = $state(false);

	function toggleDropdown() {
		dropdownOpen = !dropdownOpen;
	}

	function closeDropdown() {
		dropdownOpen = false;
	}

	function handleLogout() {
		auth.logout();
		closeDropdown();
	}

	function handleDropdownKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			closeDropdown();
		}
	}
</script>

<svelte:window onclick={closeDropdown} />

<nav class="nav">
	<div class="nav-inner">
		<a href="/" class="logo-link">
			<span class="logo-text">OzzyDB</span>
		</a>

		<div class="nav-right">
			{#if auth.isLoggedIn}
				<div class="avatar-wrapper">
					<button
						class="avatar-btn"
						onclick={(e) => { e.stopPropagation(); toggleDropdown(); }}
						aria-expanded={dropdownOpen}
						aria-haspopup="true"
					>
						{#if auth.user?.avatar_url}
							<img
								src={auth.user.avatar_url}
								alt={auth.user.username ?? 'User avatar'}
								class="avatar"
							/>
						{:else}
							<div class="avatar avatar-fallback">
								{auth.user?.username?.[0]?.toUpperCase() ?? '?'}
							</div>
						{/if}
					</button>

					{#if dropdownOpen}
						<div class="dropdown" role="menu" tabindex="0" onclick={(e) => e.stopPropagation()} onkeydown={handleDropdownKeydown}>
							<div class="dropdown-header">
								<span class="dropdown-username">{auth.user?.username ?? 'User'}</span>
								{#if auth.user?.email}
									<span class="dropdown-email">{auth.user.email}</span>
								{/if}
							</div>
							<div class="dropdown-divider"></div>
							<a
								href="/{auth.user?.username}"
								class="dropdown-item"
								role="menuitem"
								onclick={closeDropdown}
							>
								Your projects
							</a>
							{#if auth.user?.is_admin}
								<a
									href="/admin"
									class="dropdown-item"
									role="menuitem"
									onclick={closeDropdown}
								>
									Admin
								</a>
							{/if}
							<div class="dropdown-divider"></div>
							<button class="dropdown-item dropdown-item-danger" role="menuitem" onclick={handleLogout}>
								Sign out
							</button>
						</div>
					{/if}
				</div>
			{:else}
				<a href="/login" class="sign-in-link">Sign in</a>
			{/if}
		</div>
	</div>
</nav>

<style>
	.nav {
		position: fixed;
		top: 0;
		left: 0;
		right: 0;
		z-index: 100;
		height: var(--nav-height);
		background: var(--black);
	}

	.nav-inner {
		display: flex;
		align-items: center;
		justify-content: space-between;
		max-width: var(--max-width);
		margin: 0 auto;
		padding: 0 var(--space-lg);
		height: 100%;
	}

	.logo-link {
		display: flex;
		align-items: center;
		gap: var(--space-sm);
		text-decoration: none;
		color: var(--white);
	}

	.logo-link:hover {
		text-decoration: none;
		color: var(--white);
	}

	.logo-text {
		font-family: var(--font-sans);
		font-size: 18px;
		font-weight: 700;
		letter-spacing: -0.02em;
	}

	.nav-right {
		display: flex;
		align-items: center;
	}

	.sign-in-link {
		color: var(--white);
		font-size: 14px;
		font-weight: 500;
		padding: 6px 14px;
		border-radius: var(--radius);
		transition: background 0.15s;
	}

	.sign-in-link:hover {
		background: rgba(255, 255, 255, 0.1);
		color: var(--white);
		text-decoration: none;
	}

	.avatar-wrapper {
		position: relative;
	}

	.avatar-btn {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 2px;
		border-radius: 50%;
		cursor: pointer;
		transition: box-shadow 0.15s;
	}

	.avatar-btn:hover {
		box-shadow: 0 0 0 2px var(--gray-600);
	}

	.avatar {
		width: 32px;
		height: 32px;
		border-radius: 50%;
		object-fit: cover;
	}

	.avatar-fallback {
		display: flex;
		align-items: center;
		justify-content: center;
		background: var(--gray-700);
		color: var(--white);
		font-size: 14px;
		font-weight: 600;
	}

	.dropdown {
		position: absolute;
		top: calc(100% + var(--space-sm));
		right: 0;
		width: 220px;
		background: var(--white);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
		overflow: hidden;
	}

	.dropdown-header {
		padding: var(--space-sm) var(--space-md);
	}

	.dropdown-username {
		display: block;
		font-weight: 600;
		font-size: 14px;
		color: var(--text);
	}

	.dropdown-email {
		display: block;
		font-size: 12px;
		color: var(--text-secondary);
		margin-top: 2px;
	}

	.dropdown-divider {
		height: 1px;
		background: var(--border);
	}

	.dropdown-item {
		display: block;
		width: 100%;
		padding: var(--space-sm) var(--space-md);
		font-size: 14px;
		color: var(--text);
		text-align: left;
		transition: background 0.1s;
		text-decoration: none;
		cursor: pointer;
	}

	.dropdown-item:hover {
		background: var(--bg-secondary);
		text-decoration: none;
		color: var(--text);
	}

	.dropdown-item-danger:hover {
		background: color-mix(in srgb, var(--error) 10%, transparent);
		color: var(--error);
	}
</style>
