# drydemacher

Prompt-driven CAD iterator using LLMs to generate and execute FreeCAD Python macros.

## Prerequisites

- **FreeCAD:** `freecadcmd` must be installed and accessible.
- **Python:** 3.10+ (used by FreeCAD and for the runner script).
- **Node.js:** For the Tauri/Svelte frontend.
- **Rust:** For the Tauri backend.

## Installation

1. Clone the repository.
2. Install dependencies:
   ```bash
   cd drydemacher
   npm install
   ```
3. Configure the application via the in-app settings (⚙️ icon).

## Execution

### Development

Run the Tauri application in development mode:
```bash
npm run tauri dev
```

### Headless Engine

The backend executes Python macros in a headless FreeCAD environment. It injects a global `params` dictionary into the macro scope based on the UI specification returned by the LLM.

## Features

- **Multi-version History:** Persistence via SQLite.
- **Visual Context:** Current viewport screenshots are sent to the LLM for visual feedback during iterations.
- **Design Forking:** Start new threads from existing designs to mutate geometry.
- **Manual Commits:** Edit generated Python code directly and commit as new versions.
- **BRep Modeling:** Focuses on OCCT/Part operations for manifold output (customizable via system prompt).
