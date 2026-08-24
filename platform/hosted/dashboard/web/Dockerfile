FROM node:24.6.0-bookworm-slim AS build
WORKDIR /src
COPY human/apps/web/packages/layerx-ui /src/human/apps/web/packages/layerx-ui
COPY platform/hosted/dashboard/web /src/platform/hosted/dashboard/web
WORKDIR /src/platform/hosted/dashboard/web
RUN npm install --ignore-scripts --no-audit --no-fund && npm run build

FROM node:24.6.0-bookworm-slim
ENV NODE_ENV=production PORT=3000
WORKDIR /app
COPY --from=build /src/platform/hosted/dashboard/web/.next/standalone ./
COPY --from=build /src/platform/hosted/dashboard/web/.next/static ./.next/static
USER 65532:65532
CMD ["node", "server.js"]
