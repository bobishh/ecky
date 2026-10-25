# syntax=docker/dockerfile:1
FROM node:22-bookworm-slim@sha256:de170b39439eb0e5a3444ad7d60b015614a381e10fecae2e30ec9e735122f38d AS hub
WORKDIR /build
COPY package.json package-lock.json tsconfig.json ./
RUN npm ci --ignore-scripts
COPY sites/hub ./sites/hub
COPY sites/landing/src/landing.css sites/landing/src/EckyMascot.svelte ./sites/landing/src/
COPY src/lib/genie ./src/lib/genie
RUN npm run build:hub

FROM rust:1.98.0-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS engine
WORKDIR /build
COPY src-tauri ./src-tauri
RUN cargo build --locked --manifest-path src-tauri/Cargo.toml --release -p cairn-registry -j 2

FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8
RUN useradd --uid 10001 --create-home cairn && mkdir /data && chown cairn:cairn /data
COPY --from=engine /build/src-tauri/target/release/cairn-registry /usr/local/bin/cairn-registry
COPY --from=hub /build/sites/hub/dist /usr/share/cairn
ENV CAIRN_LISTEN=0.0.0.0:8247 CAIRN_DATA_DIR=/data CAIRN_STATIC_DIR=/usr/share/cairn
USER 10001:10001
EXPOSE 8247
VOLUME ["/data"]
ENTRYPOINT ["/usr/local/bin/cairn-registry"]
