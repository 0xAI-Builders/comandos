# Mobile Web Push design facts

Verified 2026-09-29. Research for product discussion; nothing implemented or tested on a phone. The user's phone platform remains unknown.

## Verified capabilities

- **iPhone/iPad:** WebKit introduced Web Push in iOS/iPadOS 16.4 for web apps added to the Home Screen. Permission must follow a direct user action. Authorized notifications can appear on the Lock Screen, in Notification Center and on a paired Apple Watch; Focus and system notification preferences apply. Source: https://webkit.org/blog/13878/web-push-for-web-apps-on-ios-and-ipados/
- **Android:** A compatible browser can be awakened by an incoming push, then wake the site's service worker even with the page closed. The documented Android flow does not require Home Screen installation. Browser API support, a registered service worker, notification permission and a valid push subscription remain prerequisites. This is not a claim about every Android browser/device or force-stopped apps. Sources: https://web.dev/articles/push-notifications-faq and https://web.dev/articles/push-notifications-overview
- **Opening the relevant item:** A service worker's `notificationclick` handler can focus an existing window or open a URL. This requires a secure context; production should use HTTPS. Routing that URL to the specific ComandOS item is application work, and compatibility must be checked on the actual phone. Source: https://developer.mozilla.org/en-US/docs/Web/API/ServiceWorkerGlobalScope/notificationclick_event
- **Sound:** The Notifications standard exposes silence and vibration preferences, not an arbitrary audio-file option. Default sound follows platform conventions. Custom game sounds cannot be promised for cross-platform Web Push. Source: https://notifications.spec.whatwg.org/
- **Delivery:** The application server submits messages to the browser vendor's push service. Offline devices may receive queued messages after reconnecting, before their TTL expires. Urgency can influence delivery under power constraints. Closing the page is supported; instant delivery is not guaranteed. An unavailable application backend cannot originate new events, although a message already accepted by a push service may still arrive. Source and architectural inference: https://web.dev/articles/push-notifications-overview

## Recommendations for the design discussion

These are proposals, not confirmed user decisions: keep configurable game-style audio within the active app; let the phone govern push sound. Offer an explicit "Activate phone notifications" setup, platform-specific instructions and a real test notification. Send push for selected events that need attention, deduplicate alerts while the relevant view is active, and open the relevant item when tapped. Use the app's activity history to recover missed alerts. Verify the resulting experience on the user's phone before promising locked-screen behavior or timing.
