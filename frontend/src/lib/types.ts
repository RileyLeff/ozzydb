export interface UserInfo {
	id: string;
	username: string;
	email: string | null;
	avatar_url: string | null;
}

export interface ProjectInfo {
	id: string;
	owner: string;
	slug: string;
	description: string | null;
	visibility: string;
	default_branch: string;
	created_at: string;
	updated_at: string;
}

export interface DeviceCodeResponse {
	device_code: string;
	user_code: string;
	verification_uri: string;
	expires_in: number;
	interval: number;
}

export interface AuthResponse {
	access_token: string | null;
	token_type: string | null;
	user: UserInfo | null;
	pending: boolean;
}

export interface ApiErrorResponse {
	error: string;
	message: string;
}
