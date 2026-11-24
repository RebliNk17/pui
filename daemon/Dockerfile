FROM node:22-bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    git ca-certificates curl ripgrep fd-find \
    && ln -sf /usr/bin/fdfind /usr/local/bin/fd \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY daemon/package.json daemon/package-lock.json* ./daemon/
RUN cd daemon && npm install --omit=dev
COPY daemon/src ./daemon/src
COPY scripts/ultracode.ts ./scripts/ultracode.ts

ENV PUI_DAEMON_BIND=all
ENV PUI_DAEMON_WORKSPACES=/workspaces
ENV PORT=4817
EXPOSE 4817

WORKDIR /app/daemon
CMD ["node", "--experimental-strip-types", "src/index.ts"]
