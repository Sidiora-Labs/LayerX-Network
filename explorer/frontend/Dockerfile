# *****************************
# *** STAGE 1: Dependencies ***
# *****************************
FROM node:22.14.0-alpine AS deps
# Check https://github.com/nodejs/docker-node/tree/b4117f9333da4138b03a546ec926ef50a31506c3#nodealpine to understand why libc6-compat might be needed.
RUN apk add --no-cache libc6-compat python3 make g++
RUN ln -sf /usr/bin/python3 /usr/bin/python

### APP
# Install dependencies
WORKDIR /app
COPY package.json yarn.lock tsconfig.json ./
COPY types ./types
COPY lib ./lib
COPY configs/app ./configs/app
COPY toolkit/theme ./toolkit/theme
COPY toolkit/utils ./toolkit/utils
COPY toolkit/components/forms/validators/url.ts ./toolkit/components/forms/validators/url.ts
RUN apk add git
RUN yarn --frozen-lockfile --network-timeout 100000


### FEATURE REPORTER
# Install dependencies
WORKDIR /feature-reporter
COPY ./deploy/tools/feature-reporter/package.json ./deploy/tools/feature-reporter/yarn.lock ./
RUN yarn --frozen-lockfile --network-timeout 100000


### ENV VARIABLES CHECKER
# Install dependencies
WORKDIR /envs-validator
COPY ./deploy/tools/envs-validator/package.json ./deploy/tools/envs-validator/yarn.lock ./
RUN yarn --frozen-lockfile --network-timeout 100000

### FAVICON GENERATOR
# Install dependencies
WORKDIR /favicon-generator
COPY ./deploy/tools/favicon-generator/package.json ./deploy/tools/favicon-generator/yarn.lock ./
RUN yarn --frozen-lockfile --network-timeout 100000

### SITEMAP GENERATOR
# Install dependencies
WORKDIR /sitemap-generator
COPY ./deploy/tools/sitemap-generator/package.json ./deploy/tools/sitemap-generator/yarn.lock ./
RUN yarn --frozen-lockfile --network-timeout 100000

### MULTICHAIN CONFIG GENERATOR
# Install dependencies
WORKDIR /multichain-config-generator
COPY ./deploy/tools/multichain-config-generator/package.json ./deploy/tools/multichain-config-generator/yarn.lock ./
RUN yarn --frozen-lockfile --network-timeout 100000

### ESSENTIAL DAPPS CHAINS CONFIG GENERATOR
# Install dependencies
WORKDIR /essential-dapps-chains-config-generator
COPY ./deploy/tools/essential-dapps-chains-config-generator/package.json ./
RUN yarn --frozen-lockfile --network-timeout 100000

### llms.txt GENERATOR
# Install dependencies
WORKDIR /llms-txt-generator
COPY ./deploy/tools/llms-txt-generator/package.json ./deploy/tools/llms-txt-generator/yarn.lock ./
RUN yarn --frozen-lockfile --network-timeout 100000


# *****************************
# ****** STAGE 2: Build *******
# *****************************
FROM node:22.14.0-alpine AS builder
RUN apk add --no-cache --upgrade libc6-compat bash jq

# pass build args to env variables
ARG GIT_COMMIT_SHA
ENV NEXT_PUBLIC_GIT_COMMIT_SHA=$GIT_COMMIT_SHA
ARG GIT_TAG
ENV NEXT_PUBLIC_GIT_TAG=$GIT_TAG
ARG NEXT_OPEN_TELEMETRY_ENABLED
ENV NEXT_OPEN_TELEMETRY_ENABLED=$NEXT_OPEN_TELEMETRY_ENABLED

ENV NODE_ENV production

### APP
# Copy dependencies and source code
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .

# Build SVG sprite and generate .env.registry with ENVs list and save build args into .env file
RUN set -a && \
    source ./deploy/scripts/build_sprite.sh && \
    ./deploy/scripts/collect_envs.sh ./docs/ENVS.md && \
    set +a

# Next.js collects completely anonymous telemetry data about general usage.
# Learn more here: https://nextjs.org/telemetry
# Uncomment the following line in case you want to disable telemetry during the build.
# ENV NEXT_TELEMETRY_DISABLED 1

# Build app for production
ENV NODE_OPTIONS="--max-old-space-size=8192"
RUN yarn build


### FEATURE REPORTER
# Copy dependencies and source code, then build
COPY --from=deps /feature-reporter/node_modules ./deploy/tools/feature-reporter/node_modules
RUN cd ./deploy/tools/feature-reporter && yarn compile_config
RUN cd ./deploy/tools/feature-reporter && yarn build


### ENV VARIABLES CHECKER
# Copy dependencies and source code, then build
COPY --from=deps /envs-validator/node_modules ./deploy/tools/envs-validator/node_modules
RUN cd ./deploy/tools/envs-validator && yarn build


### FAVICON GENERATOR
# Copy dependencies and source code
COPY --from=deps /favicon-generator/node_modules ./deploy/tools/favicon-generator/node_modules


### SITEMAP GENERATOR
# Copy dependencies and source code
COPY --from=deps /sitemap-generator/node_modules ./deploy/tools/sitemap-generator/node_modules

### MULTICHAIN CONFIG GENERATOR
# Copy dependencies and source code, then build
COPY --from=deps /multichain-config-generator/node_modules ./deploy/tools/multichain-config-generator/node_modules
RUN cd ./deploy/tools/multichain-config-generator && yarn build

### ESSENTIAL DAPPS CHAINS CONFIG GENERATOR
# Copy dependencies and source code, then build
COPY --from=deps /essential-dapps-chains-config-generator/node_modules ./deploy/tools/essential-dapps-chains-config-generator/node_modules
RUN cd ./deploy/tools/essential-dapps-chains-config-generator && yarn build

### llms.txt GENERATOR
# Copy dependencies and source code, then build
COPY --from=deps /llms-txt-generator/node_modules ./deploy/tools/llms-txt-generator/node_modules
RUN cd ./deploy/tools/llms-txt-generator && yarn build


# *****************************
# ******* STAGE 3: Run ********
# *****************************
# Production image, copy all the files and run next
FROM node:22.14.0-alpine AS runner
RUN apk add --no-cache --upgrade bash curl jq unzip

### APP
WORKDIR /app

# Uncomment the following line in case you want to disable telemetry during runtime.
# ENV NEXT_TELEMETRY_DISABLED 1

RUN addgroup --system --gid 1001 nodejs
RUN adduser --system --uid 1001 nextjs

# Set the correct permission for prerender cache
RUN mkdir .next
RUN chown nextjs:nodejs .next

COPY --from=builder /app/next.config.js ./
COPY --from=builder /app/public ./public
COPY --from=builder /app/package.json ./package.json

# Copy tools
COPY --from=builder /app/deploy/tools/envs-validator/dist/index.js ./envs-validator/index.js
COPY --from=builder /app/deploy/tools/feature-reporter/index.js ./feature-reporter.js
COPY --from=builder /app/deploy/tools/multichain-config-generator/dist ./deploy/tools/multichain-config-generator/dist
COPY --from=builder /app/deploy/tools/llms-txt-generator/dist ./deploy/tools/llms-txt-generator/dist
COPY --from=builder /app/deploy/tools/essential-dapps-chains-config-generator/dist ./deploy/tools/essential-dapps-chains-config-generator/dist

# Copy scripts
## Entrypoint
COPY --chmod=755 ./deploy/scripts/entrypoint.sh .
## ENV validator and client script maker
COPY --chmod=755 ./deploy/scripts/validate_envs.sh .
COPY --chmod=755 ./deploy/scripts/make_envs_script.sh .
## Assets downloader
COPY --chmod=755 ./deploy/scripts/download_assets.sh .
## OG image generator
COPY ./deploy/scripts/og_image_generator.js .
## Favicon generator
COPY --chmod=755 ./deploy/scripts/favicon_generator.sh .
COPY --from=builder /app/deploy/tools/favicon-generator ./deploy/tools/favicon-generator
RUN ["chmod", "-R", "777", "./deploy/tools/favicon-generator"]
RUN ["chmod", "-R", "777", "./public"]
## Sitemap generator
COPY --chmod=755 ./deploy/scripts/sitemap_generator.sh .
COPY --from=builder /app/deploy/tools/sitemap-generator ./deploy/tools/sitemap-generator

# Copy ENVs files
COPY --from=builder /app/.env.registry .
COPY --from=builder /app/.env .

# Copy ENVs presets
ARG ENVS_PRESET
ENV ENVS_PRESET=$ENVS_PRESET
COPY ./configs/envs ./configs/envs

# Automatically leverage output traces to reduce image size
# https://nextjs.org/docs/advanced-features/output-file-tracing
COPY --from=builder --chown=nextjs:nodejs /app/.next/standalone ./
COPY --from=builder --chown=nextjs:nodejs /app/.next/static ./.next/static

ENTRYPOINT ["./entrypoint.sh"]

USER nextjs

EXPOSE 3000

ENV PORT 3000

CMD ["node", "server.js"]
