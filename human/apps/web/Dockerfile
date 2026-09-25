FROM node:24.11.0-bookworm-slim AS build
WORKDIR /src/human/apps/web
COPY package.json package-lock.json /src/
COPY agent/sdk/typescript /src/agent/sdk/typescript
RUN npm --prefix /src/agent/sdk/typescript ci --ignore-scripts --no-audit --no-fund
COPY human/apps/web /src/human/apps/web
RUN npm ci --ignore-scripts --no-audit --no-fund
ARG LAYERX_HUMAN_SERVICE_URL=https://layerx-human.layerx-testnet.svc:9443
RUN LAYERX_HUMAN_SERVICE_URL="$LAYERX_HUMAN_SERVICE_URL" npm run build

FROM node:24.11.0-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends socat \
    && rm -rf /var/lib/apt/lists/*
ENV NODE_ENV=production PORT=3000 HOSTNAME=0.0.0.0
WORKDIR /app
COPY --from=build /src/human/apps/web/.next/standalone ./
COPY --from=build /src/human/apps/web/.next/static ./human/apps/web/.next/static
USER 65532:65532
CMD ["node", "human/apps/web/server.js"]
