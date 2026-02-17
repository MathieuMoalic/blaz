# **Blaz**

Blaz is a cross-platform **recipe + shopping-list application** featuring:

- A **Rust backend** with a powerful ingredient parser
- A **Flutter frontend** for a clean, modern UI
- **SQLite** persistence
- **JWT authentication**
- Automatic unit handling, ingredient normalization, and structured shopping
  list management

Blaz aims to make recipes easier to manage, ingredients easier to understand,
and shopping lists easier to generate.

---

## 🚀 Features

### **🧾 Ingredient Parsing**

- Parses free-form recipe lines such as:

  - `120 g flour`
  - `2–3 tbsp sugar`
  - `1.5 L water`
  - `2 carrots, diced`
- Extracts:

  - Quantity (range or singular)
  - Unit (with normalization + synonyms)
  - Clean ingredient name
- Falls back gracefully when parsing is ambiguous.

### **🛒 Shopping Lists**

- Structured shopping-list items
- Categories for items (optional)
- “Done” state for crossing out items
- Stored in SQLite and exposed via REST API

### **📘 Recipes**

- Support for:

  - Recipe introduction
  - Ingredients list
  - Steps
  - Tags
  - Nutrition and yield info
- Recipes saved in embedded SQLite using JSON fields

### **🔐 Authentication**

- JWT-based user system
- Configurable secret keys
- Secure login and token validation

### **⚙️ Tech Stack**

- **Backend:** Rust, Axum, SQLx, tokio
- **Frontend:** Flutter (Dart)
- **Database:** SQLite
- **Build:** Nix flake support, Justfile for common tasks

---

## 🚢 NixOS Deployment

Blaz includes a NixOS module for easy deployment. The backend binary includes the embedded Flutter web frontend.

### Option 1: Prebuilt Binary (Recommended - Fast!)

Use prebuilt binaries from GitHub releases (no compilation needed):

> **Note:** The prebuilt package references the latest *published* release. During active development, source builds may be one version ahead. See [RELEASE.md](RELEASE.md) for details.

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    blaz.url = "github:MathieuMoalic/blaz";
  };

  outputs = { nixpkgs, blaz, ... }: {
    nixosConfigurations.yourhost = nixpkgs.lib.nixosSystem {
      modules = [
        blaz.nixosModules.blaz-service
        {
          services.blaz = {
            enable = true;
            package = blaz.packages.x86_64-linux.prebuilt;  # Use prebuilt binary!
            bindAddr = "127.0.0.1:8080";
            passwordHashFile = "/run/secrets/blaz-password-hash";
          };
        }
      ];
    };
  };
}
```

### Option 2: Build from Source

Build from source (takes longer but always up-to-date):

```nix
services.blaz = {
  enable = true;
  # package = blaz.packages.x86_64-linux.backend;  # Optional: explicit source build
  bindAddr = "127.0.0.1:8080";
  passwordHashFile = "/run/secrets/blaz-password-hash";
};
```

### Generate Password Hash

```bash
# Build and run the binary to generate a password hash
nix run github:MathieuMoalic/blaz#prebuilt hash-password
# Or if building from source:
nix run github:MathieuMoalic/blaz hash-password
```

### Complete Example with Caddy

```nix
services.blaz = {
  enable = true;
  package = blaz.packages.x86_64-linux.prebuilt;
  bindAddr = "127.0.0.1:8080";
  passwordHashFile = "/run/secrets/blaz-password-hash";
  llmApiKeyFile = "/run/secrets/openrouter-api-key";  # Optional
};

services.caddy = {
  enable = true;
  virtualHosts."blaz.yourdomain.com".extraConfig = ''
    reverse_proxy localhost:8080
  '';
};
```

---
