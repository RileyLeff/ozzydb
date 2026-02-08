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
			<svg
				class="logo-svg"
				viewBox="0 0 32 32"
				fill="none"
				xmlns="http://www.w3.org/2000/svg"
				aria-hidden="true"
			>
				<!-- Ears -->
				<polygon points="4,4 10,14 2,14" fill="var(--white)" />
				<polygon points="28,4 22,14 30,14" fill="var(--white)" />
				<polygon points="6,6 10,14 4,14" fill="var(--black)" />
				<polygon points="26,6 22,14 28,14" fill="var(--black)" />
				<!-- Head -->
				<ellipse cx="16" cy="20" rx="13" ry="11" fill="var(--white)" />
				<!-- Tuxedo mask -->
				<path d="M8,14 Q16,18 24,14 Q26,20 24,24 Q16,20 8,24 Q6,20 8,14Z" fill="var(--black)" />
				<!-- Eyes -->
				<ellipse cx="11" cy="18" rx="2" ry="2.2" fill="var(--green)" />
				<ellipse cx="21" cy="18" rx="2" ry="2.2" fill="var(--green)" />
				<ellipse cx="11.4" cy="17.6" rx="0.8" ry="0.9" fill="var(--black)" />
				<ellipse cx="21.4" cy="17.6" rx="0.8" ry="0.9" fill="var(--black)" />
				<!-- Nose -->
				<ellipse cx="16" cy="22" rx="1.5" ry="1" fill="var(--pink)" />
				<!-- Mouth -->
				<path d="M14.5,23.5 Q16,25 17.5,23.5" stroke="var(--gray-600)" stroke-width="0.6" fill="none" />
				<!-- Whiskers -->
				<line x1="3" y1="20" x2="10" y2="21" stroke="var(--gray-400)" stroke-width="0.5" />
				<line x1="3" y1="23" x2="10" y2="22.5" stroke="var(--gray-400)" stroke-width="0.5" />
				<line x1="22" y1="21" x2="29" y2="20" stroke="var(--gray-400)" stroke-width="0.5" />
				<line x1="22" y1="22.5" x2="29" y2="23" stroke="var(--gray-400)" stroke-width="0.5" />
			</svg>
			<span class="logo-text">ozzy</span>
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

	.logo-svg {
		width: 28px;
		height: 28px;
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
