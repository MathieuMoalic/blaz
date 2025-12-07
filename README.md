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
