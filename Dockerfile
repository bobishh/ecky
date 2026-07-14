# syntax=docker/dockerfile:1
#
# Ecky CAD — static landing + docs.
# One image serves:
#   /           → landing (Svelte + Three.js mascot)
#   /docs       → Ecky IR Field Guide (DIY book builder)
#
# Built remotely by Kamal (context: this directory).

# ────────────────────────────────────────────────────────────
# Stage 1 — build the landing (Vite + Svelte)
# ────────────────────────────────────────────────────────────
FROM node:22-alpine AS landing-builder
WORKDIR /repo

# Install landing deps first (cache layer).
COPY sites/landing/package.json sites/landing/package-lock.json* ./sites/landing/
RUN cd sites/landing && npm ci

# Landing imports the genome from src/lib/genie — copy both trees.
COPY sites/landing/ ./sites/landing/
COPY src/lib/genie/ ./src/lib/genie/

RUN cd sites/landing && npm run build

# ────────────────────────────────────────────────────────────
# Stage 2 — build the Ecky IR Field Guide (tsx book builder)
# ────────────────────────────────────────────────────────────
FROM node:22-alpine AS docs-builder
WORKDIR /repo
RUN apk add --no-cache zip

# Install tsx (the only runtime dep the book builder needs).
RUN npm init -y && npm install tsx

# Copy the book builder + its pure-TS dependencies.
COPY scripts/build_ecky_ir_book.ts ./scripts/
COPY src/lib/docs/ ./src/lib/docs/

# Copy the canonical doc source + committed rendered images.
COPY public/docs/ecky-ir.md ./public/docs/ecky-ir.md
COPY docs/books/ecky-ir/assets/ ./target/book/public/docs/assets/

# Run the builder. It writes to target/book/dist/books/.
RUN npx tsx scripts/build_ecky_ir_book.ts

# ────────────────────────────────────────────────────────────
# Stage 3 — nginx serves everything
# ────────────────────────────────────────────────────────────
FROM nginx:alpine AS static

# Landing → / (web root)
COPY --from=landing-builder /repo/sites/landing/dist/ /usr/share/nginx/html/

# Field guide → /docs
COPY --from=docs-builder /repo/target/book/dist/books/ecky-ir-field-guide.html /usr/share/nginx/html/docs/index.html
COPY --from=docs-builder /repo/target/book/dist/books/assets/ /usr/share/nginx/html/docs/assets/
COPY --from=docs-builder /repo/target/book/dist/books/ecky-ir-field-guide.epub /usr/share/nginx/html/docs/ecky-ir-field-guide.epub

COPY nginx.conf /etc/nginx/nginx.conf

EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
