FROM node:24.11.0-bookworm-slim AS build
WORKDIR /src
COPY human/apps/web /src
RUN npm ci --ignore-scripts --no-audit --no-fund
ARG LAYERX_HUMAN_SERVICE_URL=https://layerx-human.layerx-testnet.svc:9443
RUN LAYERX_HUMAN_SERVICE_URL="$LAYERX_HUMAN_SERVICE_URL" npm run build

FROM node:24.11.0-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends socat \
    && rm -rf /var/lib/apt/lists/*
ENV NODE_ENV=production PORT=3000 HOSTNAME=0.0.0.0
WORKDIR /app
COPY --from=build /src/.next/standalone ./
COPY --from=build /src/.next/static ./.next/static
USER 65532:65532
CMD ["node", "server.js"]
