import type { ApiData } from './types';
import type { RouteParams } from 'nextjs/types';

import type { Route } from 'nextjs-routes';

import generate from './generate';

const JSON_LD_SCRIPT_ID = 'blockscout-product-schema';

export default function update<Pathname extends Route['pathname']>(route: RouteParams<Pathname>, apiData: ApiData<Pathname>) {
  const { title, description, jsonLd } = generate(route, apiData);

  window.document.title = title;
  window.document.querySelector('meta[name="description"]')?.setAttribute('content', description);

  // Update or create JSON-LD script tag for Product schema
  if (jsonLd) {
    let scriptElement = window.document.getElementById(JSON_LD_SCRIPT_ID) as HTMLScriptElement | null;

    if (!scriptElement) {
      scriptElement = window.document.createElement('script');
      scriptElement.id = JSON_LD_SCRIPT_ID;
      scriptElement.type = 'application/ld+json';
      window.document.head.appendChild(scriptElement);
    }

    scriptElement.textContent = JSON.stringify(jsonLd);
  } else {
    // Remove JSON-LD script if it exists but schema is not needed
    const existingScript = window.document.getElementById(JSON_LD_SCRIPT_ID);
    if (existingScript) {
      existingScript.remove();
    }
  }
}
