import fs from 'fs-extra';
import fetch from 'node-fetch';


const baseHost = 'eth.blockscout.com';
const baseName = baseHost.replace(/\./g, '_');
const outputDir = './crate/tests/recorded/'
const paths = [
    // Health endpoints
    "/api/health",
    
    // Blocks endpoints
    "/api/v2/blocks",
    "/api/v2/blocks/12345", // Example block number
    
    // Transactions endpoints
    "/api/v2/transactions",
    "/api/v2/transactions/0x4dd7e3f4522fcf2483ae422fd007492380051d87de6fdb17be71c7134e26857e",
    "/api/v2/transactions/0x4dd7e3f4522fcf2483ae422fd007492380051d87de6fdb17be71c7134e26857e/internal-transactions",
    
    // Smart Contracts endpoints
    "/api/v2/smart-contracts",
    // "/api/v2/smart-contracts?q=contract",
    // "/api/v2/smart-contracts?filter=verified",
    "/api/v2/smart-contracts/0x8FD4596d4E7788a71F82dAf4119D069a84E7d3f3",
    
    // Tokens endpoints
    "/api/v2/tokens",
    // "/api/v2/tokens?q=token",
    // "/api/v2/tokens?type=ERC-20",
    "/api/v2/tokens/0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401",
    "/api/v2/tokens/0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401/instances",
    "/api/v2/tokens/0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401/instances/25625468407840116393736812939389551247551040926951238633020744494000165263268",
    "/api/v2/tokens/0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401/counters",
    "/api/v2/tokens/0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401/holders",
    "/api/v2/tokens/0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401/transfers",
    "/api/v2/tokens/0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401/instances/25625468407840116393736812939389551247551040926951238633020744494000165263268/holders",
    
    // Token Transfers endpoints
    "/api/v2/token-transfers",
    
    // Addresses endpoints
    "/api/v2/addresses/0xc0De20A37E2dAC848F81A93BD85FE4ACDdE7C0DE",
    "/api/v2/addresses/0xc0De20A37E2dAC848F81A93BD85FE4ACDdE7C0DE/counters",
    "/api/v2/addresses/0x8FD4596d4E7788a71F82dAf4119D069a84E7d3f3/logs",
    "/api/v2/addresses/0xc0De20A37E2dAC848F81A93BD85FE4ACDdE7C0DE/nft/collections",
    "/api/v2/addresses/0xc0De20A37E2dAC848F81A93BD85FE4ACDdE7C0DE/token-balances",
    "/api/v2/addresses/0xc0De20A37E2dAC848F81A93BD85FE4ACDdE7C0DE/token-transfers",
    "/api/v2/addresses/0xc0De20A37E2dAC848F81A93BD85FE4ACDdE7C0DE/tokens",
    // Blocks validated
    "/api/v2/addresses/0x4838B106FCe9647Bdf1E7877BF73cE8B0BAD5f97/blocks-validated",
    
    // Search endpoints
    "/api/v2/search?q=USDT",
    
    // Stats endpoints
    "/api/v2/stats",
]



async function main(): Promise<any> {
    for (const path of paths) {
        const url = new URL(`https://${baseHost}${path}`);
        const queryParams = Object.fromEntries(url.searchParams);
        const urlPath = url.pathname;
        const requestObj: any = {
            method: 'GET',
            urlPath
        };
        if (Object.keys(queryParams).length > 0) {
            requestObj['queryParameters'] = {};
            for (const [key, value] of Object.entries(queryParams)) {
                requestObj['queryParameters'][key] = {
                    equalTo: value
                };
            }
        }
        console.log(`Making request to ${url}`);

        const response = await fetch(url);

        // write wiremock stubs
        const responseJson = await response.json();
        const stubs = {
            request: requestObj,
            response: {
                status: 200,
                body: JSON.stringify(responseJson),
                headers: {
                    'Content-Type': 'application/json'
                }
            }
        };
        const sanitazedPath = path.replace(/\//g, '_').replace(/\?/g, '_');
        const outputFileName = `${outputDir}/${baseName}/${sanitazedPath}.json`;
        await fs.ensureFile(outputFileName);
        await fs.writeFile(outputFileName, JSON.stringify(stubs, null, 2));
    }

}

main().catch(console.error);
