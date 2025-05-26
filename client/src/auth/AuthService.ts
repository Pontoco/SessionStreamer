// // src/AuthService.js
// import { createStore, produce } from 'solid-js/store';
// import { oidcProviders } from './config';
// import { jwtDecode } from 'jwt-decode';
// import { createResource } from 'solid-js';

// const NONCE_KEY = 'oidc_nonce';
// const TOKEN_KEY = 'id_token';

// // const [authState, setAuthState] = createStore({
// //     isAuthenticated: false,
// //     token: null,
// //     userInfo: null,
// //     error: null,
// // });

// const [token, { refetch }] = createResource(async () => {
//     const storedToken = localStorage.getItem('jwt_token');

//     if (storedToken) {
//         return storedToken;
//     }



//     // navigate to google for now
//     const 
// });



// // --- Helper Functions ---

// function generateNonce() {
//     const nonce = crypto.randomUUID();
//     sessionStorage.setItem(NONCE_KEY, nonce);
//     return nonce;
// }

// function getStoredNonce() {
//     return sessionStorage.getItem(NONCE_KEY);
// }

// function clearNonce() {
//     sessionStorage.removeItem(NONCE_KEY);
// }

// function storeToken(token) {
//     localStorage.setItem(TOKEN_KEY, token);
//     try {
//         const decoded = jwtDecode(token);
//         setAuthState(
//             produce((state) => {
//                 state.isAuthenticated = true;
//                 state.token = token;
//                 state.userInfo = {
//                     // Add more fields if needed and available
//                 };
//                 state.error = null;
//             })
//         );
//     } catch (e) {
//         console.error("Failed to decode token:", e);
//         setError("Failed to process token.");
//         clearToken();
//     }
// }

// function clearToken() {
//     localStorage.removeItem(TOKEN_KEY);
//     setAuthState(
//         produce((state) => {
//             state.isAuthenticated = false;
//             state.token = null;
//             state.userInfo = null;
//         })
//     );
// }

// function loadToken() {
//     const token = localStorage.getItem(TOKEN_KEY);
//     if (token) {
//         // Basic check: decode and check expiry before setting state
//         try {
//             const decoded = jwtDecode(token);
//             if (decoded.exp * 1000 > Date.now()) {
//                 storeToken(token); // Re-store to set state
//             } else {
//                 console.warn("Stored token expired.");
//                 clearToken();
//             }
//         } catch (e) {
//             console.error("Failed to load stored token:", e);
//             clearToken();
//         }
//     }
// }

// function setError(message) {
//      setAuthState(
//         produce((state) => {
//             state.error = message;
//         })
//     );
// }

// // --- OIDC Core Logic ---

// function login(providerName = 'google') {
//     const provider = oidcProviders[providerName];
//     if (!provider) {
//         console.error(`Provider ${providerName} not configured.`);
//         return;
//     }
    
//     if(!provider.clientId || provider.clientId === 'YOUR_GOOGLE_CLIENT_ID.apps.googleusercontent.com') {
//         setError("OIDC Client ID is not configured. Please set it in src/config.js");
//         console.error("OIDC Client ID is not configured.");
//         return;
//     }

//     const nonce = generateNonce();
//     const params = new URLSearchParams({
//         client_id: provider.clientId,
//         redirect_uri: provider.redirectUri,
//         response_type: 'id_token', // Request ID Token directly (Implicit Flow)
//         scope: provider.scopes.join(' '),
//         nonce: nonce,
//         prompt: 'select_account', // Optional: forces account selection
//     });

//     window.location.href = `${provider.authorizationEndpoint}?${params.toString()}`;
// }

// function logout() {
//     clearToken();
//     clearNonce();
//     // Optional: Redirect to a logged-out page or home
//     window.location.href = '/';
//     // Note: True OIDC logout might involve redirecting to Google's logout endpoint,
//     // but for a stateless client, simply deleting the token is often sufficient.
// }


// // --- API Calls ---

// async function fetchWithAuth(url, options = {}) {
//     const token = authState.token;
//     if (!token) {
//         return Promise.reject(new Error("Not authenticated"));
//     }

//     const headers = {
//         'Authorization': `Bearer ${token}`,
//         'Content-Type': 'application/json', // Assuming JSON APIs
//     };

//     try {
//         const response = await fetch(url, { ...options, headers });
//         if (!response.ok) {
//             // Handle potential 401/403 (token expired/invalid)
//             if (response.status === 401 || response.status === 403) {
//                  console.warn("API returned unauthorized. Logging out.");
//                  logout();
//             }
//             throw new Error(`API request failed with status ${response.status}`);
//         }
//         // Check if response is JSON before parsing
//         const contentType = response.headers.get("content-type");
//         if (contentType && contentType.indexOf("application/json") !== -1) {
//              return response.json();
//         } else {
//             return response.text();
//         }
//     } catch(err) {
//         console.error("API Fetch Error:", err);
//         throw err;
//     }
// }


// // --- Exports ---
// export { authState, login, logout, handleCallback, loadToken, fetchWithAuth };