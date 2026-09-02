export type ProviderKind = 'openai' | 'anthropic' | 'deepseek' | 'glm';

export type ModelCapability = 'chat' | 'embedding';

export type StatusPayload = {
	status: string;
	version: string;
	keys: number;
	providers: number;
	models: number;
	host: string;
	port: number;
	embedding_dim?: number;
	langfuse?: boolean;
};

export type ApiKey = {
	id: string;
	name: string;
	key_prefix: string;
	created_at: number;
	revoked_at: number | null;
};

export type CreatedKey = ApiKey & {
	key: string;
};

export type Provider = {
	id: string;
	name: string;
	kind: ProviderKind;
	base_url: string;
	has_api_key: boolean;
	created_at: number;
};

export type Model = {
	id: string;
	provider_id: string;
	provider_name: string;
	provider_kind: ProviderKind;
	upstream_model: string;
	display_name: string;
	capability: ModelCapability;
	created_at: number;
};

export type ListResponse<T> = {
	data: T[];
};

export type GatewayError = {
	error: {
		message: string;
		type: string;
	};
};
