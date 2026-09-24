favicon_folder="./public/assets/favicon/"
master_url="https://raw.githubusercontent.com/blockscout/frontend/main/tools/scripts/favicon.svg"

dotenv \
  -v MASTER_URL=$master_url \
  -- bash -c 'cd ./deploy/tools/favicon-generator && yarn install --frozen-lockfile && node "$(pwd)/index.js"'

if [ -d "$favicon_folder" ]; then
  rm -r "$favicon_folder"
fi
mkdir -p "$favicon_folder"
cp -r ./deploy/tools/favicon-generator/output/* "$favicon_folder"