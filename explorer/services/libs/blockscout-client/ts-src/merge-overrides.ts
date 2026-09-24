import * as fs from 'fs';
import * as path from 'path';
import { parse, stringify } from 'yaml';
import { merge } from 'lodash';

const OVERRIDES_DIR = path.join(__dirname, '..', 'swaggers', 'overrides');
const MAIN_SPEC_PATH = path.join(__dirname, '..', 'swaggers', 'blockscout-api-v2.yaml');
const OUTPUT_PATH = path.join(__dirname, '..', 'swaggers', 'blockscout-api-final.yaml');

type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };

function removeMarkedFields(obj: JsonValue): JsonValue {
    if (Array.isArray(obj)) {
        return obj.map(item => removeMarkedFields(item));
    }
    if (obj && typeof obj === 'object') {
        const result: { [key: string]: JsonValue } = {};
        for (const [key, value] of Object.entries(obj)) {
            if (value === '__REMOVE__') {
                continue; // Skip this field
            }
            result[key] = removeMarkedFields(value);
        }
        return result;
    }
    return obj;
}

async function mergeOverrides() {
    try {
        // Read the main spec file
        const mainSpecContent = fs.readFileSync(MAIN_SPEC_PATH, 'utf8');
        let mergedSpec = parse(mainSpecContent);

        // Read all YAML files from the overrides directory
        const overrideFiles = fs.readdirSync(OVERRIDES_DIR)
            .filter(file => file.endsWith('.yaml'));

        // Sort files to ensure consistent merging order
        overrideFiles.sort();

        // Apply each override
        for (const file of overrideFiles) {
            const overridePath = path.join(OVERRIDES_DIR, file);
            const overrideContent = fs.readFileSync(overridePath, 'utf8');
            const overrideSpec = parse(overrideContent);

            // Deep merge the override with the main spec
            mergedSpec = merge(mergedSpec, overrideSpec);
        }

        // Remove marked fields
        mergedSpec = removeMarkedFields(mergedSpec);

        // Write the merged result back to the main spec file
        const mergedYaml = stringify(mergedSpec);
        fs.writeFileSync(OUTPUT_PATH, mergedYaml);

        console.log(`Successfully merged all overrides into ${OUTPUT_PATH}`);
    } catch (error) {
        console.error('Error merging overrides:', error);
        process.exit(1);
    }
}

mergeOverrides(); 