# Blaz

> Breton for "flavour" 🧂
>
> _Vibe coded with Claude Sonnet 4.5_

A self-hosted recipe and shopping list manager with a Rust backend and Flutter
frontend.

## Features

- 🧾 **Smart ingredient parsing** - Handles quantities, units, and ranges
  (`2-3 tbsp`, `1.5 L`, etc.)
- 🛒 **Shopping lists** - Auto-categorized items with merge suggestions
- 📘 **Recipe management** - Import from URLs, images, or manual entry
- 🔐 **JWT authentication** - Simple password-based auth
- 📱 **Cross-platform** - Android app + Web interface (embedded in backend)
- 🤖 **LLM integration** - Optional recipe parsing and macro estimation

## Tech Stack

- **Backend:** Rust (Axum, SQLx, SQLite)
- **Frontend:** Flutter (Dart)
- **Deployment:** NixOS module with prebuilt binaries

---

## Local Development

```bash
nix develop
cd backend
just build-web  # Build Flutter web
cargo run       # Run backend (web embedded)
```

## Android App

```bash
nix develop
cd flutter
flutter build apk
```

---

## Configuration

The NixOS module supports:

**Required:**

- `enable` - Enable the service
- `passwordHashFile` - Path to password hash file

**Common:**

- `package` - Use `prebuilt` for fast deployment or `backend` for source build
- `bindAddr` - Server address (default: `127.0.0.1:8080`)
- `corsOrigin` - CORS origin for web access
- `verbosity` - Log level (-2 to 3, default: 0)

**LLM Features (Optional):**

- `llmApiKeyFile` - API key for recipe parsing
- `llmModel` - Model name (default: `deepseek/deepseek-chat`)
- `llmApiUrl` - API endpoint (default: OpenRouter)

See [flake.nix](flake.nix) for all options.

---

## Release Process

```bash
just bump patch         # Bump version and push
# Wait for GitHub Actions...
just update-prebuilt 1.0.12  # Update prebuilt hash
```

---

## License

GPLv3 - See [LICENSE](LICENSE)
