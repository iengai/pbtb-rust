/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_API_URL?: string;
  readonly VITE_OAUTH_ISSUER?: string;
  readonly VITE_OAUTH_CLIENT_ID?: string;
  readonly VITE_EGRESS_IP?: string;
  readonly VITE_MOCK_API?: string;
}
