/// <reference types="vite/client" />

type ImportMetaEnv = {
	readonly PUBLIC_GATEWAY_URL?: string;
};

interface ImportMeta {
	readonly env: ImportMetaEnv;
}

interface Window {
	__CLOSEDROUTER_CONFIG__?: {
		gatewayUrl?: string;
	};
}
