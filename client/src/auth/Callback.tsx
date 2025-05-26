// import { onMount } from 'solid-js';
// import { useNavigate } from '@solidjs/router';
// import { handleCallback } from './AuthService';

// function OAuthCallback() {
//     const navigate = useNavigate();

//     onMount(() => {
//         handleCallback();
//         // Redirect to home or a dashboard after processing
//         navigate('/', { replace: true });
//     });

//     return <div>Processing login...</div>;
// }

// // function handleCallback() {
// //     const hash = window.location.hash.substring(1);
// //     const params = new URLSearchParams(hash);

// //     const idToken = params.get('id_token');
// //     const error = params.get('error');
// //     const state = params.get('state'); // Though we don't use state in this flow

// //     if (error) {
// //         console.error("OIDC Error:", error);
// //         setError(`Login failed: ${error}`);
// //         clearNonce();
// //         return;
// //     }

// //     if (!idToken) {
// //         console.error("No ID token found in callback.");
// //         setError("Login callback failed: No token received.");
// //         clearNonce();
// //         return;
// //     }

// //     try {
// //         const decoded = jwtDecode(idToken);
// //         const storedNonce = getStoredNonce();

// //         // if (decoded.nonce !== storedNonce) {
// //         //     console.error("Nonce mismatch! Possible replay attack.");
// //         //     setError("Security check failed (Nonce mismatch). Please try again.");
// //         //     clearNonce();
// //         //     return;
// //         // }

// //         // Nonce is verified, clear it and store the token
// //         clearNonce();
// //         storeToken(idToken);

// //     } catch (e) {
// //         console.error("Failed to process ID token:", e);
// //         setError("Failed to process login response.");
// //         clearNonce();
// //         clearToken();
// //     }
// // }
