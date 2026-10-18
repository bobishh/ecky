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

# Landing imports shared mascot geometry and the pure Ecky lexer from the
# repository root.
COPY sites/landing/ ./sites/landing/
COPY src/lib/genie/ ./src/lib/genie/
COPY src/lib/eckyLexer.ts ./src/lib/eckyLexer.ts
COPY model-runtime/examples/dovetail-box.ecky ./model-runtime/examples/dovetail-box.ecky
COPY docs/books/ecky-ir/examples/frame-array-bracket.ecky ./docs/books/ecky-ir/examples/frame-array-bracket.ecky

RUN cd sites/landing && npm run build

# ────────────────────────────────────────────────────────────
# Stage 2 — build the Ecky IR Field Guide (tsx book builder)
# ────────────────────────────────────────────────────────────
FROM node:22-alpine AS docs-builder
WORKDIR /repo
RUN apk add --no-cache zip

# Install tsx (the only runtime dep the book builder needs).
RUN npm init -y && npm install tsx

# Copy the book builders + projection pipeline + pure-TS dependencies.
COPY scripts/build_ecky_ir_book.ts scripts/build_ecky_ir_docs_site.ts scripts/ecky_ir_content.ts scripts/ecky_ir_source.ts scripts/worm_farm_sources.ts ./scripts/
COPY src/lib/docs/ ./src/lib/docs/

# Copy the canonical corpus, six static chapter sources, exact Ecky
# checkpoints, and committed rendered images. Chapter pages read source
# files directly; copying only the manifest would make the production build
# depend on files absent from the image.
COPY docs/books/ecky-ir/ ./docs/books/ecky-ir/
COPY sites/landing/src/models/ ./sites/landing/src/models/
COPY docs/books/ecky-ir/assets/ ./target/book/public/docs/assets/

# Build both: the EPUB (offline download) and the themed web docs site.
RUN npx tsx scripts/build_ecky_ir_book.ts && npx tsx scripts/build_ecky_ir_docs_site.ts

# ────────────────────────────────────────────────────────────
# Stage 3 — nginx serves everything
# ────────────────────────────────────────────────────────────
FROM nginx:alpine AS static

# Landing → / (web root)
COPY --from=landing-builder /repo/sites/landing/dist/ /usr/share/nginx/html/

# Field guide → /docs
# Paged server-rendered HTML + mobile navigation shell
COPY --from=docs-builder /repo/target/book/dist/docs-site/ /usr/share/nginx/html/docs/
COPY --from=docs-builder /repo/target/book/dist/books/assets/ /usr/share/nginx/html/docs/assets/
# Raw markdown for agents/LLMs
COPY --from=docs-builder /repo/public/docs/ecky-ir.md /usr/share/nginx/html/docs/ecky-ir.md
# EPUB for offline reading
COPY --from=docs-builder /repo/target/book/dist/books/ecky-ir-field-guide.epub /usr/share/nginx/html/docs/ecky-ir-field-guide.epub

COPY nginx.conf /etc/nginx/nginx.conf

EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
