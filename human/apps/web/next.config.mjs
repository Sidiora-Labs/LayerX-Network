/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "standalone",
  reactStrictMode: true,
  async rewrites() {
    const service = process.env.LAYERX_HUMAN_SERVICE_URL;
    if (service === undefined) {
      return [];
    }
    const endpoint = new URL(service);
    if (
      endpoint.protocol !== "https:" ||
      endpoint.username !== "" ||
      endpoint.password !== "" ||
      endpoint.pathname !== "/" ||
      endpoint.search !== "" ||
      endpoint.hash !== ""
    ) {
      throw new Error("LAYERX_HUMAN_SERVICE_URL must name the HTTPS human service");
    }
    const baseUrl = endpoint.origin;
    return [
      {
        source: "/v1/:path*",
        destination: `${baseUrl}/v1/:path*`,
      },
      {
        source: "/readyz",
        destination: `${baseUrl}/readyz`,
      },
    ];
  },
  async headers() {
    return [
      {
        source: "/explorer",
        headers: [
          {
            key: "Cache-Control",
            value: "public, s-maxage=60, stale-while-revalidate=300",
          },
        ],
      },
      {
        source: "/explorer/:path*",
        headers: [
          {
            key: "Cache-Control",
            value: "public, s-maxage=60, stale-while-revalidate=300",
          },
        ],
      },
      {
        source: "/api/performance/vitals",
        headers: [{ key: "Cache-Control", value: "no-store" }],
      },
    ];
  },
};

export default nextConfig;
