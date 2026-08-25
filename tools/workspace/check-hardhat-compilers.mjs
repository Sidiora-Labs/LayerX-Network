import { createRequire } from "node:module";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const requirements = [
  ["paxeer-network/contracts", "0.8.28"],
  ["paxeer-network/integration_test/dapp_tests", "0.8.20"],
  ["paxeer-network/integration_test/rpc_tests", "0.8.28"],
];

for (const [directory, version] of requirements) {
  const require = createRequire(pathToFileURL(path.resolve(directory, "package.json")));
  const { CompilerDownloader, CompilerPlatform } = require("hardhat/internal/solidity/compiler/downloader");
  const { getCompilersDir } = require("hardhat/internal/util/global-dir");
  const compilersDir = await getCompilersDir();
  const platforms = [CompilerDownloader.getCompilerPlatform(), CompilerPlatform.WASM];
  for (const platform of new Set(platforms)) {
    const downloader = CompilerDownloader.getConcurrencySafeDownloader(platform, compilersDir);
    if (!(await downloader.isCompilerDownloaded(version))) {
      console.error(`${directory}: Hardhat solc ${version} for ${platform} is not installed`);
      process.exitCode = 1;
    }
  }
}
