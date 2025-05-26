import { useLocation, useNavigate } from "@solidjs/router";
import { createMemo, createRenderEffect, createSignal, JSX, on, onMount, ParentProps, Show } from "solid-js";
import { oidcProviders } from "./config";
import { createStore, produce } from "solid-js/store";
import { jwtDecode, JwtPayload } from "jwt-decode";

const TOKEN_KEY = "jwt_token_local"

export const [authState, setAuthState] = createStore({
    isAuthenticated: false,
    token: null,
    userInfo: null,
    error: null,
});

export const auth_headers = createMemo(() => {
    if (authState.token) {
        console.log("Generating auth headers!");
        return {
            Authorization: `Bearer ${authState.token}`,
        };
    }

    console.log("No token found in auth_state, returning empty headers");
    return undefined;
});

export function AuthGuard(props: ParentProps): JSX.Element {
    const [isAuthenticated, setIsAuthenticated] = createSignal(false);
    const location = useLocation();

    async function performAuthCheck() {
        setIsAuthenticated(false);

        const token = loadToken();
        if (token) {
            setIsAuthenticated(true);
            setIsAuthenticated(true);
        } else {
            // If no token, redirect to login
            console.log(`No token found, redirecting to login: ${new Date()}`);
            login();
        }
    }

    createRenderEffect(on(() => location.pathname, performAuthCheck));

    return (
        <>
            <Show when={isAuthenticated()} fallback={<div>FALLLBACK</div>}>
                {props.children}
            </Show>
        </>
    );
}

export function OAuthCallbackPage() {
    const navigate = useNavigate();

    // Unpack the OAuth Reponse, which comes in the URL 
    onMount(() => {
        const hash = window.location.hash.substring(1);
        const params = new URLSearchParams(hash);

        const idToken = params.get('id_token');
        const error = params.get('error');
        const state = params.get('state'); // Though we don't use state in this flow

        if (error) {
            console.error("OIDC Error:", error);
            setError(`Login failed: ${error}`);
            clearNonce();
            return;
        }

        if (!idToken) {
            console.error("No ID token found in callback.");
            setError("Login callback failed: No token received.");
            clearNonce();
            return;
        }

        try {
            const decoded = jwtDecode(idToken);
            const storedNonce = getStoredNonce();

            console.log("Decoded ID Token:", decoded);
            // if (decoded.nonce !== storedNonce) {
            //     console.error("Nonce mismatch! Possible replay attack.");
            //     setError("Security check failed (Nonce mismatch). Please try again.");
            //     clearNonce();
            //     return;
            // }

            // Nonce is verified, clear it and store the token
            clearNonce();

            localStorage.setItem(TOKEN_KEY, idToken);
            try {
                const decoded = jwtDecode(idToken);
                setAuthState(
                    produce((state) => {
                        state.isAuthenticated = true;
                        state.token = idToken;
                        state.userInfo = {
                            // Add more fields if needed and available
                            // todo
                        };
                        state.error = null;
                    })
                );
            } catch (e) {
                console.error("Failed to decode token:", e);
                setError("Failed to process token.");
                clearToken();
            }

        } catch (e) {
            console.error("Failed to process ID token:", e);
            setError("Failed to process login response.");
            clearNonce();
            clearToken();
        }

        navigate('/', { replace: true });
    });

    return <div>Processing login...</div>;
}

function getStoredNonce() {
    return sessionStorage.getItem(NONCE_KEY);
}

function clearNonce() {
    sessionStorage.removeItem(NONCE_KEY);
}

function storeToken(token) {
    localStorage.setItem(TOKEN_KEY, token);
    try {
        const decoded = jwtDecode(token);
        setAuthState(
            produce((state) => {
                state.isAuthenticated = true;
                state.token = token;
                state.userInfo = {
                    // Add more fields if needed and available
                };
                state.error = null;
            })
        );
    } catch (e) {
        console.error("Failed to decode token:", e);
        setError("Failed to process token.");
        clearToken();
    }
}

function clearToken() {
    localStorage.removeItem(TOKEN_KEY);
    setAuthState(
        produce((state) => {
            state.isAuthenticated = false;
            state.token = null;
            state.userInfo = null;
        })
    );
}

function loadToken() : JwtPayload | null {
    const token = localStorage.getItem(TOKEN_KEY);
    if (token) {
        // Basic check: decode and check expiry before setting state
        try {
            const decoded = jwtDecode(token);
            if (decoded.exp * 1000 > Date.now()) {
                storeToken(token); // Re-store to set state
                console.warn("Storing token");
            } else {
                console.warn("Stored token expired.");
                clearToken();
            }
            return decoded;
        } catch (e) {
            console.error("Failed to load stored token:", e);
            console.error("Clearing token.");
            clearToken();
        }
    }

    return null;
}

function setError(message) {
     setAuthState(
        produce((state) => {
            state.error = message;
        })
    );
}


function login(providerName = 'google') {
    const provider = oidcProviders[providerName];
    if (!provider) {
        console.error(`Provider ${providerName} not configured.`);
        return;
    }

    const nonce = generateNonce();
    const paramss = {
        client_id: provider.clientId,
        redirect_uri: provider.redirectUri,
        response_type: 'id_token', // Request ID Token directly (Implicit Flow)
        scope: provider.scopes.join(' '),
        nonce: nonce,
        prompt: 'select_account', // Optional: forces account selection
    }
    console.log(paramss);

    const params = new URLSearchParams(paramss);



    const url = `${provider.authorizationEndpoint}?${params.toString()}`;
    console.log(`Redirecting to OIDC provider: ${url}`);
    window.location.href = url;
}


const NONCE_KEY = 'oidc_nonce';
function generateNonce() {
    const nonce = crypto.randomUUID();
    sessionStorage.setItem(NONCE_KEY, nonce);
    return nonce;
}