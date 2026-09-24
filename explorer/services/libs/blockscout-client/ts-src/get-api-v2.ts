import axios from 'axios';
import fs from 'fs-extra';
import yaml from 'js-yaml';
import {Swagger} from "atlassian-openapi";


const blockscoutApiV2Url = 'https://raw.githubusercontent.com/blockscout/blockscout-api-v2-swagger/main/swagger.yaml';
const outputFilePath = './swaggers/blockscout-api-v2.yaml';

async function downloadSwaggerFile(url: string): Promise<any> {
    try {
        const response = await axios.get(url);
        console.log(`Downloaded Swagger file from ${url}`);
        return response.data;
    } catch (error) {
        console.error(`Error downloading Swagger file: ${error}`);
    }
}

function guessTagFromPath(path: string): string {
    const parts = path.split('/');
    if (parts.length > 1) {
        return parts[1];
    }
    return 'default';
}

function patchSwagger(swaggerData: Swagger.SwaggerV3): Swagger.SwaggerV3 {
    try {
        swaggerData.servers?.forEach(server => {
            if (server.url.endsWith('api/v2/')) {
                server.url = server.url.replace('api/v2/', '');
            }
            if (server.url.startsWith('http://')) {
                server.url = server.url.replace('http://', 'https://');
            }
            if (server.variables?.hasOwnProperty('server')) {
                server.variables.server.default = 'eth.blockscout.com';
            }
        })

        if (swaggerData.paths) {
            Object.keys(swaggerData.paths).forEach(path => {
                Object.keys(swaggerData.paths[path]).forEach(method => {
                    const pathItem = swaggerData.paths[path] as any;
                    if (!pathItem[method].tags) {
                        const tag = guessTagFromPath(path);
                        pathItem[method].tags = [tag];
                    }

                    if (!path.startsWith('/api/v')) {
                        // rename path to /api/v2/...
                        const newPath = `/api/v2${path}`;
                        swaggerData.paths[newPath] = swaggerData.paths[path];
                        delete swaggerData.paths[path];
                    }
                });
            });
        }

        console.log(`Applied changes to swagger`);
        return swaggerData;
    } catch (error) {
        console.error(`Error applying changes to Swagger file: ${error}`);
        throw error;
    }
}

async function saveAsYamlFile(swaggerData: any, filePath: string): Promise<void> {
    const newSwaggerYaml = yaml.dump(swaggerData);
    await fs.writeFile(filePath, newSwaggerYaml);
}

async function getApiV2(): Promise<void> {
    const swaggerYaml = await downloadSwaggerFile(blockscoutApiV2Url);
    const swaggerData = patchSwagger(yaml.load(swaggerYaml) as any);
    await saveAsYamlFile(swaggerData, outputFilePath);
}

getApiV2().catch(error => console.error(`Error in get api script: ${error.message}`));
