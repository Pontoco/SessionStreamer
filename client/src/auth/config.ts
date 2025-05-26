// src/config.js

const REDIRECT_PAGE = '/oauth_callback'

// IMPORTANT: Must match exactly the redirect URI configured in Google Cloud Console
const REDIRECT_URI = `${window.location.origin}${REDIRECT_PAGE}`

export const oidcProviders = {
    google: {
        clientId: "250832464539-0m471qro1qad8108jel2kqu3dbcaldii.apps.googleusercontent.com",
        authorizationEndpoint: 'https://accounts.google.com/o/oauth2/v2/auth',
        redirectUri: REDIRECT_URI,
        scopes: ['openid', 'email', 'profile'], // Request email and basic profile
    },
    // You can add other providers here later
};
