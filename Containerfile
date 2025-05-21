# ---- Cargo chef / Builder stage----
FROM rust:latest as chef

RUN cargo install cargo-chef

WORKDIR /usr/src/app

FROM chef AS planner
COPY ./server ./server
COPY ./vendor ./vendor

RUN cd server && cargo chef prepare --recipe-path recipe.json

FROM --platform=linux/amd64 chef as builder

# Install build dependencies for Rust and GStreamer
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    # Recommended from: 
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-bad1.0-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app/server

COPY --from=planner /usr/src/app/server/recipe.json recipe.json

# Vendor is part of our dependencies
COPY ./vendor ../vendor

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

# Install client tools
RUN apt-get install -y deno
RUN deno install -g npm:vite

# Build the server binary.
COPY ./server .
RUN cargo build --release --bin main

# Build the client distribution.
WORKDIR /usr/src/app/client
COPY ./client .
RUN deno run build

# ---- Runner Stage ----
FROM --platform=linux/amd64 debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    # GStreamer runtime libraries
    libgstreamer1.0-0 \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    gstreamer1.0-tools \
    gstreamer1.0-x \
    gstreamer1.0-alsa \
    gstreamer1.0-gl \
    gstreamer1.0-gtk3 \
    gstreamer1.0-qt5 \
    gstreamer1.0-pulseaudio \
    ca-certificates \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app

# Copy the compiled binary from the builder stage
# Adjust 'server' if your binary name is different
COPY --from=builder /usr/src/app/server/target/release/main .
COPY --from=builder /usr/src/app/client/dist ./client_dist

# Expose the port. 
EXPOSE 3000

# Use a more detailed logging for the server by default.
ENV RUST_LOG "info,gstreamer=warn,server=debug"

ENTRYPOINT ["/usr/src/app/main", "--use-structured-logging", "--client-files=./client_dist"]
