import { runMerchantApplication } from "../support/merchant-app.mjs";

await runMerchantApplication(import.meta.url, "merchant-checkout");
