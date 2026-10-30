# GIPHY GIF search

The launcher uses one application API key for all launcher users. It is injected at build time through `VITE_GIPHY_API_KEY`; end users do not enter or store their own keys.

For local development, copy `.env.example` to `.env.local`, add the app's GIPHY API key, and restart `npm run tauri:dev` so Vite loads the new value. `.env.local` is ignored by Git. Release builds must receive `VITE_GIPHY_API_KEY` from the release build environment.

This is a client-side API key: Vite embeds it in the frontend bundle. Only use a GIPHY key intended for the launcher app; do not put unrelated server credentials here. GIPHY requires its API calls to originate client-side and requires visible “Powered by GIPHY” attribution. Beta keys are rate limited; request production access before a public release with higher usage.

References: [GIPHY API quick start](https://developers.giphy.com/docs/api/quick-start-guide/), [GIPHY Search endpoint](https://developers.giphy.com/docs/api/endpoint/search/), [GIPHY integration guidelines](https://developers.giphy.com/docs/api/best-practices/).
