# Admin Panel Phase 1.5: Matrix Authentication

## Overview

Add proper Matrix-based authentication to the admin panel before implementing channel management (Phase 2).

## Current State

- Localhost access allowed if no API key configured
- X-API-Key header required for remote access
- No login page or session management

## Proposed Approach: Matrix Login API

Use the standard Matrix Client-Server login API (same as Element, Cinny, etc.):

### Flow

1. `GET /admin/login` - Show login form (Matrix user ID + password)
2. `POST /admin/login` - Authenticate via Matrix API:
   - Parse homeserver from user ID (e.g., `@user:matrix.org` → `matrix.org`)
   - Call `POST /_matrix/client/v3/login` with credentials
   - Verify response contains valid user ID
   - Check user ID is in `allowed_users` config
   - Create session cookie (don't store Matrix token)
3. `GET /admin/logout` - Clear session cookie

### Alternative: SSO Redirect (Future Enhancement)

If homeserver supports SSO:
1. Redirect to `/_matrix/client/v3/login/sso/redirect?redirectUrl=...`
2. User authenticates on homeserver's web UI
3. Homeserver redirects back with `loginToken`
4. Exchange token, verify user, create session

This only works if the homeserver has SSO configured (matrix.org does, self-hosted may not).

### Hybrid Approach (Ideal)

1. Try to detect if homeserver supports SSO
2. If yes, use SSO redirect flow
3. If no, fall back to password form

## Technical Details

### Session Storage

Use `tower-sessions` with memory store (already added in Phase 1):
- Session ID in secure cookie
- Store authenticated user ID in session
- Session expiry: 24 hours (configurable)

### Templates Needed

- `templates/admin/login.html` - Login form
- Update `templates/base.html` - Show logged-in user, logout link

### Routes

```
GET  /admin/login          → Login form (unprotected)
POST /admin/login          → Process login
GET  /admin/logout         → Clear session, redirect to login
```

### Auth Middleware Update

Current middleware checks API key or localhost. Update to:
1. Check for valid session cookie first
2. If no session, check API key header (for programmatic access)
3. If no API key, check localhost
4. If none, redirect to `/admin/login`

## Security Considerations

1. **Password handling**: Password sent to Matrix API only, never stored
2. **HTTPS**: Should be used in production (passwords in transit)
3. **Session security**: Secure cookie flags, reasonable expiry
4. **Rate limiting**: Prevent brute force (future enhancement)

## Success Criteria

- [ ] Can log in with Matrix credentials
- [ ] Session persists across page loads
- [ ] Logout clears session
- [ ] Only `allowed_users` can access admin
- [ ] API key header still works for programmatic access

## Dependencies

- Phase 1 complete (base admin panel)
- `tower-sessions` already added

## Estimated Effort

Medium - requires Matrix API integration and session management updates.
