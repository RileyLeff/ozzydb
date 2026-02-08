import type { UserInfo } from './types';

const TOKEN_KEY = 'ozzy_token';
const USER_KEY = 'ozzy_user';

function createAuth() {
	let token = $state<string | null>(null);
	let user = $state<UserInfo | null>(null);

	// Hydrate from localStorage on init
	if (typeof window !== 'undefined') {
		token = localStorage.getItem(TOKEN_KEY);
		const stored = localStorage.getItem(USER_KEY);
		if (stored) {
			try {
				user = JSON.parse(stored);
			} catch {
				localStorage.removeItem(USER_KEY);
			}
		}
	}

	return {
		get token() {
			return token;
		},
		get user() {
			return user;
		},
		get isLoggedIn() {
			return token !== null;
		},
		login(newToken: string, newUser: UserInfo) {
			token = newToken;
			user = newUser;
			localStorage.setItem(TOKEN_KEY, newToken);
			localStorage.setItem(USER_KEY, JSON.stringify(newUser));
		},
		logout() {
			token = null;
			user = null;
			localStorage.removeItem(TOKEN_KEY);
			localStorage.removeItem(USER_KEY);
		}
	};
}

export const auth = createAuth();
