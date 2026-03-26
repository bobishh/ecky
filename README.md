# Ecky CAD

Prompt-driven CAD iterator using LLMs to generate and execute FreeCAD Python macros.

<img width="1520" height="1008" alt="Screenshot 2026-03-05 at 23 25 54" src="https://github.com/user-attachments/assets/7476e9ac-bd47-4d8c-b8b5-148cdcfa95e6" />

## Prerequisites

- **FreeCAD:** `freecadcmd` must be installed and accessible.
- **Python:** 3.10+ (used by FreeCAD and for the runner script).
- **Node.js:** For the Tauri/Svelte frontend.
- **Rust:** For the Tauri backend.

## Installation

1. Clone the repository and `cd` into it.
2. Install dependencies:
   ```bash
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
