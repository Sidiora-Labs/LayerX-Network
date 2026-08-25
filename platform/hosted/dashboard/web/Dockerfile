FROM node:24.6.0-bookworm-slim AS build
WORKDIR /src
COPY human/apps/web/package.json human/apps/web/package-lock.json /src/human/apps/web/
COPY human/apps/web/packages/layerx-ui /src/human/apps/web/packages/layerx-ui
COPY platform/hosted/dashboard/web /src/platform/hosted/dashboard/web
COPY tools/ci/developer-dashboard-lock.mjs /src/tools/ci/developer-dashboard-lock.mjs
RUN node tools/ci/developer-dashboard-lock.mjs
RUN npm --prefix /src/human/apps/web ci --ignore-scripts --no-audit --no-fund && \
    npm --prefix /src/human/apps/web run build --workspace @layerx/ui && \
    ln -s ../../../../human/apps/web/node_modules /src/platform/hosted/dashboard/web/node_modules
WORKDIR /src/platform/hosted/dashboard/web
RUN npm run test:static && npm run build

FROM node:24.6.0-bookworm-slim
ENV NODE_ENV=production PORT=3000
WORKDIR /app
COPY --from=build /src/platform/hosted/dashboard/web/.next/standalone ./
COPY --from=build /src/platform/hosted/dashboard/web/.next/static ./.next/static
USER 65532:65532
CMD ["node", "server.js"]
